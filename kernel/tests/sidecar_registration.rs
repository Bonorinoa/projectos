//! Acceptance tests for the M1 registration + sidecar slice (child A).
//!
//! Each test is isolated in its own `tempfile::tempdir`. The crate exposes no
//! [dependencies] directly to integration tests here; these tests use only the
//! kernel's public API, `tempfile`, and `std`.

use std::path::{Path, PathBuf};

use projectos_kernel::error::KernelError;
use projectos_kernel::registration::{move_project, open, register};
use projectos_kernel::sidecar::{MANIFEST_FILE, SIDECAR_DIR};

/// Convenience: path to a project's manifest inside its `.projectos` sidecar.
fn manifest_path(folder: &Path) -> PathBuf {
    folder.join(SIDECAR_DIR).join(MANIFEST_FILE)
}

/// Helper: create an empty project folder ready for registration.
fn make_project(dir: &Path, name: &str) -> PathBuf {
    let folder = dir.join(name);
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

// 1. register -> sidecar exists, UUID stable across reopen.
#[test]
fn register_creates_sidecar_and_uuid_is_stable_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let folder = make_project(dir.path(), "proj");

    let first = register(&folder, "writing", vec!["src".to_string()]).unwrap();

    // A sidecar manifest now exists on disk.
    assert!(manifest_path(&folder).exists());
    let uuid = first.envelope.project_uuid;

    // Re-registering the same folder is idempotent and keeps the UUID.
    let second = register(&folder, "writing", vec!["src".to_string()]).unwrap();
    assert_eq!(second.envelope.project_uuid, uuid);

    // Re-opening also preserves the UUID.
    let third = open(&folder).unwrap();
    assert_eq!(third.envelope.project_uuid, uuid);
    // `third` holds the writer lock; dropping it releases `.projectos/lock`.
    drop(third);
}

// 2. re-registering the same folder is idempotent (same UUID, no error).
#[test]
fn reregistering_same_folder_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let folder = make_project(dir.path(), "proj");

    let first = register(&folder, "writing", vec!["src".to_string()]).unwrap();
    let second = register(&folder, "writing", vec!["src".to_string()]).unwrap();

    assert_eq!(first.envelope.project_uuid, second.envelope.project_uuid);
    assert_eq!(first.envelope, second.envelope);
}

// 3. symlink alias of an already-registered folder -> AliasPath naming the
//    existing path.
#[test]
fn symlink_alias_of_registered_folder_is_alias_path() {
    let dir = tempfile::tempdir().unwrap();
    let real = make_project(dir.path(), "real");
    register(&real, "writing", vec!["src".to_string()]).unwrap();

    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let result = register(&link, "writing", vec!["src".to_string()]);
    match result {
        Err(KernelError::AliasPath(p)) => {
            // Must name the *original* registered path, not the symlink.
            assert_eq!(PathBuf::from(p), real);
        }
        other => panic!("expected AliasPath naming the existing path, got {other:?}"),
    }
}

// 4. open() on a folder with no sidecar -> NotRegistered.
#[test]
fn open_on_folder_without_sidecar_is_not_registered() {
    let dir = tempfile::tempdir().unwrap();
    let folder = make_project(dir.path(), "bare");

    let result = open(&folder);
    match result {
        Err(KernelError::NotRegistered(p)) => {
            assert_eq!(PathBuf::from(p), folder);
        }
        other => panic!("expected NotRegistered, got {other:?}"),
    }
}

// 5. move_project preserves the UUID and the old path no longer opens.
#[test]
fn move_project_preserves_uuid_and_old_path_no_longer_opens() {
    let dir = tempfile::tempdir().unwrap();
    let from = make_project(dir.path(), "from");

    let first = register(&from, "writing", vec!["src".to_string()]).unwrap();
    let uuid = first.envelope.project_uuid;

    let to = dir.path().join("to");
    let moved = move_project(&from, &to).unwrap();
    assert_eq!(moved.envelope.project_uuid, uuid);

    // The sidecar traveled with the folder.
    assert!(manifest_path(&moved.folder).exists());

    // Old path is gone: no sidecar, no open.
    let old_open = open(&from);
    assert!(
        matches!(old_open, Err(KernelError::NotRegistered(_))),
        "expected NotRegistered on the old path, got {old_open:?}"
    );

    // New path opens and keeps the UUID.
    let reopened = open(&to).unwrap();
    assert_eq!(reopened.envelope.project_uuid, uuid);
    drop(reopened);
}

// 6. a second open() while the writer lock is held -> ProjectLocked.
#[test]
fn second_open_while_locked_is_project_locked() {
    let dir = tempfile::tempdir().unwrap();
    let folder = make_project(dir.path(), "proj");
    register(&folder, "writing", vec!["src".to_string()]).unwrap();

    // First open acquires `.projectos/lock`.
    let first = open(&folder).unwrap();
    assert!(lock_path(&folder).exists());

    // Second open must be rejected; the first handle is still alive.
    let second = open(&folder);
    assert!(
        matches!(second, Err(KernelError::ProjectLocked(_))),
        "expected ProjectLocked, got {second:?}"
    );

    // Releasing the first handle clears the lock.
    drop(first);
    assert!(!lock_path(&folder).exists());
}

// 7. a content root containing `..` -> InvalidSidecar.
#[test]
fn content_root_with_parent_escape_is_invalid_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let folder = make_project(dir.path(), "proj");

    let result = register(&folder, "writing", vec!["../escape".to_string()]);
    assert!(
        matches!(result, Err(KernelError::InvalidSidecar(_))),
        "expected InvalidSidecar, got {result:?}"
    );
}

// 8. a sidecar with a future schema_version -> UnsupportedSchema.
#[test]
fn future_schema_version_is_unsupported_schema() {
    let dir = tempfile::tempdir().unwrap();
    let folder = make_project(dir.path(), "proj");

    // Hand-write a manifest with a schema version ahead of this kernel.
    let sidecar = folder.join(SIDECAR_DIR);
    std::fs::create_dir_all(&sidecar).unwrap();
    let bad = r#"
project_uuid = "123e4567-e89b-12d3-a456-426614174000"
declared_type = "writing"
content_roots = ["src"]
schema_version = 99
engine_version = "0.0.1"
export_version = "0.0.1"
checkpoint = ""
"#;
    std::fs::write(sidecar.join(MANIFEST_FILE), bad).unwrap();

    let result = projectos_kernel::sidecar::SidecarEnvelope::read(&folder);
    assert!(
        matches!(
            result,
            Err(KernelError::UnsupportedSchema {
                found: 99,
                expected: 1
            })
        ),
        "expected UnsupportedSchema {{ found: 99, expected: 1 }}, got {result:?}"
    );
}

/// Path to a project's `.projectos/lock` (mirrors the kernel's own layout).
fn lock_path(folder: &Path) -> PathBuf {
    folder.join(SIDECAR_DIR).join("lock")
}
