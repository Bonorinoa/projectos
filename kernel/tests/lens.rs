//! Tests for the evidence / lineage lens rows (M2 §D).

use projectos_kernel::graph::{Edge, GraphEngine, Object, Relation};
use projectos_kernel::graph_memory::InMemoryGraphEngine;
use projectos_kernel::lens::{evidence_lens, lineage_lens, one_hop};
use std::collections::BTreeMap;

fn obj(id: &str, kind: &str) -> Object {
    Object {
        id: id.to_string(),
        kind: kind.to_string(),
        label: id.to_string(),
        fields: BTreeMap::new(),
    }
}

fn engine() -> InMemoryGraphEngine {
    let mut e = InMemoryGraphEngine::new();
    for (id, kind) in [("a", "Draft"), ("b", "Evidence"), ("c", "Claim"), ("d", "Asset")] {
        e.upsert_object(obj(id, kind)).unwrap();
    }
    e
}

#[test]
fn one_hop_finds_both_directions() {
    let mut e = engine();
    // a -> b (Cites), c -> a (Supports)
    e.add_edge(Edge { from: "a".into(), to: "b".into(), rel: Relation::Cites }).unwrap();
    e.add_edge(Edge { from: "c".into(), to: "a".into(), rel: Relation::Supports }).unwrap();

    let lens = one_hop(&e, &"a".to_string()).unwrap();
    let mut objects: Vec<&str> = lens.rows.iter().map(|r| r.object.as_str()).collect();
    objects.sort();
    assert_eq!(objects, vec!["b", "c"], "one hop sees outgoing and incoming neighbours");
    // relation labels are the edge relation names.
    assert!(lens.rows.iter().any(|r| r.object == "b" && r.relation == "Cites"));
    assert!(lens.rows.iter().any(|r| r.object == "c" && r.relation == "Supports"));
}

#[test]
fn lineage_is_transitive_and_cycle_safe() {
    let mut e = engine();
    // 3-cycle a -> b -> c -> a
    e.add_edge(Edge { from: "a".into(), to: "b".into(), rel: Relation::Cites }).unwrap();
    e.add_edge(Edge { from: "b".into(), to: "c".into(), rel: Relation::Cites }).unwrap();
    e.add_edge(Edge { from: "c".into(), to: "a".into(), rel: Relation::Cites }).unwrap();

    let lens = lineage_lens(&e, &"a".to_string()).unwrap();
    let mut objects: Vec<&str> = lens.rows.iter().map(|r| r.object.as_str()).collect();
    objects.sort();
    assert_eq!(objects, vec!["a", "b", "c"], "transitive closure, cycle-safe");
}

#[test]
fn rows_are_deterministically_ordered() {
    let mut e = engine();
    e.add_edge(Edge { from: "a".into(), to: "d".into(), rel: Relation::LinksTo }).unwrap();
    e.add_edge(Edge { from: "a".into(), to: "b".into(), rel: Relation::Cites }).unwrap();
    e.add_edge(Edge { from: "c".into(), to: "a".into(), rel: Relation::Supports }).unwrap();

    let one = one_hop(&e, &"a".to_string()).unwrap();
    let two = one_hop(&e, &"a".to_string()).unwrap();
    assert_eq!(one, two, "two calls produce identical, ordered rows");
}

#[test]
fn unknown_id_yields_empty_rows_not_error() {
    let e = engine();
    let lens = one_hop(&e, &"missing".to_string()).unwrap();
    assert!(lens.rows.is_empty());
    let lens = lineage_lens(&e, &"missing".to_string()).unwrap();
    assert!(lens.rows.is_empty());
}

#[test]
fn evidence_lens_filters_to_support_relations() {
    let mut e = engine();
    e.add_edge(Edge { from: "a".into(), to: "b".into(), rel: Relation::Supports }).unwrap();
    e.add_edge(Edge { from: "a".into(), to: "c".into(), rel: Relation::Cites }).unwrap();
    e.add_edge(Edge { from: "a".into(), to: "d".into(), rel: Relation::Produced }).unwrap();

    let lens = evidence_lens(&e, &"a".to_string()).unwrap();
    let mut objects: Vec<&str> = lens.rows.iter().map(|r| r.object.as_str()).collect();
    objects.sort();
    assert_eq!(objects, vec!["b", "c"], "Produced is excluded; Supports/Cites kept");
}
