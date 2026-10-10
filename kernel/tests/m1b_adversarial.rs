//! M1b adversarial round — reviewer-authored property tests for the slice-2
//! modules. Each test tries to falsify an invariant the module claims.

use projectos_kernel::graph::{Edge, GraphEngine, Object, Relation};
use projectos_kernel::graph_memory::InMemoryGraphEngine;
use projectos_kernel::journal::{Journal, Record, RecoverPolicy};
use projectos_kernel::receipts::{ReceiptWriter, RedactedReceipt};
use projectos_kernel::registration;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

fn obj(id: &str) -> Object {
    Object { id: id.into(), kind: "Note".into(), label: id.into(), fields: BTreeMap::new() }
}

/// ADV-1 — `lock_taken_over` must be TRUE only when a lock was actually taken
/// over. Claiming a takeover where no lock existed is a false audit fact.
#[test]
fn adv_recovery_does_not_claim_takeover_without_a_lock() {
    let dir = tempfile::tempdir().unwrap();
    let proj = dir.path().join("proj");
    fs::create_dir(&proj).unwrap();
    registration::register(&proj, "writing", vec![]).unwrap();

    let mut journal = Journal::open(&proj).unwrap();
    journal.append(Record::Opened { at: 1 }).unwrap();
    drop(journal);

    assert!(
        registration::lock_owner(&proj).unwrap().is_none(),
        "precondition: no lock file exists"
    );
    let report = Journal::recover(&proj, RecoverPolicy::TakeoverStaleLock).unwrap();
    assert!(
        !report.lock_taken_over,
        "FALSE AUDIT FACT: recover() reported a takeover although no lock was held"
    );
}

/// ADV-2 — a checkpoint whose snapshot file was deleted must fail loudly, never
/// silently report a successful recovery.
#[test]
fn adv_missing_snapshot_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let proj = dir.path().join("proj");
    fs::create_dir(&proj).unwrap();
    registration::register(&proj, "writing", vec![]).unwrap();

    let mut journal = Journal::open(&proj).unwrap();
    journal.checkpoint(&proj, &InMemoryGraphEngine::new()).unwrap();
    drop(journal);
    fs::remove_file(proj.join(".projectos").join("graph").join("snapshot.toml")).unwrap();

    assert!(
        Journal::recover(&proj, RecoverPolicy::Replay).is_err(),
        "a missing snapshot must not be reported as a successful recovery"
    );
}

/// ADV-3 — documented behaviour: receipt ids are content-addressed by
/// (at, op, rel_path), so two identical denials in the same instant share an id
/// while both facts are still stored. Asserted so the property is explicit.
#[test]
fn adv_identical_receipts_share_an_id_but_both_are_stored() {
    let dir = tempfile::tempdir().unwrap();
    let proj = dir.path().join("proj");
    fs::create_dir(&proj).unwrap();

    let mut broker = projectos_kernel::broker::Broker::new(
        proj.clone(),
        projectos_kernel::broker::CapabilitySet::new(),
    );
    let _ = broker
        .request(projectos_kernel::broker::BrokerRequest::ReadFile("/etc/passwd".into()))
        .unwrap();
    let _ = broker
        .request(projectos_kernel::broker::BrokerRequest::ReadFile("/etc/passwd".into()))
        .unwrap();
    assert_eq!(broker.receipts().len(), 2);

    let n = ReceiptWriter::ingest_broker_receipts(&proj, broker.receipts(), 99).unwrap();
    assert_eq!(n, 2);
    let stored = ReceiptWriter::new(&proj).unwrap();
    assert_eq!(stored.receipts().len(), 2, "both facts are stored");
    assert_eq!(
        stored.receipts()[0].id, stored.receipts()[1].id,
        "documented: identical facts share a content-addressed id"
    );
}

/// ADV-4 — lineage terminates on cycles and snapshots are byte-stable.
#[test]
fn adv_graph_lineage_cycle_safe_and_snapshot_byte_stable() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = InMemoryGraphEngine::new();
    for id in ["a", "b", "c"] {
        engine.upsert_object(obj(id)).unwrap();
    }
    engine.add_edge(Edge { from: "a".into(), to: "b".into(), rel: Relation::Cites }).unwrap();
    engine.add_edge(Edge { from: "b".into(), to: "c".into(), rel: Relation::Cites }).unwrap();
    engine.add_edge(Edge { from: "c".into(), to: "a".into(), rel: Relation::Cites }).unwrap();

    let lineage = engine.lineage(&"a".to_string()).unwrap();
    assert!(lineage.contains(&"a".to_string()));
    assert!(lineage.contains(&"b".to_string()));
    assert!(lineage.contains(&"c".to_string()));
    assert!(lineage.len() <= 3, "cycle must not grow the result set");

    let one = dir.path().join("one.toml");
    let two = dir.path().join("two.toml");
    engine.save_snapshot(&one).unwrap();
    let reloaded = InMemoryGraphEngine::load_snapshot(&one).unwrap();
    reloaded.save_snapshot(&two).unwrap();
    assert_eq!(
        fs::read(&one).unwrap(),
        fs::read(&two).unwrap(),
        "snapshot round-trip must be byte-identical (deterministic ordering)"
    );
}

/// ADV-5 — an absolute or traversal-shaped path handed to the writer is stored
/// in redacted form; the persisted file contains no absolute path.
#[test]
fn adv_receipt_redaction_survives_absolute_and_traversal_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let proj = dir.path().join("proj");
    fs::create_dir(&proj).unwrap();

    let mut writer = ReceiptWriter::new(&proj).unwrap();
    for rel in ["/etc/passwd", "C:\\Windows\\system32", "../../secrets.txt", "ok/inside.txt"] {
        writer
            .append(RedactedReceipt {
                id: String::new(),
                at: 7,
                op: "read".into(),
                rel_path: rel.into(),
                outcome: "denied".into(),
                content_hash: None,
                route: None,
                cost: None,
            })
            .unwrap();
    }
    drop(writer);

    let body = fs::read_to_string(proj.join(".projectos").join("receipts.toml")).unwrap();
    assert!(!body.contains("C:\\Windows"), "drive-prefixed path leaked: {body}");
    for line in body.lines() {
        if let Some(v) = line.strip_prefix("rel_path = \"") {
            let stored = v.trim_end_matches('"');
            assert!(
                !Path::new(stored).is_absolute(),
                "persisted rel_path is absolute: {stored}"
            );
        }
    }
    assert!(!proj.join(".projectos").join("receipts.tmp").exists(), "no .tmp left behind");
    // Traversal-shaped names are preserved verbatim by contract (broker rule
    // strips only a leading separator); assert the documented behaviour.
    assert!(body.contains("../../secrets.txt"));
}
