//! Adversarial review suite — written by the reviewing lead, not by the children.
//!
//! Purpose: try to *break* the frozen-contract implementations on the cases a
//! generated-but-plausible implementation typically gets wrong. These tests
//! assert SECURITY PROPERTIES (nothing outside the root is ever written or
//! read), not just that the implementation returns the shape it documents.
//!
//! Findings are reported in the commit message / review note; any failure here
//! is evidence about the implementing model, not just about the code.

use projectos_kernel::broker::{
    Broker, BrokerOutcome, BrokerRequest, Capability, CapabilitySet,
};
use projectos_kernel::error::KernelError;
use projectos_kernel::registration;
use std::fs;
use std::path::Path;

fn grants(rel: &str) -> CapabilitySet {
    let mut g = CapabilitySet::new();
    g.push(Capability::ReadFile(rel.to_string()));
    g.push(Capability::WriteFile(rel.to_string()));
    g
}

/// A write through a DANGLING symlink inside the root must not create a file
/// outside the root. (Child B documented "dangling-symlink escape" as a known
/// untested limitation; this is that case.)
#[test]
fn dangling_symlink_write_does_not_escape_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let outside_dir = tempfile::tempdir().unwrap();
    let outside_target = outside_dir.path().join("pwned.txt");
    assert!(!outside_target.exists());

    // A symlink inside the root pointing at a not-yet-existing outside file.
    let link = root.join("link.txt");
    std::os::unix::fs::symlink(&outside_target, &link).unwrap();

    let mut broker = Broker::new(root.clone(), grants("link.txt"));
    let out = broker
        .request(BrokerRequest::WriteFile {
            path: "link.txt".to_string(),
            bytes: b"PWNED".to_vec(),
        })
        .unwrap();

    assert!(
        !outside_target.exists(),
        "ESCAPE: a brokered write created a file OUTSIDE the root through a dangling symlink (outcome: {:?})",
        out
    );
}

/// A write through a symlinked DIRECTORY inside the root must not land outside.
#[test]
fn symlinked_directory_write_does_not_escape_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let outside_dir = tempfile::tempdir().unwrap();

    std::os::unix::fs::symlink(outside_dir.path(), root.join("outdir")).unwrap();

    let mut broker = Broker::new(root.clone(), grants("outdir/new.txt"));
    let out = broker
        .request(BrokerRequest::WriteFile {
            path: "outdir/new.txt".to_string(),
            bytes: b"PWNED".to_vec(),
        })
        .unwrap();

    assert!(
        !outside_dir.path().join("new.txt").exists(),
        "ESCAPE: brokered write landed outside the root through a symlinked directory (outcome: {:?})",
        out
    );
    assert!(
        matches!(out, BrokerOutcome::Denied(_)),
        "expected a denial for an escaping path, got {:?}",
        out
    );
}

/// Case-insensitive (APFS) alias of a registered project must be rejected.
#[test]
fn case_insensitive_alias_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("Proj");
    fs::create_dir(&project).unwrap();

    let registered = registration::register(&project, "writing", vec![]).unwrap();
    assert!(!registered.envelope.project_uuid.is_nil());

    // Same folder, different letter case (macOS resolves both to the same dir).
    let alias = dir.path().join("proj");
    let err = registration::register(&alias, "writing", vec![]).unwrap_err();
    match err {
        KernelError::AliasPath(named) => assert!(!named.is_empty()),
        other => panic!("expected AliasPath for a case-insensitive alias, got {:?}", other),
    }
}

/// Opening an already-registered project by its CANONICAL path must be the same
/// project, not an alias rejection (a realistic app flow canonicalizes first).
#[test]
fn open_by_canonical_path_after_register_is_same_project() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("proj");
    fs::create_dir(&project).unwrap();
    let registered = registration::register(&project, "writing", vec![]).unwrap();

    let canonical = fs::canonicalize(&project).unwrap();
    let opened = registration::open(&canonical);
    match opened {
        Ok(p) => assert_eq!(
            p.envelope.project_uuid, registered.envelope.project_uuid,
            "canonical-path open resolved a different project"
        ),
        Err(e) => panic!(
            "open() by canonical path failed after register() — false AliasPath/NotRegistered: {:?}",
            e
        ),
    }
}

/// The writer lock must be released when the handle is dropped.
#[test]
fn writer_lock_is_released_on_drop() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("proj");
    fs::create_dir(&project).unwrap();
    registration::register(&project, "writing", vec![]).unwrap();

    {
        let _held = registration::open(&project).unwrap();
        // Second open while held must fail.
        let err = registration::open(&project).unwrap_err();
        assert!(matches!(err, KernelError::ProjectLocked(_)), "got {:?}", err);
    }
    // After the handle drops, the lock must be gone.
    registration::open(&project).expect("lock must be released when the handle drops");
}

/// Honest documentation of a limitation: a lock file left behind by a crashed
/// process is NOT yet recovered (no staleness detection). Recorded here so the
/// next slice (recovery journal, gate-d model) has to address it.
#[test]
fn stale_lock_file_is_not_recovered_yet() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("proj");
    fs::create_dir(&project).unwrap();
    registration::register(&project, "writing", vec![]).unwrap();

    // Simulate a crashed writer: a lock file that nobody owns any more.
    fs::write(project.join(".projectos").join("lock"), b"crashed-pid").unwrap();

    let err = registration::open(&project).unwrap_err();
    assert!(
        matches!(err, KernelError::ProjectLocked(_)),
        "expected ProjectLocked for a stale lock (documented limitation), got {:?}",
        err
    );
}

/// A write through a symlink CHAIN (link -> link2 -> dangling outside file) must
/// not create anything outside the root.
#[test]
fn symlink_chain_write_does_not_escape_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let outside_dir = tempfile::tempdir().unwrap();
    let outside_target = outside_dir.path().join("chained.txt");

    std::os::unix::fs::symlink(&outside_target, root.join("link2")).unwrap();
    std::os::unix::fs::symlink(root.join("link2"), root.join("link1")).unwrap();

    let mut broker = Broker::new(root.clone(), grants("link1"));
    let out = broker
        .request(BrokerRequest::WriteFile {
            path: "link1".to_string(),
            bytes: b"PWNED".to_vec(),
        })
        .unwrap();

    assert!(
        !outside_target.exists(),
        "ESCAPE: chained-symlink write created a file outside the root (outcome: {:?})",
        out
    );
}

/// A write through a symlink pointing at an EXISTING outside file must not
/// overwrite it.
#[test]
fn symlink_to_existing_outside_file_write_denied() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let outside_dir = tempfile::tempdir().unwrap();
    let outside_target = outside_dir.path().join("real.txt");
    fs::write(&outside_target, "ORIGINAL").unwrap();

    std::os::unix::fs::symlink(&outside_target, root.join("link.txt")).unwrap();

    let mut broker = Broker::new(root.clone(), grants("link.txt"));
    let out = broker
        .request(BrokerRequest::WriteFile {
            path: "link.txt".to_string(),
            bytes: b"PWNED".to_vec(),
        })
        .unwrap();

    assert_eq!(
        fs::read_to_string(&outside_target).unwrap(),
        "ORIGINAL",
        "ESCAPE: brokered write overwrote an existing file outside the root (outcome: {:?})",
        out
    );
    assert!(matches!(out, BrokerOutcome::Denied(_)), "expected Denied, got {:?}", out);
}

/// Sanity: the broker must never hand back an absolute path in a receipt, even
/// for a `..` escape attempt that combines with an absolute-looking suffix.
#[test]
fn receipt_rel_path_is_never_absolute_or_root_leaking() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let mut broker = Broker::new(root.clone(), CapabilitySet::new());
    for attempt in ["../x.txt", "/etc/passwd", "a/../../b"] {
        let _ = broker.request(BrokerRequest::ReadFile(attempt.to_string()));
    }
    let root_str = fs::canonicalize(&root).unwrap().to_string_lossy().to_string();
    for r in broker.receipts() {
        assert!(
            !Path::new(&r.rel_path).is_absolute(),
            "receipt rel_path is absolute: {}",
            r.rel_path
        );
        assert!(
            !r.rel_path.contains(&root_str) && !r.outcome.contains(&root_str),
            "receipt leaked the absolute root: {:?}",
            r
        );
    }
}
