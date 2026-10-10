//! Named milestones + portable export (M2, `m2.milestone`).
//!
//! A [`Milestone`] is a **named, restorable checkpoint**: `create` quiesces the
//! graph engine through `journal::Journal::checkpoint` (which snapshots the graph
//! to `.projectos/graph/snapshot.toml` and appends a `Checkpoint` record) and
//! records the name alongside the checkpoint id. Milestones persist at
//! `.projectos/milestones.toml` (atomic tmp + rename).
//!
//! [`export`] writes a **portable** copy — the manifest, graph snapshot,
//! redacted receipts, revisions, and milestones — and never credentials,
//! caches, or `.tmp` files (none exist yet; the rule is enforced by copying an
//! explicit allowlist of relative names only).

use crate::error::{KernelError, Result};
use crate::graph::GraphEngine;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A named, restorable milestone (a quiesced checkpoint).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Milestone {
    pub name: String,
    pub checkpoint: String,
    pub at: u64,
}

/// The on-disk TOML shape.
#[derive(Debug, Default, Serialize, Deserialize)]
struct MilestonesFile {
    milestones: Vec<Milestone>,
}

/// Durable milestone store backed by `.projectos/milestones.toml`.
pub struct MilestoneStore {
    pub(crate) path: PathBuf,
    pub(crate) milestones: Vec<Milestone>,
}

/// What a portable export produced.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportReport {
    pub files: usize,
    pub graph_snapshot: bool,
    pub receipts: usize,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl MilestoneStore {
    pub fn new(folder: &Path) -> Result<Self> {
        let dir = folder.join(".projectos");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("milestones.toml");
        let milestones = if path.exists() {
            let text = std::fs::read_to_string(&path)?;
            if text.trim().is_empty() {
                Vec::new()
            } else {
                let f: MilestonesFile = toml::from_str(&text).map_err(|e| {
                    KernelError::InvalidSidecar(format!("milestones.toml is corrupt: {e}"))
                })?;
                f.milestones
            }
        } else {
            Vec::new()
        };
        Ok(MilestoneStore { path, milestones })
    }

    pub fn milestones(&self) -> &[Milestone] {
        &self.milestones
    }

    /// Quiesce the engine and record a named milestone. Empty name → error.
    pub fn create(
        &mut self,
        folder: &Path,
        name: &str,
        engine: &dyn GraphEngine,
    ) -> Result<Milestone> {
        if name.trim().is_empty() {
            return Err(KernelError::InvalidSidecar(
                "milestone name must not be empty".to_string(),
            ));
        }
        let mut journal = crate::journal::Journal::open(folder)?;
        let checkpoint = journal.checkpoint(folder, engine)?;
        let m = Milestone {
            name: name.to_string(),
            checkpoint,
            at: now(),
        };
        self.milestones.push(m.clone());
        self.persist()?;
        Ok(m)
    }

    fn persist(&self) -> Result<()> {
        let f = MilestonesFile {
            milestones: self.milestones.clone(),
        };
        let text = toml::to_string(&f)
            .map_err(|e| KernelError::InvalidSidecar(format!("serialize milestones: {e}")))?;
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

/// Parse just the receipt facts to count them (`receipts.toml` shape).
#[derive(Deserialize)]
struct ReceiptCount {
    #[serde(default)]
    receipts: Vec<toml::Value>,
}

/// Export a portable copy (manifest, graph snapshot, receipts, revisions,
/// milestones) of `folder` into the directory `to`.
pub fn export(folder: &Path, to: &Path) -> Result<ExportReport> {
    std::fs::create_dir_all(to)?;
    let sidecar = folder.join(".projectos");
    let mut files = 0usize;
    let mut graph_snapshot = false;
    let mut receipts = 0usize;

    let manifest = sidecar.join("project.toml");
    if manifest.exists() {
        std::fs::copy(&manifest, to.join("project.toml"))?;
        files += 1;
    }

    let snap = sidecar.join("graph").join("snapshot.toml");
    if snap.exists() {
        std::fs::create_dir_all(to.join("graph"))?;
        std::fs::copy(&snap, to.join("graph").join("snapshot.toml"))?;
        files += 1;
        graph_snapshot = true;
    }

    let receipts_path = sidecar.join("receipts.toml");
    if receipts_path.exists() {
        std::fs::copy(&receipts_path, to.join("receipts.toml"))?;
        files += 1;
        let text = std::fs::read_to_string(&receipts_path)?;
        if !text.trim().is_empty() {
            let parsed: ReceiptCount = toml::from_str(&text).map_err(|e| {
                KernelError::InvalidSidecar(format!("receipts.toml is corrupt: {e}"))
            })?;
            receipts = parsed.receipts.len();
        }
    }

    for name in ["revisions.toml", "milestones.toml"] {
        let p = sidecar.join(name);
        if p.exists() {
            std::fs::copy(&p, to.join(name))?;
            files += 1;
        }
    }

    Ok(ExportReport {
        files,
        graph_snapshot,
        receipts,
    })
}
