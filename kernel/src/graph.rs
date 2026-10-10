//! `GraphEngine` trait plus the graph object/edge types — implemented by child G
//! per `INTERFACES-M1b.md` (§G).
//!
//! This module owns only the *data model* and the *trait*. The in-memory
//! implementation lives in [`crate::graph_memory`]; a Helix-db-backed engine is
//! the planned M2 follow-on and will implement the same trait.

use std::collections::BTreeMap;
use std::path::Path;

/// Opaque object identifier string (e.g. a uuid).
pub type ObjectId = String;

/// Kinds of directed relationship between object nodes.
///
/// The variants and their relative ordering are part of the frozen contract
/// and are preserved when persisted, so they derive `Ord`/`serde`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum Relation {
    Supports,
    Cites,
    GovernedBy,
    Executed,
    Produced,
    RevisionOf,
    AppliedTo,
    LinksTo,
}

/// A typed artifact node in the project graph.
///
/// `fields` is an ordered map so serialization is byte-stable.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Object {
    pub id: ObjectId,
    pub kind: String,
    pub label: String,
    pub fields: BTreeMap<String, String>,
}

/// A directed relationship between two objects.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct Edge {
    pub from: ObjectId,
    pub to: ObjectId,
    pub rel: Relation,
}

/// The graph engine contract. Implementations must provide deterministic
/// iteration (sorted) so snapshots are byte-stable, and must make `lineage`
/// terminate on cycles.
pub trait GraphEngine {
    /// Insert `obj`, replacing an existing object with the same `id` if any.
    fn upsert_object(&mut self, obj: Object) -> crate::error::Result<()>;
    /// Remove the object with `id`; returns `false` if it was not present.
    /// Removing an object also removes every incident edge.
    fn delete_object(&mut self, id: &ObjectId) -> crate::error::Result<bool>;
    /// Fetch a single object, or `None` if absent.
    fn object(&self, id: &ObjectId) -> crate::error::Result<Option<Object>>;
    /// All objects in deterministic (sorted) order.
    fn objects(&self) -> crate::error::Result<Vec<Object>>;
    /// Add a directed edge. Errors if either endpoint object is missing.
    fn add_edge(&mut self, e: Edge) -> crate::error::Result<()>;
    /// All edges in deterministic (sorted) order.
    fn edges(&self) -> crate::error::Result<Vec<Edge>>;
    /// Transitive closure of `id` over directed edges (the node plus every
    /// node reachable from it). Cycle-safe: always terminates.
    fn lineage(&self, id: &ObjectId) -> crate::error::Result<Vec<ObjectId>>;
    /// Persist the graph as an atomic (tmp + rename) TOML snapshot.
    fn save_snapshot(&self, file: &Path) -> crate::error::Result<()>;
}
