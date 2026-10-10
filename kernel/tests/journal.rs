//! Requirement tests for the recovery journal (child J) per `INTERFACES-M1b.md` (§J).

use std::path::Path;
use std::collections::BTreeMap;

use projectos_kernel::graph::{Edge, GraphEngine, Object, ObjectId, Relation};
use projectos_kernel::graph_memory::InMemoryGraphEngine;
use projectos_kernel::journal::{Journal, RecoverPolicy, Record};
use projectos_kernel::registration;

fn tmpdir() -> tempfile::TempDir {
    tempfile::tempdir().expect("create tempdir")
}

fn register_project(folder: &Path) {
    registration::register(folder, "docs", vec!["docs".to_string()])
        .expect("register project");
}

fn obj(id: &str) -> Object {
    Object {
        id: id.to_string(),
        kind: "note".to_string(),
        label: id.to_string(),
        fields: BTreeMap::new(),
    }
}

#[test]
fn append_read_round_trip() {
    let dir = tmpdir();
    register_project(dir.path());

    let mut j = Journal::open(dir.path()).expect("open journal");
    assert_eq!(j.records(), &[]);

    j.append(Record::Opened { at: 1 }).expect("append Opened");
    j.append(Record::Mutated { object: "a".into(), at: 2 }).expect("append Mutated");
    j.append(Record::Checkpoint { id: "cp-1".into(), at: 3 }).expect("append Checkpoint");

    assert_eq!(
        j.records(),
        &[
            Record::Opened { at: 1 },
            Record::Mutated { object: "a".into(), at: 2 },
            Record::Checkpoint { id: "cp-1".into(), at: 3 },
        ]
    );

    // Re-open: records persist across handles.
    let j2 = Journal::open(dir.path()).expect("reopen journal");
    assert_eq!(j2.records().len(), 3);
}

#[test]
fn journal_persists_across_open() {
    let dir = tmpdir();
    register_project(dir.path());

    {
        let mut j = Journal::open(dir.path()).unwrap();
        j.append(Record::Opened { at: 7 }).unwrap();
        j.append(Record::Mutated { object: "x".into(), at: 8 }).unwrap();
    } // handle dropped

    let j = Journal::open(dir.path()).unwrap();
    assert_eq!(j.records().len(), 2);
    assert_eq!(j.records()[1], Record::Mutated { object: "x".into(), at: 8 });
}

#[test]
fn checkpoint_writes_snapshot_and_returns_id() {
    let dir = tmpdir();
    register_project(dir.path());

    let mut engine = InMemoryGraphEngine::new();
    engine.upsert_object(obj("abc")).unwrap();

    let mut j = Journal::open(dir.path()).unwrap();
    let id = j.checkpoint(dir.path(), &engine).expect("checkpoint");

    assert!(!id.is_empty());

    // A snapshot file must exist with the engine's object.
    let snap = dir.path().join(".projectos/graph/snapshot.toml");
    assert!(snap.exists(), "snapshot must exist");
    let loaded = InMemoryGraphEngine::load_snapshot(&snap).expect("load snapshot");
    assert!(loaded.object(&"abc".to_string()).unwrap().is_some());

    // The Checkpoint record carries the returned id.
    let last = j.records().last().unwrap();
    match last {
        Record::Checkpoint { id: rid, .. } => assert_eq!(rid, &id),
        other => panic!("expected Checkpoint record, got {:?}", other),
    }
}

#[test]
fn replay_after_checkpoint_reports_right_count() {
    let dir = tmpdir();
    register_project(dir.path());

    let mut j = Journal::open(dir.path()).unwrap();
    // Records before the checkpoint (not replayed after it).
    j.append(Record::Mutated { object: "pre1".into(), at: 1 }).unwrap();
    j.append(Record::Mutated { object: "pre2".into(), at: 2 }).unwrap();

    let mut engine = InMemoryGraphEngine::new();
    engine.upsert_object(obj("saved")).unwrap();
    let cp_id = j.checkpoint(dir.path(), &engine).unwrap();

    // Records after the checkpoint (these are the ones "replayed").
    j.append(Record::Mutated { object: "post1".into(), at: 3 }).unwrap();
    j.append(Record::Mutated { object: "post2".into(), at: 4 }).unwrap();
    j.append(Record::Opened { at: 5 }).unwrap();

    let report = Journal::recover(dir.path(), RecoverPolicy::Replay).expect("recover Replay");

    assert_eq!(report.replayed, 3, "three records after the checkpoint");
    assert!(!report.lock_taken_over);
    assert_eq!(report.checkpoint.as_deref(), Some(cp_id.as_str()));

    // A Recovery record is appended.
    let recs = Journal::open(dir.path()).unwrap().records().to_vec();
    let last = recs.last().unwrap();
    match last {
        Record::Recovery { policy, .. } => assert_eq!(policy, "Replay"),
        other => panic!("expected Recovery record, got {:?}", other),
    }
}

#[test]
fn takeover_stale_lock_releases_lock_file() {
    let dir = tmpdir();
    register_project(dir.path());

    // Create a stale lock: open holds the lock, then the handle is dropped
    // OUT of scope (drop is the normal release). To simulate a STALE lock
    // that survives a crash, write the lock file directly.
    let lock_path = dir.path().join(".projectos/lock");
    std::fs::write(&lock_path, "pid = 999999\nstarted_at = 1\n").expect("write stale lock");
    // A journal must exist for recover to run.
    Journal::open(dir.path())
        .expect("open journal")
        .append(Record::Opened { at: 1 })
        .expect("append record");
    assert!(lock_path.exists(), "stale lock present");
    // A fresh `open` must currently fail: the lock is held.
    assert!(registration::open(dir.path()).is_err(), "open should fail while lock held");

    let report =
        Journal::recover(dir.path(), RecoverPolicy::TakeoverStaleLock).expect("takeover");
    assert!(report.lock_taken_over, "lock_taken_over must be true");

    // The stale lock file was released; a later open now succeeds.
    assert!(!lock_path.exists(), "lock file must be removed after takeover");
    let handle = registration::open(dir.path()).expect("open succeeds after takeover");
    drop(handle);

    // A Recovery record naming the policy was appended.
    let recs = Journal::open(dir.path()).unwrap().records().to_vec();
    let last = recs.last().unwrap();
    match last {
        Record::Recovery { policy, .. } => assert_eq!(policy, "TakeoverStaleLock"),
        other => panic!("expected Recovery record, got {:?}", other),
    }
}

#[test]
fn takeover_also_uses_snapshot_for_replay() {
    let dir = tmpdir();
    register_project(dir.path());

    let mut j = Journal::open(dir.path()).unwrap();
    j.append(Record::Mutated { object: "pre".into(), at: 1 }).unwrap();
    let mut engine = InMemoryGraphEngine::new();
    engine.upsert_object(obj("kept")).unwrap();
    j.checkpoint(dir.path(), &engine).unwrap();
    j.append(Record::Mutated { object: "post".into(), at: 2 }).unwrap();

    let _stale = dir.path().join(".projectos/lock");
    std::fs::write(&_stale, "pid = 424242\nstarted_at = 1\n").unwrap();

    let report = Journal::recover(dir.path(), RecoverPolicy::TakeoverStaleLock).unwrap();
    assert_eq!(report.replayed, 1);
    assert!(report.lock_taken_over);
    // A later open succeeds (lock released by takeover).
    let handle = registration::open(dir.path()).unwrap();
    drop(handle);
}

#[test]
fn recover_without_journal_is_error() {
    let dir = tmpdir();
    register_project(dir.path());
    // No journal file has ever been written.
    let err = Journal::recover(dir.path(), RecoverPolicy::Replay).unwrap_err();
    assert!(format!("{:?}", err).contains("no recovery journal"));
}

#[test]
fn graph_helpers_are_available() {
    // Sanity that the graph scaffolding contract used by the journal is intact.
    let mut engine = InMemoryGraphEngine::new();
    let a = obj("a");
    let b = obj("b");
    let id_a: ObjectId = "a".into();
    let id_b: ObjectId = "b".into();
    engine.upsert_object(a).unwrap();
    engine.upsert_object(b).unwrap();
    engine
        .add_edge(Edge { from: id_a.clone(), to: id_b.clone(), rel: Relation::Cites })
        .unwrap();
    let lineage = engine.lineage(&id_a).unwrap();
    assert!(lineage.contains(&id_a));
    assert!(lineage.contains(&id_b));
}
