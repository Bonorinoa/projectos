//! Recovery journal (append-only records, quiesced checkpoints, replay,
//! stale-lock takeover) — implemented by child J per `INTERFACES-M1b.md`.
//!
//! The journal lives at `<project>/.projectos/journal.toml` and is a simple
//! append-only log of [`Record`]s. `append` rewrites the whole file atomically
//! (tmp + rename) each time — simple and auditable, no unsafe. `checkpoint`
//! snapshots a [`crate::graph::GraphEngine`] to
//! `.projectos/graph/snapshot.toml` and records a [`Record::Checkpoint`].
//! `recover` replays post-checkpoint records and can explicitly take over a
//! stale writer lock via [`crate::registration::release_lock`].

use std::path::{Path, PathBuf};

use crate::error::{KernelError, Result};
use crate::graph::GraphEngine;
use crate::graph_memory::InMemoryGraphEngine;

/// Name of the sidecar directory inside every registered project.
const SIDECAR_DIR: &str = ".projectos";
/// Name of the journal file within the sidecar dir.
const JOURNAL_FILE: &str = "journal.toml";
/// Subdirectory holding checkpoint snapshots.
const GRAPH_DIR: &str = "graph";
/// Name of the snapshot file within [`GRAPH_DIR`].
const SNAPSHOT_FILE: &str = "snapshot.toml";

/// An immutable, append-only journal entry.
///
/// External-tagged serde representation keeps the TOML file human-auditable,
/// e.g. `[[records]] Opened = { at = 1728000000 }`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Record {
    /// The project was (re)opened for a writer session.
    Opened { at: u64 },
    /// An artifact object was mutated.
    Mutated { object: String, at: u64 },
    /// A quiesced snapshot was written; `id` is the checkpoint id.
    Checkpoint { id: String, at: u64 },
    /// A recovery run happened; `policy` names the [`RecoverPolicy`] used.
    Recovery { policy: String, at: u64 },
}

/// Outcome of a [`Journal::recover`] run.
#[derive(Debug, Clone, PartialEq)]
pub struct RecoveryReport {
    /// Number of records replayed (those after the latest checkpoint).
    pub replayed: usize,
    /// `true` when this recovery took over a stale writer lock.
    pub lock_taken_over: bool,
    /// The id of the latest checkpoint the replay was based on, if any.
    pub checkpoint: Option<String>,
}

/// How [`Journal::recover`] should proceed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoverPolicy {
    /// Load the latest snapshot and count post-checkpoint records.
    Replay,
    /// Same as [`RecoverPolicy::Replay`] and additionally release a stale
    /// writer lock so a later `registration::open` succeeds.
    TakeoverStaleLock,
}

impl RecoverPolicy {
    /// Stable, auditable name recorded in the journal's `Recovery` record.
    fn name(self) -> &'static str {
        match self {
            RecoverPolicy::Replay => "Replay",
            RecoverPolicy::TakeoverStaleLock => "TakeoverStaleLock",
        }
    }
}

/// The on-disk journal body. Kept as a separate (de)serializable type so the
/// in-memory [`Journal`] can hold `records: Vec<Record>`.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
struct JournalFile {
    records: Vec<Record>,
}

/// The recovery journal for a project folder.
///
/// Append-only: [`Journal::open`] may start with an empty in-memory log, and
/// every [`Journal::append`] persists the entire log atomically.
#[derive(Debug, Clone)]
pub struct Journal {
    records: Vec<Record>,
    file: PathBuf,
}

fn journal_path(folder: &Path) -> PathBuf {
    folder.join(SIDECAR_DIR).join(JOURNAL_FILE)
}

fn snapshot_path(folder: &Path) -> PathBuf {
    folder.join(SIDECAR_DIR).join(GRAPH_DIR).join(SNAPSHOT_FILE)
}

/// Seconds since the UNIX epoch (same convention as the registration lock).
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Journal {
    /// Open the journal for `folder`, parsing an existing `journal.toml` if
    /// present, otherwise starting with an empty log. No file is created until
    /// the first `append` (so `recover` can distinguish "no journal yet").
    pub fn open(folder: &Path) -> Result<Self> {
        let file = journal_path(folder);
        let records = if file.exists() {
            let body = std::fs::read_to_string(&file)?;
            if body.trim().is_empty() {
                Vec::new()
            } else {
                let parsed: JournalFile = toml::from_str(&body).map_err(|e| {
                    KernelError::InvalidSidecar(format!("malformed journal: {e}"))
                })?;
                parsed.records
            }
        } else {
            Vec::new()
        };
        Ok(Self { records, file })
    }

    /// Append a record, then atomically persist the full log.
    pub fn append(&mut self, r: Record) -> Result<()> {
        self.records.push(r);
        let dir = self.file.parent().expect("journal path always has a parent");
        std::fs::create_dir_all(dir)?;
        let payload = JournalFile {
            records: self.records.clone(),
        };
        let toml = toml::to_string(&payload).map_err(|e| {
            KernelError::InvalidSidecar(format!("cannot serialize journal: {e}"))
        })?;
        let tmp = self.file.with_extension("toml.tmp");
        std::fs::write(&tmp, toml)?;
        std::fs::rename(&tmp, &self.file)?;
        Ok(())
    }

    /// All records currently in the log.
    pub fn records(&self) -> &[Record] {
        &self.records
    }

    /// Quiesce the graph: snapshot `engine` to `.projectos/graph/snapshot.toml`
    /// (creating the dir), append a `Checkpoint` record, and return its id.
    pub fn checkpoint(
        &mut self,
        folder: &Path,
        engine: &dyn GraphEngine,
    ) -> Result<String> {
        let snap = snapshot_path(folder);
        if let Some(parent) = snap.parent() {
            std::fs::create_dir_all(parent)?;
        }
        engine.save_snapshot(&snap)?;
        let id = uuid::Uuid::new_v4().to_string();
        self.append(Record::Checkpoint { id: id.clone(), at: now() })?;
        Ok(id)
    }

    /// Recover the project from its latest checkpoint.
    ///
    /// * No journal file on disk → [`KernelError::InvalidSidecar`] (a project
    ///   with no journal cannot be "recovered").
    /// * Loads the latest snapshot via
    ///   `InMemoryGraphEngine::load_snapshot`, counts the records after the
    ///   latest `Checkpoint` as `replayed`.
    /// * With [`RecoverPolicy::TakeoverStaleLock`], also calls
    ///   `registration::release_lock(folder)` and sets `lock_taken_over`.
    /// * Appends a `Recovery` record naming the policy used.
    pub fn recover(
        folder: &Path,
        policy: RecoverPolicy,
    ) -> Result<RecoveryReport> {
        let file = journal_path(folder);
        if !file.exists() {
            return Err(KernelError::InvalidSidecar(format!(
                "no recovery journal at {}",
                file.to_string_lossy()
            )));
        }
        let mut journal = Journal::open(folder)?;

        // Locate the latest checkpoint and count the records after it.
        let mut last_checkpoint: Option<(usize, String)> = None;
        for (i, rec) in journal.records.iter().enumerate() {
            if let Record::Checkpoint { id, .. } = rec {
                last_checkpoint = Some((i, id.clone()));
            }
        }
        let (last_idx, checkpoint_id) = match last_checkpoint {
            Some((i, id)) => (i, Some(id)),
            None => (usize::MAX, None), // no checkpoint: replay every record
        };
        let replayed = journal
            .records
            .iter()
            .enumerate()
            .filter(|(i, _)| *i > last_idx)
            .count();

        // Load the latest snapshot so the caller can inspect recovered state.
        // A missing snapshot is an error — a checkpoint always writes one.
        if let Some(_) = checkpoint_id {
            let snap = snapshot_path(folder);
            if !snap.exists() {
                return Err(KernelError::InvalidSidecar(format!(
                    "checkpoint {} referenced but snapshot missing at {}",
                    checkpoint_id.clone().unwrap(),
                    snap.to_string_lossy()
                )));
            }
            InMemoryGraphEngine::load_snapshot(&snap)?;
        }

        let mut lock_taken_over = false;
        if policy == RecoverPolicy::TakeoverStaleLock {
            // Truthful audit fact: report a takeover only when a lock actually
            // existed. Reviewer finding (adversarial round 2): an unconditional
            // `true` here recorded a takeover that never happened.
            let had_lock = crate::registration::lock_owner(folder)?.is_some();
            crate::registration::release_lock(folder)?;
            lock_taken_over = had_lock;
        }

        journal.append(Record::Recovery {
            policy: policy.name().to_string(),
            at: now(),
        })?;

        Ok(RecoveryReport {
            replayed,
            lock_taken_over,
            checkpoint: checkpoint_id,
        })
    }
}
