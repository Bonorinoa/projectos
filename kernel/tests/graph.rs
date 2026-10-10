//! Acceptance tests for the graph engine — child G per `INTERFACES-M1b.md` (§G).

use projectos_kernel::error::KernelError;
use projectos_kernel::graph::{Edge, GraphEngine, Object, Relation};
use projectos_kernel::graph_memory::InMemoryGraphEngine;

fn obj(id: &str, kind: &str, label: &str) -> Object {
    Object {
        id: id.to_string(),
        kind: kind.to_string(),
        label: label.to_string(),
        fields: Default::default(),
    }
}

fn edge(from: &str, to: &str, rel: Relation) -> Edge {
    Edge {
        from: from.to_string(),
        to: to.to_string(),
        rel,
    }
}

/// The frozen trait signatures take `&ObjectId` (`&String`), so wrap literals.
fn oid(s: &str) -> String {
    s.to_string()
}

#[test]
fn upsert_get_delete() {
    let mut g = InMemoryGraphEngine::new();
    assert!(g.object(&oid("a")).unwrap().is_none());

    g.upsert_object(obj("a", "note", "A")).unwrap();
    let o = g.object(&oid("a")).unwrap().expect("a should exist");
    assert_eq!(o.label, "A");
    assert_eq!(g.objects().unwrap().len(), 1);

    // Upsert replaces the existing object rather than duplicating it.
    g.upsert_object(obj("a", "note", "A2")).unwrap();
    assert_eq!(g.object(&oid("a")).unwrap().unwrap().label, "A2");
    assert_eq!(g.objects().unwrap().len(), 1);

    assert!(g.delete_object(&oid("a")).unwrap());
    assert!(g.object(&oid("a")).unwrap().is_none());
    // Deleting an absent object returns false (idempotent).
    assert!(!g.delete_object(&oid("a")).unwrap());
}

#[test]
fn delete_removes_incident_edges() {
    let mut g = InMemoryGraphEngine::new();
    g.upsert_object(obj("a", "", "A")).unwrap();
    g.upsert_object(obj("b", "", "B")).unwrap();
    g.upsert_object(obj("c", "", "C")).unwrap();
    g.add_edge(edge("a", "b", Relation::Cites)).unwrap();
    g.add_edge(edge("b", "c", Relation::LinksTo)).unwrap();
    assert_eq!(g.edges().unwrap().len(), 2);

    // Deleting `b` removes both the outgoing (b->c) and incoming (a->b) edges.
    assert!(g.delete_object(&oid("b")).unwrap());
    assert!(g.edges().unwrap().is_empty());
}

#[test]
fn edge_to_missing_object_errors() {
    let mut g = InMemoryGraphEngine::new();
    g.upsert_object(obj("a", "", "A")).unwrap();

    // Outgoing and incoming endpoints are both validated.
    let err = g.add_edge(edge("a", "ghost", Relation::Cites));
    assert!(matches!(err, Err(KernelError::InvalidSidecar(_))));
    let err = g.add_edge(edge("ghost", "a", Relation::Cites));
    assert!(matches!(err, Err(KernelError::InvalidSidecar(_))));

    // The failed edge is never recorded.
    assert!(g.edges().unwrap().is_empty());
}

#[test]
fn lineage_is_transitive_and_cycle_safe() {
    let mut g = InMemoryGraphEngine::new();
    for id in ["a", "b", "c", "d"] {
        g.upsert_object(obj(id, "", id)).unwrap();
    }
    // a -> b -> c, with a cycle b <-> c, and c -> d.
    g.add_edge(edge("a", "b", Relation::Cites)).unwrap();
    g.add_edge(edge("b", "c", Relation::Cites)).unwrap();
    g.add_edge(edge("c", "b", Relation::Cites)).unwrap(); // cycle
    g.add_edge(edge("c", "d", Relation::Cites)).unwrap();

    // Transitive closure from a includes everything reachable (and a itself).
    let mut from_a = g.lineage(&oid("a")).unwrap();
    from_a.sort();
    assert_eq!(from_a, vec!["a", "b", "c", "d"]);

    // From inside the cycle: terminates and reaches b, c, d.
    let mut from_b = g.lineage(&oid("b")).unwrap();
    from_b.sort();
    assert_eq!(from_b, vec!["b", "c", "d"]);

    // A leaf node closes over just itself.
    assert_eq!(g.lineage(&oid("d")).unwrap(), vec!["d".to_string()]);

    // An absent id yields an empty closure.
    assert!(g.lineage(&oid("nope")).unwrap().is_empty());
}

#[test]
fn snapshot_round_trip_is_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let p1 = dir.path().join("snap1.toml");
    let p2 = dir.path().join("snap2.toml");

    let mut g = InMemoryGraphEngine::new();
    g.upsert_object(obj("z", "note", "Z")).unwrap();
    g.upsert_object(obj("a", "note", "A")).unwrap();
    g.upsert_object(obj("m", "note", "M")).unwrap();
    g.add_edge(edge("a", "z", Relation::Cites)).unwrap();
    g.add_edge(edge("m", "a", Relation::GovernedBy)).unwrap();

    g.save_snapshot(&p1).unwrap();
    g.save_snapshot(&p2).unwrap();
    assert_eq!(std::fs::read(&p1).unwrap(), std::fs::read(&p2).unwrap());

    // Round-trip through load_snapshot reproduces identical data...
    let loaded = InMemoryGraphEngine::load_snapshot(&p1).unwrap();
    assert_eq!(g.objects().unwrap(), loaded.objects().unwrap());
    assert_eq!(g.edges().unwrap(), loaded.edges().unwrap());

    // ...and a re-save of the loaded engine is byte-identical to the original.
    let p3 = dir.path().join("snap3.toml");
    loaded.save_snapshot(&p3).unwrap();
    assert_eq!(std::fs::read(&p1).unwrap(), std::fs::read(&p3).unwrap());

    // Atomic write leaves no .tmp behind.
    assert!(dir
        .path()
        .read_dir()
        .unwrap()
        .all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
}

#[test]
fn deterministic_ordering() {
    let mut g = InMemoryGraphEngine::new();
    // Insert in deliberately non-sorted order.
    for id in ["z", "a", "m", "b", "y"] {
        g.upsert_object(obj(id, "", "")).unwrap();
    }
    let ids: Vec<String> = g.objects().unwrap().into_iter().map(|o| o.id).collect();
    assert_eq!(ids, vec!["a", "b", "m", "y", "z"]);

    g.add_edge(edge("y", "a", Relation::Cites)).unwrap();
    g.add_edge(edge("b", "z", Relation::Cites)).unwrap();
    g.add_edge(edge("a", "m", Relation::Cites)).unwrap();
    let order: Vec<(String, String)> = g
        .edges()
        .unwrap()
        .into_iter()
        .map(|e| (e.from, e.to))
        .collect();
    assert_eq!(order[0], ("a".to_string(), "m".to_string()));
    assert_eq!(order[1], ("b".to_string(), "z".to_string()));
    assert_eq!(order[2], ("y".to_string(), "a".to_string()));
}
