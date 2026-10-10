//! Proposal → diff → approval → revision loop (M2 core, `m2.diff`).
//!
//! A [`Proposal`] is a typed, reviewable suggestion (a replacement or an
//! annotation). Nothing is inserted silently: a proposal is applied only through
//! [`apply`], which **re-resolves the anchor** (a stale selection is refused),
//! keeps the **before-image** content-addressed under `.projectos/objects/`,
//! replaces the file atomically (tmp + rename), and records a journal `Mutated`
//! entry, a redacted receipt, and a durable [`Revision`]. Rejection mutates
//! nothing and records a `rejected` receipt.

use crate::error::{KernelError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Whether a proposal rewrites the artifact bytes or only annotates them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ProposalKind {
    Replacement,
    Annotation,
}

/// A typed, reviewable proposal produced by an action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub rel_path: String,
    pub kind: ProposalKind,
    pub original: String,
    pub proposed: String,
    /// Document hash the proposal was computed against.
    pub before_hash: String,
    /// Hash of the proposed content.
    pub after_hash: String,
}

/// A minimal line-level diff of a proposal.
#[derive(Debug, Clone, PartialEq)]
pub struct Diff {
    pub removed: Vec<String>,
    pub added: Vec<String>,
}

/// The user's decision on a proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Accept,
    Reject,
}

/// A durable record of an accepted revision (before/after content hashes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Revision {
    pub id: String,
    pub proposal_id: String,
    pub rel_path: String,
    pub before_hash: String,
    pub after_hash: String,
    pub at: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RevisionsFile {
    revisions: Vec<Revision>,
}

/// The durable revision timeline at `.projectos/revisions.toml`.
pub struct RevisionLog {
    path: PathBuf,
    revisions: Vec<Revision>,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn lines(s: &str) -> Vec<String> {
    if s.is_empty() {
        Vec::new()
    } else {
        s.lines().map(|l| l.to_string()).collect()
    }
}

impl Proposal {
    /// Build a proposal, deriving `after_hash` and a deterministic `id`.
    pub fn new(rel_path: &str, kind: ProposalKind, original: &str, proposed: &str, before_hash: &str) -> Self {
        let after_hash = crate::provider::stable_hash(proposed.as_bytes());
        let id = crate::provider::stable_hash(
            format!("{rel_path}\u{1f}{original}\u{1f}{proposed}").as_bytes(),
        );
        Proposal {
            id,
            rel_path: rel_path.to_string(),
            kind,
            original: original.to_string(),
            proposed: proposed.to_string(),
            before_hash: before_hash.to_string(),
            after_hash,
        }
    }

    pub fn diff(&self) -> Diff {
        Diff {
            removed: lines(&self.original),
            added: lines(&self.proposed),
        }
    }
}

impl RevisionLog {
    pub fn new(folder: &Path) -> Result<Self> {
        let dir = folder.join(".projectos");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("revisions.toml");
        let revisions = if path.exists() {
            let text = std::fs::read_to_string(&path)?;
            if text.trim().is_empty() {
                Vec::new()
            } else {
                let f: RevisionsFile = toml::from_str(&text).map_err(|e| {
                    KernelError::InvalidSidecar(format!("revisions.toml is corrupt: {e}"))
                })?;
                f.revisions
            }
        } else {
            Vec::new()
        };
        Ok(RevisionLog { path, revisions })
    }

    pub fn revisions(&self) -> &[Revision] {
        &self.revisions
    }

    fn append(&mut self, r: Revision) -> Result<()> {
        self.revisions.push(r);
        let f = RevisionsFile {
            revisions: self.revisions.clone(),
        };
        let text = toml::to_string(&f)
            .map_err(|e| KernelError::InvalidSidecar(format!("serialize revisions: {e}")))?;
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

/// Apply a decision to a proposal.
///
/// `Accept` re-resolves the anchor (**stale → error, never applied**), stores the
/// before-image content-addressed, replaces the anchored range atomically,
/// records journal + receipt + revision, and returns the `Revision`. `Reject`
/// mutates nothing, records a `rejected` receipt, and returns `None`.
pub fn apply(
    folder: &Path,
    anchor: &crate::selection::DocAnchor,
    p: &Proposal,
    d: Decision,
) -> Result<Option<Revision>> {
    match crate::selection::resolve(folder, anchor)? {
        crate::selection::AnchorState::Fresh => {}
        crate::selection::AnchorState::Stale(reason) => {
            return Err(KernelError::InvalidSidecar(format!("stale selection: {reason}")));
        }
    }
    if p.rel_path != anchor.rel_path {
        return Err(KernelError::InvalidSidecar(
            "proposal path does not match the anchor".to_string(),
        ));
    }

    if matches!(d, Decision::Reject) {
        let mut w = crate::receipts::ReceiptWriter::new(folder)?;
        w.append(crate::receipts::RedactedReceipt {
            id: String::new(),
            at: now(),
            op: "apply".to_string(),
            rel_path: p.rel_path.clone(),
            outcome: "rejected".to_string(),
            content_hash: Some(p.after_hash.clone()),
            route: None,
            cost: None,
        })?;
        return Ok(None);
    }

    let path = folder.join(&p.rel_path);
    let bytes = std::fs::read(&path)?;
    if anchor.byte_start > anchor.byte_end || anchor.byte_end > bytes.len() {
        return Err(KernelError::InvalidSidecar("anchor range out of bounds".to_string()));
    }

    // Before-image: full previous bytes, content-addressed (no-op if present).
    let objdir = folder.join(".projectos").join("objects");
    std::fs::create_dir_all(&objdir)?;
    let before_file = objdir.join(&p.before_hash);
    if !before_file.exists() {
        std::fs::write(&before_file, &bytes)?;
    }

    // Splice the anchored byte range, then replace atomically.
    let mut new_bytes = Vec::with_capacity(bytes.len() + p.proposed.len());
    new_bytes.extend_from_slice(&bytes[..anchor.byte_start]);
    new_bytes.extend_from_slice(p.proposed.as_bytes());
    new_bytes.extend_from_slice(&bytes[anchor.byte_end..]);
    let tmp = path.with_extension("projectos-tmp");
    std::fs::write(&tmp, &new_bytes)?;
    std::fs::rename(&tmp, &path)?;

    // Journal + redacted receipt + revision timeline.
    let mut journal = crate::journal::Journal::open(folder)?;
    journal.append(crate::journal::Record::Mutated {
        object: p.rel_path.clone(),
        at: now(),
    })?;
    let mut w = crate::receipts::ReceiptWriter::new(folder)?;
    w.append(crate::receipts::RedactedReceipt {
        id: String::new(),
        at: now(),
        op: "apply".to_string(),
        rel_path: p.rel_path.clone(),
        outcome: "accepted".to_string(),
        content_hash: Some(p.after_hash.clone()),
        route: None,
        cost: None,
    })?;
    let at = now();
    let rev = Revision {
        id: crate::provider::stable_hash(format!("{}\u{1f}{at}", p.id).as_bytes()),
        proposal_id: p.id.clone(),
        rel_path: p.rel_path.clone(),
        before_hash: p.before_hash.clone(),
        after_hash: p.after_hash.clone(),
        at,
    };
    RevisionLog::new(folder)?.append(rev.clone())?;
    Ok(Some(rev))
}
