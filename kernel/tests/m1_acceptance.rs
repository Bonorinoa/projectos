//! M1 acceptance suite — authored by the reviewing lead (see ACCEPTANCE-M1.md).
//!
//! These tests prove the brief's §18 exit criteria (open → move → recover a
//! project and restore the profile graph; no provider required) against the
//! real kernel modules. They are deliberately written by the reviewer, not by
//! the implementing agents.

use projectos_kernel::broker::{Broker, BrokerRequest, CapabilitySet};
use projectos_kernel::error::KernelError;
use projectos_kernel::graph::{Edge, GraphEngine, Object, Relation};
use projectos_kernel::graph_memory::InMemoryGraphEngine;
use projectos_kernel::journal::{Journal, Record, RecoverPolicy};
use projectos_kernel::profile::{ProfileEntry, ProfileRegistry};
use projectos_kernel::receipts::{ReceiptWriter, RedactedReceipt};
use projectos_kernel::registration;
use projectos_kernel::sidecar::SidecarEnvelope;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

fn obj(id: &str, kind: &str) -> Object {
    Object {
        id: id.to_string(),
        kind: kind.to_string(),
        label: id.to_string(),
        fields: BTreeMap::new(),
    }
}

fn receipt(at: u64, op: &str, rel: &str, outcome: &str) -> RedactedReceipt {
    RedactedReceipt {
        id: String::new(), // recomputed by the writer
        at,
        op: op.to_string(),
        rel_path: rel.to_string(),
        outcome: outcome.to_string(),
        content_hash: Some("digest-abc".to_string()),
        route: None,
        cost: None,
    }
}

fn snapshot_file(folder: &Path) -> std::path::PathBuf {
    folder.join(".projectos").join("graph").join("snapshot.toml")
}

/// A1 — headline: open → mutate → checkpoint → mutate → move → recover, with
/// the graph, the identity and the receipts all surviving.
#[test]
fn a1_open_move_recover_restores_project_graph() {
    let dir = tempfile::tempdir().unwrap();
    let proj = dir.path().join("proj");
    fs::create_dir(&proj).unwrap();

    let registered = registration::register(&proj, "writing", vec![]).unwrap();
    let uuid = registered.envelope.project_uuid;
    assert!(!uuid.is_nil());
    drop(registered);

    // Writer session: the lock records this process as owner.
    let held = registration::open(&proj).unwrap();
    let owner = registration::lock_owner(&proj).unwrap().expect("lock owner");
    assert_eq!(owner.pid, std::process::id());

    // Graph state + a quiesced checkpoint, then two records after it.
    let mut engine = InMemoryGraphEngine::new();
    for (id, kind) in [
        ("d1", "Draft"),
        ("e1", "Evidence"),
        ("r1", "Receipt"),
        ("v1", "Revision"),
    ] {
        engine.upsert_object(obj(id, kind)).unwrap();
    }
    engine
        .add_edge(Edge { from: "d1".into(), to: "e1".into(), rel: Relation::Cites })
        .unwrap();
    engine
        .add_edge(Edge { from: "v1".into(), to: "d1".into(), rel: Relation::RevisionOf })
        .unwrap();
    engine
        .add_edge(Edge { from: "d1".into(), to: "r1".into(), rel: Relation::Produced })
        .unwrap();
    let objects_before = engine.objects().unwrap().len();

    let mut journal = Journal::open(&proj).unwrap();
    let checkpoint = journal.checkpoint(&proj, &engine).unwrap();
    assert!(!checkpoint.is_empty());
    // Capture the snapshot bytes: the graph state is exactly this file.
    let snapshot_before = fs::read(snapshot_file(&proj)).unwrap();
    journal.append(Record::Mutated { object: "d1".into(), at: 1 }).unwrap();
    journal.append(Record::Mutated { object: "e1".into(), at: 2 }).unwrap();

    // One authored receipt + two ingested broker denials (one absolute, one `../`).
    let mut writer = ReceiptWriter::new(&proj).unwrap();
    writer.append(receipt(10, "read", "note.txt", "ok")).unwrap();
    let mut broker = Broker::new(proj.clone(), CapabilitySet::new());
    let _ = broker.request(BrokerRequest::ReadFile("/etc/passwd".into())).unwrap();
    let _ = broker.request(BrokerRequest::ReadFile("../escape.txt".into())).unwrap();
    let ingested = ReceiptWriter::ingest_broker_receipts(&proj, broker.receipts(), 11).unwrap();
    assert_eq!(ingested, 2, "both denials became durable facts");
    drop(writer);

    drop(held); // lock released with the session

    let moved = dir.path().join("moved");
    let moved_handle = registration::move_project(&proj, &moved).unwrap();
    assert_eq!(moved_handle.envelope.project_uuid, uuid, "UUID survives the move");
    assert!(!proj.exists(), "the folder really moved");
    drop(moved_handle);

    // Recovery from the checkpoint: the two post-checkpoint records replay.
    let report = Journal::recover(&moved, RecoverPolicy::Replay).unwrap();
    assert_eq!(report.replayed, 2, "post-checkpoint records replayed");
    assert!(!report.lock_taken_over, "a plain replay takes over nothing");
    assert_eq!(report.checkpoint.as_deref(), Some(checkpoint.as_str()));

    // The snapshot restores the exact graph: byte-identical snapshot file, and
    // the object/edge sets are readable and complete.
    assert_eq!(
        fs::read(snapshot_file(&moved)).unwrap(),
        snapshot_before,
        "the checkpointed graph survived the move byte-for-byte"
    );
    let restored = InMemoryGraphEngine::load_snapshot(&snapshot_file(&moved)).unwrap();
    let mut ids: Vec<String> = restored.objects().unwrap().into_iter().map(|o| o.id).collect();
    ids.sort();
    assert_eq!(ids.len(), objects_before, "no object lost");
    let mut pairs: Vec<(String, String)> = restored
        .edges()
        .unwrap()
        .into_iter()
        .map(|e| (e.from, e.to))
        .collect();
    pairs.sort();
    assert_eq!(
        pairs,
        vec![
            ("d1".to_string(), "e1".to_string()),
            ("d1".to_string(), "r1".to_string()),
            ("v1".to_string(), "d1".to_string()),
        ]
    );
    assert_eq!(restored.edges().unwrap().len(), 3, "no edge lost");

    // Receipts survived the move, still redacted, none lost.
    let after = ReceiptWriter::new(&moved).unwrap();
    assert_eq!(after.receipts().len(), 3, "3 receipt facts survived");
    for r in after.receipts() {
        assert!(!Path::new(&r.rel_path).is_absolute(), "no absolute rel_path: {}", r.rel_path);
        assert!(!r.rel_path.contains(&dir.path().to_string_lossy().to_string()));
    }

    // The project opens again, with no lock leak.
    let reopened = registration::open(&moved).unwrap();
    assert_eq!(reopened.envelope.project_uuid, uuid);
}

/// A2 — the profile graph restores into a CLEAN app-data dir, and the on-disk
/// sidecar UUID always wins over a profile entry.
#[test]
fn a2_profile_restore_into_clean_app_data_dir_and_uuid_authority() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    fs::create_dir(&root).unwrap();
    let p1 = root.join("p1");
    let p2 = root.join("p2");
    fs::create_dir(&p1).unwrap();
    fs::create_dir(&p2).unwrap();
    let u1 = registration::register(&p1, "writing", vec![]).unwrap().envelope.project_uuid;
    let u2 = registration::register(&p2, "code", vec![]).unwrap().envelope.project_uuid;

    let app_a = dir.path().join("appdata_a");
    let mut registry = ProfileRegistry::open(&app_a).unwrap();
    for (uuid, path) in [(u1, &p1), (u2, &p2)] {
        registry
            .upsert(ProfileEntry {
                project_uuid: uuid,
                path_binding: path.clone(),
                last_seen_checkpoint: None,
                links: vec![],
            })
            .unwrap();
    }
    registry.save().unwrap();

    let export = dir.path().join("profile-export.toml");
    registry.export(&export).unwrap();

    // Restore into a CLEAN app-data dir.
    let app_b = dir.path().join("appdata_b");
    let mut fresh = ProfileRegistry::open(&app_b).unwrap();
    let report = fresh.restore(&export, &root).unwrap();
    assert_eq!(report.entries, 2);
    assert!(report.missing.is_empty(), "both folders are present: {:?}", report.missing);
    for entry in fresh.entries() {
        assert!(
            entry.path_binding.starts_with(&root),
            "path binding re-attached under the project root: {:?}",
            entry.path_binding
        );
    }

    // UUID authority: tamper p1's sidecar; a fresh restore must adopt the
    // SIDECAR uuid, never the stale profile entry.
    let tampered_uuid = uuid::Uuid::new_v4();
    let mut env = SidecarEnvelope::read(&p1).unwrap().unwrap();
    env.project_uuid = tampered_uuid;
    SidecarEnvelope::write(&p1, &env).unwrap();

    let app_c = dir.path().join("appdata_c");
    let mut third = ProfileRegistry::open(&app_c).unwrap();
    let report_c = third.restore(&export, &root).unwrap();
    assert_eq!(report_c.entries, 2);
    assert!(
        third.get(&tampered_uuid).is_some(),
        "the entry was corrected to the on-disk sidecar UUID"
    );
    assert!(
        third.get(&u1).is_none(),
        "the stale profile UUID must not survive the restore"
    );
    assert!(report_c.rebound >= 1, "the correction is reported, not silent");
}

/// A3 — alias rejection at the registration boundary, and spelling-agnostic
/// `open()` for real work.
#[test]
fn a3_alias_rejected_but_open_is_spelling_agnostic() {
    let dir = tempfile::tempdir().unwrap();
    let proj = dir.path().join("proj");
    fs::create_dir(&proj).unwrap();
    let registered = registration::register(&proj, "writing", vec![]).unwrap();
    let uuid = registered.envelope.project_uuid;
    drop(registered);

    // Symlink alias.
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&proj, &link).unwrap();
    match registration::register(&link, "writing", vec![]) {
        Err(KernelError::AliasPath(named)) => {
            assert!(!named.is_empty(), "the error names the registered path");
        }
        other => panic!("expected AliasPath for a symlink alias, got {:?}", other),
    }

    // Case-insensitive alias (macOS volumes are case-insensitive by default).
    let case_alias = dir.path().join("PROJ");
    match registration::register(&case_alias, "writing", vec![]) {
        Err(KernelError::AliasPath(_)) => {}
        Ok(_) => eprintln!("note: this volume is case-sensitive; case-alias case skipped"),
        other => panic!("expected AliasPath for a case alias, got {:?}", other),
    }

    // open() by the canonical spelling is the same project.
    let canonical = fs::canonicalize(&proj).unwrap();
    let opened = registration::open(&canonical).unwrap();
    assert_eq!(opened.envelope.project_uuid, uuid);
}

/// A4 — second writer blocked; lock released on drop; stale lock needs an
/// explicit, recorded takeover.
#[test]
fn a4_second_writer_block_and_explicit_stale_lock_takeover() {
    let dir = tempfile::tempdir().unwrap();
    let proj = dir.path().join("proj");
    fs::create_dir(&proj).unwrap();
    registration::register(&proj, "writing", vec![]).unwrap();

    let held = registration::open(&proj).unwrap();
    assert!(matches!(
        registration::open(&proj).unwrap_err(),
        KernelError::ProjectLocked(_)
    ));
    drop(held);
    drop(registration::open(&proj).expect("lock released on drop"));

    // Simulate a crashed writer.
    fs::write(proj.join(".projectos").join("lock"), b"pid = 999999\nstarted_at = 1\n").unwrap();
    assert!(matches!(
        registration::open(&proj).unwrap_err(),
        KernelError::ProjectLocked(_)
    ));

    // Recovery is explicit and recorded.
    let mut journal = Journal::open(&proj).unwrap();
    journal.append(Record::Opened { at: 1 }).unwrap();
    let report = Journal::recover(&proj, RecoverPolicy::TakeoverStaleLock).unwrap();
    assert!(report.lock_taken_over, "the stale lock was taken over");
    let records = Journal::open(&proj).unwrap().records().to_vec();
    assert!(
        records.iter().any(|r| matches!(r, Record::Recovery { policy, .. } if policy == "TakeoverStaleLock")),
        "the takeover is recorded in the journal"
    );
    registration::open(&proj).expect("open succeeds after the recorded takeover");
}

/// A5 — OUTCOME_UNKNOWN is recorded once and never silently retried.
#[test]
fn a5_outcome_unknown_recorded_once_no_silent_retry() {
    let dir = tempfile::tempdir().unwrap();
    let proj = dir.path().join("proj");
    fs::create_dir(&proj).unwrap();
    registration::register(&proj, "writing", vec![]).unwrap();

    let mut writer = ReceiptWriter::new(&proj).unwrap();
    writer.append(receipt(5, "external-effect", "outbox/item.txt", "outcome-unknown")).unwrap();
    drop(writer);
    let mut journal = Journal::open(&proj).unwrap();
    journal.append(Record::Mutated { object: "outbox/item.txt".into(), at: 5 }).unwrap();
    journal.checkpoint(&proj, &InMemoryGraphEngine::new()).unwrap();
    drop(journal);

    // Recover, then re-open: the unknown fact is still there exactly once and
    // nothing re-executed it.
    Journal::recover(&proj, RecoverPolicy::Replay).unwrap();
    let after = ReceiptWriter::new(&proj).unwrap();
    let unknowns: Vec<_> = after
        .receipts()
        .iter()
        .filter(|r| r.outcome == "outcome-unknown")
        .collect();
    assert_eq!(unknowns.len(), 1, "no duplicate, no silent retry");
    assert_eq!(after.receipts().len(), 1, "no additional effect was recorded");
    // Reconciliation POLICY is out of scope this pass (documented in the plan).
}
