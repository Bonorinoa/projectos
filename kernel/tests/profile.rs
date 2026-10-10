//! Acceptance tests for the M1 profile registry (child P).
//!
//! Per `INTERFACES-M1b.md` §P: save/open round-trip; export → restore into a
//! clean app-data dir reproduces entries and rebinds paths under a given
//! project_root; UUID authority (the sidecar UUID always wins); missing
//! folders land in `restore.missing`; deterministic ordering.
//!
//! Each test is isolated in its own `tempfile::tempdir`. Tests use only the
//! kernel's public API, `tempfile`, and `std`.

use std::path::{Path, PathBuf};

use projectos_kernel::profile::{ProfileEntry, ProfileLink, ProfileRegistry};
use projectos_kernel::sidecar::SidecarEnvelope;
use uuid::Uuid;

/// Write a sidecar envelope into `folder` so it becomes a real on-disk
/// project with the given UUID (the identity authority for restore).
fn make_sidecar(folder: &Path, uuid: Uuid, declared_type: &str) {
    let env = SidecarEnvelope {
        project_uuid: uuid,
        declared_type: declared_type.to_string(),
        content_roots: vec![],
        schema_version: 1,
        engine_version: "0.0.1".to_string(),
        export_version: "0.0.1".to_string(),
        checkpoint: None,
    };
    SidecarEnvelope::write(folder, &env).unwrap();
}

/// Build a profile entry with the given identity and a path whose basename
/// we can re-bind under a project_root on restore.
fn make_entry(uuid: Uuid, binding: &Path) -> ProfileEntry {
    ProfileEntry {
        project_uuid: uuid,
        path_binding: binding.to_path_buf(),
        last_seen_checkpoint: Some("cp-1".to_string()),
        links: vec![ProfileLink {
            label: "rel".to_string(),
            target_object: "obj-1".to_string(),
        }],
    }
}

// 1. save / open round-trip preserves all entry fields.
#[test]
fn save_open_round_trip_preserves_entries() {
    let dir = tempfile::tempdir().unwrap();
    let app_data = dir.path().join("appdata");

    let uuid = Uuid::new_v4();
    let mut reg = ProfileRegistry::open(&app_data).unwrap();
    reg.upsert(make_entry(uuid, &PathBuf::from("/machine/p1"))).unwrap();
    reg.save().unwrap();

    let reopened = ProfileRegistry::open(&app_data).unwrap();
    let e = reopened.get(&uuid).expect("entry present after reopen");
    assert_eq!(e.project_uuid, uuid);
    assert_eq!(e.path_binding, PathBuf::from("/machine/p1"));
    assert_eq!(e.last_seen_checkpoint.as_deref(), Some("cp-1"));
    assert_eq!(e.links.len(), 1);
    assert_eq!(e.links[0].label, "rel");
    assert_eq!(e.links[0].target_object, "obj-1");
    assert_eq!(reopened.entries().len(), 1);
}

// 2. export → restore into a clean app-data dir reproduces entries and
//    re-binds each path under the given project_root.
#[test]
fn export_restore_reproduces_entries_and_rebinds_paths() {
    let dir = tempfile::tempdir().unwrap();

    // Real on-disk project folders with sidecars.
    let projects = dir.path().join("projects");
    let p1 = projects.join("alpha");
    let p2 = projects.join("beta");
    std::fs::create_dir_all(&p1).unwrap();
    std::fs::create_dir_all(&p2).unwrap();
    let u1 = Uuid::new_v4();
    let u2 = Uuid::new_v4();
    make_sidecar(&p1, u1, "writing");
    make_sidecar(&p2, u2, "software");

    // Source registry bound to the original (machine-local) folders.
    let src_app = dir.path().join("src-app");
    let mut reg = ProfileRegistry::open(&src_app).unwrap();
    reg.upsert(make_entry(u1, &p1)).unwrap();
    reg.upsert(make_entry(u2, &p2)).unwrap();

    let export_file = dir.path().join("export.toml");
    reg.export(&export_file).unwrap();

    // Clean app-data dir + a new project_root holding the moved folders.
    let clean_app = dir.path().join("clean-app");
    let new_root = dir.path().join("new-root");
    std::fs::create_dir_all(&new_root).unwrap();
    std::fs::rename(&p1, &new_root.join("alpha")).unwrap();
    std::fs::rename(&p2, &new_root.join("beta")).unwrap();

    let mut restored = ProfileRegistry::open(&clean_app).unwrap();
    let report = restored.restore(&export_file, &new_root).unwrap();

    assert_eq!(report.entries, 2);
    assert_eq!(report.rebound, 0, "sidecar UUIDs matched, nothing corrected");
    assert!(report.missing.is_empty());

    // Both entries reproduced, paths re-bound under new_root.
    let e1 = restored.get(&u1).expect("u1 restored");
    assert_eq!(e1.path_binding, new_root.join("alpha"));
    assert_eq!(e1.last_seen_checkpoint.as_deref(), Some("cp-1"));
    let e2 = restored.get(&u2).expect("u2 restored");
    assert_eq!(e2.path_binding, new_root.join("beta"));
}

// 3. UUID authority: an entry whose folder holds a different sidecar UUID is
//    corrected to the on-disk sidecar UUID — the sidecar always wins.
#[test]
fn uuid_authority_sidecar_wins() {
    let dir = tempfile::tempdir().unwrap();

    // Export claims uuid_a for a folder whose basename is "delta", but the
    // on-disk folder's sidecar actually holds uuid_b.
    let uuid_a = Uuid::new_v4();
    let uuid_b = Uuid::new_v4();
    assert_ne!(uuid_a, uuid_b);

    let src_app = dir.path().join("src-app");
    let mut reg = ProfileRegistry::open(&src_app).unwrap();
    reg.upsert(make_entry(uuid_a, &dir.path().join("original").join("delta"))).unwrap();

    let export_file = dir.path().join("export.toml");
    reg.export(&export_file).unwrap();

    let clean_app = dir.path().join("clean-app");
    let new_root = dir.path().join("new-root");
    let delta = new_root.join("delta");
    std::fs::create_dir_all(&delta).unwrap();
    make_sidecar(&delta, uuid_b, "writing");

    let mut restored = ProfileRegistry::open(&clean_app).unwrap();
    let report = restored.restore(&export_file, &new_root).unwrap();

    assert_eq!(report.entries, 1);
    assert_eq!(report.rebound, 1, "one identity correction recorded");
    assert!(report.missing.is_empty());

    // The sidecar UUID won: entry exists under uuid_b, not uuid_a.
    let e = restored.get(&uuid_b).expect("sidecar uuid present");
    assert_eq!(e.project_uuid, uuid_b);
    assert_eq!(e.path_binding, delta);
    assert!(restored.get(&uuid_a).is_none(), "stale entry uuid removed");
}

// 4. Entries whose folder is not present land in restore.missing and are not
//    reproduced.
#[test]
fn missing_folders_land_in_restore_missing() {
    let dir = tempfile::tempdir().unwrap();
    let uuid = Uuid::new_v4();

    let src_app = dir.path().join("src-app");
    let mut reg = ProfileRegistry::open(&src_app).unwrap();
    // Binding whose basename "ghost" has no folder under new_root.
    reg.upsert(make_entry(uuid, &dir.path().join("original").join("ghost"))).unwrap();
    let export_file = dir.path().join("export.toml");
    reg.export(&export_file).unwrap();

    let clean_app = dir.path().join("clean-app");
    let new_root = dir.path().join("new-root");
    std::fs::create_dir_all(&new_root).unwrap();

    let mut restored = ProfileRegistry::open(&clean_app).unwrap();
    let report = restored.restore(&export_file, &new_root).unwrap();

    assert_eq!(report.entries, 0);
    assert_eq!(report.missing, vec![uuid], "missing folder reported by uuid");
    assert!(restored.entries().is_empty());
}

// 5. Deterministic ordering: entries() is sorted by project UUID string and
//    stable across separate opens of the same persisted file.
#[test]
fn deterministic_ordering_across_opens() {
    let dir = tempfile::tempdir().unwrap();
    let app_data = dir.path().join("appdata");

    let uuids = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];

    // Insert in a deliberately scrambled order.
    for i in [2usize, 0, 1] {
        let mut reg = ProfileRegistry::open(&app_data).unwrap();
        reg.upsert(make_entry(uuids[i], &dir.path().join("p"))).unwrap();
        reg.save().unwrap();
    }

    let order = |uuid: &Uuid| uuid.to_string();
    let mut expected: Vec<String> = uuids.iter().map(order).collect();
    expected.sort();

    let first = ProfileRegistry::open(&app_data).unwrap();
    let order1: Vec<String> =
        first.entries().iter().map(|e| e.project_uuid.to_string()).collect();
    assert_eq!(order1, expected, "entries are sorted by uuid string");

    // A second independent open produces the same deterministic order.
    let second = ProfileRegistry::open(&app_data).unwrap();
    let order2: Vec<String> =
        second.entries().iter().map(|e| e.project_uuid.to_string()).collect();
    assert_eq!(order1, order2, "ordering stable across opens");
}
