//! M2 reviewer adversarial round (lead-authored).
//!
//! Supplements `tests/adversarial.rs` (slice 1) and `tests/m1b_adversarial.rs`
//! (slice 2). The M2 session instruction asks for adversarial coverage of
//! symlink escapes, `..`/absolute path rejection, case-insensitive alias
//! duplicates, and second-writer lock races. The earlier suites cover READS on
//! those paths and single-threaded locking; this round adds the cases they
//! left open:
//!
//! * brokered WRITES carrying a `..` or an absolute path (must be denied and
//!   must create nothing outside the root);
//! * a READ through a symlinked *directory* (not just a file symlink);
//! * an alias reached through a symlinked *parent* component;
//! * `is_alias_of_registered` (public, previously untested);
//! * a genuine multi-threaded second-writer RACE (single-threaded block was
//!   the only lock coverage before);
//! * content roots that escape via a symlink, `.`, or an absolute path.
//!
//! Every failure here is evidence about the boundary, not just the shape.

use projectos_kernel::broker::{
    Broker, BrokerOutcome, BrokerRequest, Capability, CapabilitySet,
};
use projectos_kernel::error::KernelError;
use projectos_kernel::registration;
use projectos_kernel::sidecar::SIDECAR_DIR;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::thread;

fn grants(rel: &str) -> CapabilitySet {
    let mut g = CapabilitySet::new();
    g.push(Capability::ReadFile(rel.to_string()));
    g.push(Capability::WriteFile(rel.to_string()));
    g
}

fn lock_path(folder: &Path) -> PathBuf {
    folder.join(SIDECAR_DIR).join("lock")
}

/// ADV-M2-1 — a brokered WRITE whose path contains `..` must be denied by path
/// resolution and must not create a file outside the root. (The earlier suites
/// exercised `..` on READS only.)
#[test]
fn adv_write_traversal_denied_and_nothing_escapes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let outside = dir.path().join("evil.txt");
    assert!(!outside.exists());

    // Grant the escaping spelling so the GRANT check passes: path resolution is
    // then the only gate that can stop the write.
    let mut broker = Broker::new(root.clone(), grants("../evil.txt"));
    let out = broker
        .request(BrokerRequest::WriteFile {
            path: "../evil.txt".to_string(),
            bytes: b"PWNED".to_vec(),
        })
        .unwrap();

    assert!(
        matches!(out, BrokerOutcome::Denied(_)),
        "traversal write must be denied, got {:?}",
        out
    );
    assert!(!outside.exists(), "ESCAPE: traversal write created a file outside the root");
}

/// ADV-M2-2 — a brokered WRITE to an absolute path must be denied and must not
/// write anything.
#[test]
fn adv_write_absolute_path_denied_and_nothing_written() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let outside_dir = tempfile::tempdir().unwrap();
    let target = outside_dir.path().join("abs_pwned.txt");
    let abs = target.to_string_lossy().to_string();

    let mut broker = Broker::new(root.clone(), grants(&abs));
    let out = broker
        .request(BrokerRequest::WriteFile {
            path: abs.clone(),
            bytes: b"PWNED".to_vec(),
        })
        .unwrap();

    assert!(
        matches!(out, BrokerOutcome::Denied(_)),
        "absolute write must be denied, got {:?}",
        out
    );
    assert!(!target.exists(), "ESCAPE: absolute write created a file outside the root");
    // Redaction: no receipt may leak the absolute path verbatim.
    for r in broker.receipts() {
        assert!(!Path::new(&r.rel_path).is_absolute(), "absolute rel_path leaked: {}", r.rel_path);
    }
}

/// ADV-M2-3 — a READ through a symlinked *directory* that points outside the
/// root must be denied and must not return the outside bytes.
#[test]
fn adv_read_through_symlinked_directory_denied() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.txt"), "TOPSECRET-DIRLINK").unwrap();

    // A symlinked directory inside the root whose target lives outside.
    std::os::unix::fs::symlink(outside.path(), root.join("dirlink")).unwrap();

    let mut broker = Broker::new(root.clone(), grants("dirlink/secret.txt"));
    let out = broker
        .request(BrokerRequest::ReadFile("dirlink/secret.txt".to_string()))
        .unwrap();

    match out {
        BrokerOutcome::Denied(_) => {}
        BrokerOutcome::Read(b) => panic!(
            "ESCAPE: read through a symlinked directory leaked bytes: {:?}",
            String::from_utf8_lossy(&b)
        ),
        BrokerOutcome::Written => panic!("read returned Written"),
    }
}

/// ADV-M2-4 — a project reached through a symlinked PARENT directory is the
/// same canonical project, so registering it is an `AliasPath`.
#[test]
fn adv_alias_through_symlinked_parent_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base");
    let real_parent = base.join("real");
    let proj = real_parent.join("proj");
    fs::create_dir_all(&proj).unwrap();

    let registered = registration::register(&proj, "writing", vec![]).unwrap();
    let uuid = registered.envelope.project_uuid;

    // link -> base/real ; the project is then reachable as base/link/proj.
    let link = base.join("link");
    std::os::unix::fs::symlink(&real_parent, &link).unwrap();
    let reached = link.join("proj");

    match registration::register(&reached, "writing", vec![]) {
        Err(KernelError::AliasPath(_)) => {}
        other => panic!(
            "expected AliasPath for a project reached through a symlinked parent, got {:?}",
            other
        ),
    }

    // `is_alias_of_registered` (public, previously untested) resolves the same
    // way and reports the registered path.
    let alias = registration::is_alias_of_registered(&reached).unwrap();
    assert!(
        alias.is_some(),
        "is_alias_of_registered must recognise the symlinked-parent spelling"
    );
    // And the canonical open still resolves to the same UUID.
    let canonical = fs::canonicalize(&reached).unwrap();
    assert_eq!(registration::open(&canonical).unwrap().envelope.project_uuid, uuid);
}

/// ADV-M2-5 — a case-insensitive duplicate spelling is an alias for
/// `register`, and `is_alias_of_registered` reports it. Tolerant of
/// case-sensitive volumes (the case is then skipped, as in the M1 suite).
#[test]
fn adv_case_insensitive_alias_duplicate() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("CaseProj");
    fs::create_dir(&project).unwrap();
    registration::register(&project, "writing", vec![]).unwrap();

    let alias = dir.path().join("caseproj");
    let case_insensitive = fs::canonicalize(&alias)
        .map(|c| c == fs::canonicalize(&project).unwrap())
        .unwrap_or(false);

    if case_insensitive {
        match registration::register(&alias, "writing", vec![]) {
            Err(KernelError::AliasPath(_)) => {}
            other => panic!("expected AliasPath for a case-insensitive duplicate, got {:?}", other),
        }
        assert!(
            registration::is_alias_of_registered(&alias).unwrap().is_some(),
            "is_alias_of_registered must flag a case-insensitive duplicate"
        );
    } else {
        eprintln!("note: case-sensitive volume — case-insensitive alias case skipped");
    }
}

/// ADV-M2-6 — a genuine second-writer RACE: many threads open the same project
/// at once; exactly one must win and every other must get `ProjectLocked`.
/// (Prior coverage was single-threaded, which does not exercise the atomicity
/// of `create_new`.)
#[test]
fn adv_concurrent_second_writer_race_has_exactly_one_winner() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("proj");
    fs::create_dir(&folder).unwrap();
    registration::register(&folder, "writing", vec![]).unwrap();

    const N: usize = 16;
    let barrier = Arc::new(Barrier::new(N));
    let results: Vec<Result<registration::RegisteredProject, KernelError>> = thread::scope(|s| {
        let handles: Vec<_> = (0..N)
            .map(|_| {
                let b = barrier.clone();
                let f = folder.clone();
                s.spawn(move || {
                    b.wait(); // maximise the overlap of the open() calls
                    registration::open(&f)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    let oks = results.iter().filter(|r| r.is_ok()).count();
    let locked = results
        .iter()
        .filter(|r| matches!(r, Err(KernelError::ProjectLocked(_))))
        .count();
    assert_eq!(oks, 1, "exactly one concurrent open() may hold the writer lock");
    assert_eq!(locked, N - 1, "every other concurrent open() must be ProjectLocked");

    // Dropping the winner releases the lock; the folder opens cleanly again.
    drop(results);
    assert!(!lock_path(&folder).exists(), "lock must be released once the winner drops");
    registration::open(&folder).expect("open succeeds after the race winner drops");
}

/// ADV-M2-7 — a content root that is a symlink escaping the project folder must
/// be rejected at registration (`InvalidSidecar`).
#[test]
fn adv_content_root_symlink_escaping_folder_is_invalid() {
    let dir = tempfile::tempdir().unwrap();
    let proj = dir.path().join("proj");
    fs::create_dir(&proj).unwrap();
    let outside = tempfile::tempdir().unwrap();

    std::os::unix::fs::symlink(outside.path(), proj.join("data")).unwrap();

    let result = registration::register(&proj, "writing", vec!["data".to_string()]);
    assert!(
        matches!(result, Err(KernelError::InvalidSidecar(_))),
        "a symlinked content root escaping the folder must be InvalidSidecar, got {:?}",
        result
    );
}

/// ADV-M2-8 — `.` and absolute content roots are invalid.
#[test]
fn adv_content_root_dot_and_absolute_are_invalid() {
    let dir = tempfile::tempdir().unwrap();

    let p_dot = dir.path().join("p_dot");
    fs::create_dir(&p_dot).unwrap();
    assert!(
        matches!(
            registration::register(&p_dot, "writing", vec![".".to_string()]),
            Err(KernelError::InvalidSidecar(_))
        ),
        "a `.` content root must be InvalidSidecar"
    );

    let p_abs = dir.path().join("p_abs");
    fs::create_dir(&p_abs).unwrap();
    assert!(
        matches!(
            registration::register(&p_abs, "writing", vec!["/etc".to_string()]),
            Err(KernelError::InvalidSidecar(_))
        ),
        "an absolute content root must be InvalidSidecar"
    );
}

/// ADV-M2-9 — documented trade-off, asserted rather than hidden: a write whose
/// FINAL component is a symlink is refused even when the link stays inside the
/// root, because the ultimate target of a dangling link cannot be proven
/// in-root without following it. The in-root target must be left untouched.
#[test]
fn adv_in_root_symlink_write_target_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    fs::write(root.join("real.txt"), "ORIGINAL").unwrap();
    std::os::unix::fs::symlink(root.join("real.txt"), root.join("link.txt")).unwrap();

    let mut broker = Broker::new(root.clone(), grants("link.txt"));
    let out = broker
        .request(BrokerRequest::WriteFile {
            path: "link.txt".to_string(),
            bytes: b"NEW".to_vec(),
        })
        .unwrap();

    assert!(
        matches!(out, BrokerOutcome::Denied(_)),
        "a symlink write target is refused by policy, got {:?}",
        out
    );
    assert_eq!(
        fs::read_to_string(root.join("real.txt")).unwrap(),
        "ORIGINAL",
        "the write must not have followed the in-root symlink"
    );
}
