//! Evidence / lineage lens rows (M2, `m2.context`).
//!
//! Every lens is a **table-equivalent** view: a deterministically ordered list of
//! [`LensRow`]s around a center object, satisfying brief §8 ("every lens has an
//! equivalent list/table"). `one_hop` = direct neighbours; `lineage_lens` = the
//! transitive closure; `evidence_lens` = only support/conflict-style relations.
//! An unknown center id yields an empty lens (not an error).

use crate::error::Result;
use crate::graph::{GraphEngine, ObjectId, Relation};

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

fn kind_of(engine: &dyn GraphEngine, id: &ObjectId) -> Result<String> {
    Ok(engine.object(id)?.map(|o| o.kind).unwrap_or_default())
}

fn sort_rows(rows: &mut [LensRow]) {
    rows.sort_by(|a, b| {
        (a.object.as_str(), a.relation.as_str(), a.kind.as_str()).cmp(&(
            b.object.as_str(),
            b.relation.as_str(),
            b.kind.as_str(),
        ))
    });
}

/// Objects linked to `center` by any single edge, in either direction.
pub fn one_hop(engine: &dyn GraphEngine, id: &ObjectId) -> Result<Lens> {
    if engine.object(id)?.is_none() {
        return Ok(Lens {
            center: id.clone(),
            rows: Vec::new(),
        });
    }
    let mut rows = Vec::new();
    for e in engine.edges()? {
        if &e.from == id {
            rows.push(LensRow {
                object: e.to.clone(),
                kind: kind_of(engine, &e.to)?,
                relation: format!("{:?}", e.rel),
            });
        } else if &e.to == id {
            rows.push(LensRow {
                object: e.from.clone(),
                kind: kind_of(engine, &e.from)?,
                relation: format!("{:?}", e.rel),
            });
        }
    }
    sort_rows(&mut rows);
    Ok(Lens {
        center: id.clone(),
        rows,
    })
}

/// The transitive closure from `center` (cycle-safe via `engine.lineage`).
pub fn lineage_lens(engine: &dyn GraphEngine, id: &ObjectId) -> Result<Lens> {
    if engine.object(id)?.is_none() {
        return Ok(Lens {
            center: id.clone(),
            rows: Vec::new(),
        });
    }
    let mut ids = engine.lineage(id)?;
    ids.sort();
    ids.dedup();
    let mut rows = Vec::with_capacity(ids.len());
    for oid in ids {
        let kind = kind_of(engine, &oid)?;
        rows.push(LensRow {
            object: oid,
            kind,
            relation: "Lineage".to_string(),
        });
    }
    sort_rows(&mut rows);
    Ok(Lens {
        center: id.clone(),
        rows,
    })
}

/// Neighbours joined only by support/conflict-style relations.
pub fn evidence_lens(engine: &dyn GraphEngine, id: &ObjectId) -> Result<Lens> {
    if engine.object(id)?.is_none() {
        return Ok(Lens {
            center: id.clone(),
            rows: Vec::new(),
        });
    }
    let evidence_rel = |r: Relation| {
        matches!(r, Relation::Supports | Relation::Cites | Relation::GovernedBy)
    };
    let mut rows = Vec::new();
    for e in engine.edges()? {
        if !evidence_rel(e.rel) {
            continue;
        }
        if &e.from == id {
            rows.push(LensRow {
                object: e.to.clone(),
                kind: kind_of(engine, &e.to)?,
                relation: format!("{:?}", e.rel),
            });
        } else if &e.to == id {
            rows.push(LensRow {
                object: e.from.clone(),
                kind: kind_of(engine, &e.from)?,
                relation: format!("{:?}", e.rel),
            });
        }
    }
    sort_rows(&mut rows);
    Ok(Lens {
        center: id.clone(),
        rows,
    })
}
