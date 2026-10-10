//! In-memory `GraphEngine` adapter — implemented by child G per
//! `INTERFACES-M1b.md` (§G).
//!
//! A simple, auditable BTree-based engine: objects live in a sorted map and
//! edges in a sorted set. Everything iterates in sorted order (deterministic
//! order), which makes snapshots byte-stable. There is deliberately no unsafe
//! code and no speculative abstraction — the Helix-db engine can replace this
//! behind the same [`crate::graph::GraphEngine`] trait later.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::error::{KernelError, Result};
use crate::graph::{Edge, GraphEngine, Object, ObjectId};

/// Serialized snapshot shape. `objects` is a map (stable order) and `edges` a
/// sorted set (stable order), so two saves of identical graphs are byte-equal.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct SnapshotDto {
    objects: BTreeMap<ObjectId, Object>,
    edges: BTreeSet<Edge>,
}

/// An in-memory [`GraphEngine`] backed by sorted BTree collections.
#[derive(Debug, Default)]
pub struct InMemoryGraphEngine {
    objects: BTreeMap<ObjectId, Object>,
    edges: BTreeSet<Edge>,
}

impl InMemoryGraphEngine {
    /// A new, empty in-memory engine.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a graph previously written by [`GraphEngine::save_snapshot`].
    ///
    /// This is the entry point the recovery journal (`journal.rs`) uses to
    /// hydrate an engine from `.projectos/graph/snapshot.toml`, so its
    /// signature is part of the frozen contract and must not change.
    pub fn load_snapshot(file: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(file)?;
        let dto: SnapshotDto = toml::from_str(&content).map_err(|e| {
            KernelError::InvalidSidecar(format!("malformed snapshot {}: {e}", file.display()))
        })?;
        Ok(Self {
            objects: dto.objects,
            edges: dto.edges,
        })
    }
}

impl GraphEngine for InMemoryGraphEngine {
    fn upsert_object(&mut self, obj: Object) -> Result<()> {
        self.objects.insert(obj.id.clone(), obj);
        Ok(())
    }

    fn delete_object(&mut self, id: &ObjectId) -> Result<bool> {
        if self.objects.remove(id).is_none() {
            return Ok(false);
        }
        // Remove every incident edge: outgoing (from == id) and incoming.
        self.edges.retain(|e| e.from != *id && e.to != *id);
        Ok(true)
    }

    fn object(&self, id: &ObjectId) -> Result<Option<Object>> {
        Ok(self.objects.get(id).cloned())
    }

    fn objects(&self) -> Result<Vec<Object>> {
        Ok(self.objects.values().cloned().collect())
    }

    fn add_edge(&mut self, e: Edge) -> Result<()> {
        let missing = if !self.objects.contains_key(&e.from) {
            Some(&e.from)
        } else if !self.objects.contains_key(&e.to) {
            Some(&e.to)
        } else {
            None
        };
        if let Some(endpoint) = missing {
            return Err(KernelError::InvalidSidecar(format!(
                "add_edge: endpoint object does not exist: {endpoint}"
            )));
        }
        self.edges.insert(e);
        Ok(())
    }

    fn edges(&self) -> Result<Vec<Edge>> {
        Ok(self.edges.iter().cloned().collect())
    }

    fn lineage(&self, id: &ObjectId) -> Result<Vec<ObjectId>> {
        // Terms immediately if the source object is absent.
        if !self.objects.contains_key(id) {
            return Ok(Vec::new());
        }
        // DFS with a visited set makes this terminate on cycles. The source
        // node itself is part of the closure.
        let mut seen = BTreeSet::new();
        let mut stack = vec![id.clone()];
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            for edge in self.edges.iter().filter(|e| e.from == current) {
                stack.push(edge.to.clone());
            }
        }
        Ok(seen.into_iter().collect())
    }

    fn save_snapshot(&self, file: &Path) -> Result<()> {
        let dto = SnapshotDto {
            objects: self.objects.clone(),
            edges: self.edges.clone(),
        };
        let toml = toml::to_string(&dto).map_err(|e| {
            KernelError::InvalidSidecar(format!("cannot serialize snapshot: {e}"))
        })?;
        let parent = file.parent().filter(|p| !p.as_os_str().is_empty());
        if let Some(dir) = parent {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = sibling_tmp_path(file);
        std::fs::write(&tmp, toml)?;
        std::fs::rename(&tmp, file)?;
        Ok(())
    }
}

/// `file.tmp` next to `file`, used for the atomic tmp+rename snapshot write.
fn sibling_tmp_path(file: &Path) -> std::path::PathBuf {
    let name = file
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "snapshot.toml".to_string());
    match file.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(format!("{name}.tmp")),
        _ => std::path::PathBuf::from(format!("{name}.tmp")),
    }
}
