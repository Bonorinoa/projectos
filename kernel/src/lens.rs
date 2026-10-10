//! Evidence / lineage lens rows (M2 child D) — SCAFFOLD STUB.
//!
//! Frozen contract: `kernel/INTERFACES-M2.md` §D. The owner of this file
//! replaces every `unimplemented!()` body with the real behaviour + tests in
//! `tests/lens.rs`. Do not change the public signatures.

use crate::error::Result;
use crate::graph::{GraphEngine, ObjectId};

/// One row of a lens (a table-equivalent record).
#[derive(Debug, Clone, PartialEq)]
pub struct LensRow {
    pub object: String,
    pub kind: String,
    pub relation: String,
}

/// A table-equivalent view around a center object.
#[derive(Debug, Clone, PartialEq)]
pub struct Lens {
    pub center: String,
    pub rows: Vec<LensRow>,
}

pub fn one_hop(engine: &dyn GraphEngine, id: &ObjectId) -> Result<Lens> {
    let _ = (engine, id);
    unimplemented!()
}

pub fn lineage_lens(engine: &dyn GraphEngine, id: &ObjectId) -> Result<Lens> {
    let _ = (engine, id);
    unimplemented!()
}

pub fn evidence_lens(engine: &dyn GraphEngine, id: &ObjectId) -> Result<Lens> {
    let _ = (engine, id);
    unimplemented!()
}
