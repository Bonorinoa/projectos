//! Named milestones + portable export (M2 child C) — SCAFFOLD STUB.
//!
//! Frozen contract: `kernel/INTERFACES-M2.md` §C. The owner of this file
//! replaces every `unimplemented!()` body with the real behaviour + tests in
//! `tests/milestone.rs`. Do not change the public signatures.

use crate::error::Result;
use crate::graph::GraphEngine;
use std::path::Path;

/// A named, restorable milestone (a quiesced checkpoint).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Milestone {
    pub name: String,
    pub checkpoint: String,
    pub at: u64,
}

/// The durable milestone store at `.projectos/milestones.toml`.
pub struct MilestoneStore {
    pub(crate) path: std::path::PathBuf,
    pub(crate) milestones: Vec<Milestone>,
}

/// What a portable export produced.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportReport {
    pub files: usize,
    pub graph_snapshot: bool,
    pub receipts: usize,
}

impl MilestoneStore {
    pub fn new(folder: &Path) -> Result<Self> {
        let _ = folder;
        unimplemented!()
    }

    pub fn milestones(&self) -> &[Milestone] {
        &self.milestones
    }

    /// Quiesce the engine and record a named milestone.
    pub fn create(&mut self, folder: &Path, name: &str, engine: &dyn GraphEngine) -> Result<Milestone> {
        let _ = (folder, name, engine);
        unimplemented!()
    }
}

/// Export a portable copy (manifest, graph snapshot, receipts, revisions,
/// milestones) into `to`; never secrets/caches/`.tmp`.
pub fn export(folder: &Path, to: &Path) -> Result<ExportReport> {
    let _ = (folder, to);
    unimplemented!()
}
