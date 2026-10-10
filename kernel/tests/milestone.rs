//! Tests for named milestones + portable export (M2 §C).

use projectos_kernel::graph_memory::InMemoryGraphEngine;
use projectos_kernel::milestone::{export, MilestoneStore};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

/// A registered-looking project folder with a sidecar manifest.
fn project_with_manifest(dir: &Path, name: &str) -> std::path::PathBuf {
    let folder = dir.join(name);
    let sidecar = folder.join(".projectos");
    fs::create_dir_all(&sidecar).unwrap();
    fs::write(
        sidecar.join("project.toml"),
        "project_uuid = \"123e4567-e89b-12d3-a456-426614174000\"\ndeclared_type = \"writing\"\ncontent_roots = []\nschema_version = 1\nengine_version = \"0.0.1\"\nexport_version = \"0.0.1\"\ncheckpoint = \"\"\n",
    )
    .unwrap();
    folder
}

#[test]
fn create_persists_and_reopens() {
    let dir = tempdir().unwrap();
    let proj = project_with_manifest(dir.path(), "proj");
    let engine = InMemoryGraphEngine::new();

    let mut store = MilestoneStore::new(&proj).unwrap();
    let m = store.create(&proj, "argument locked", &engine).unwrap();
    assert_eq!(m.name, "argument locked");
    assert!(!m.checkpoint.is_empty());
    // The checkpoint snapshot exists on disk.
    assert!(proj.join(".projectos").join("graph").join("snapshot.toml").exists());

    // Reopen a fresh store: the milestone persisted.
    let reopened = MilestoneStore::new(&proj).unwrap();
    assert_eq!(reopened.milestones().len(), 1);
    assert_eq!(reopened.milestones()[0].name, "argument locked");
    assert_eq!(reopened.milestones()[0].checkpoint, m.checkpoint);
}

#[test]
fn empty_name_is_rejected() {
    let dir = tempdir().unwrap();
    let proj = project_with_manifest(dir.path(), "proj");
    let engine = InMemoryGraphEngine::new();
    let mut store = MilestoneStore::new(&proj).unwrap();
    assert!(store.create(&proj, "   ", &engine).is_err());
}

#[test]
fn export_copies_milestones_receipts_and_manifest() {
    let dir = tempdir().unwrap();
    let proj = project_with_manifest(dir.path(), "proj");
    let engine = InMemoryGraphEngine::new();
    let mut store = MilestoneStore::new(&proj).unwrap();
    store.create(&proj, "publish candidate", &engine).unwrap();

    // One redacted receipt fact.
    fs::write(
        proj.join(".projectos").join("receipts.toml"),
        "[[receipts]]\nid = \"abc\"\nat = 1\nop = \"read\"\nrel_path = \"a.txt\"\noutcome = \"denied\"\n",
    )
    .unwrap();

    let to = dir.path().join("export");
    let report = export(&proj, &to).unwrap();

    assert!(to.join("project.toml").exists());
    assert!(to.join("receipts.toml").exists());
    assert!(to.join("milestones.toml").exists());
    assert!(to.join("graph").join("snapshot.toml").exists());
    assert!(report.graph_snapshot);
    assert_eq!(report.receipts, 1);
    assert!(report.files >= 4, "manifest + snapshot + receipts + milestones");

    // No `.tmp` and no absolute-path-named file under the export dir.
    for entry in fs::read_dir(&to).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().to_string();
        assert!(!name.ends_with(".tmp"), "left a .tmp: {name}");
        assert!(!Path::new(&name).is_absolute());
    }
}

#[test]
fn export_without_snapshot_still_succeeds() {
    let dir = tempdir().unwrap();
    let proj = project_with_manifest(dir.path(), "proj");
    let to = dir.path().join("export");
    let report = export(&proj, &to).unwrap();
    assert!(!report.graph_snapshot);
    assert!(to.join("project.toml").exists());
}
