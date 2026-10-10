//! Child B acceptance tests for the capability broker (frozen contract).
//!
//! Five cases, per `INTERFACES.md`:
//! 1. granted read returns bytes; ungranted read -> `Denied` + receipt;
//! 2. `../escape`, absolute path, and a symlink pointing outside the root are
//!    all `Denied` and the outside file is demonstrably not read;
//! 3. granted write lands the bytes; ungranted write denied;
//! 4. receipts never contain absolute paths or file bytes (redaction);
//! 5. `InProcessWorker` routes every op through the broker (its denials
//!    behave identically to the broker's).

use projectos_kernel::broker::{
    Broker, BrokerOutcome, BrokerRequest, Capability, CapabilitySet,
};
use projectos_kernel::error::KernelError;
use projectos_kernel::worker::{InProcessWorker, WorkerHost, WorkerOp};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

/// Helpers -------------------------------------------------------------------

/// A grant set granting read+write of exactly `rel`.
fn grants(rel: &str) -> CapabilitySet {
    let mut g = CapabilitySet::new();
    g.push(Capability::ReadFile(rel.to_string()));
    g.push(Capability::WriteFile(rel.to_string()));
    g
}

/// Read the root's canonical string form.
fn root_canon_str(root: &Path) -> String {
    fs::canonicalize(root)
        .unwrap()
        .to_string_lossy()
        .to_string()
}

/// Test 1: granted read returns bytes; ungranted read -> Denied + receipt -----

#[test]
fn granted_read_returns_bytes_ungranted_is_denied_with_receipt() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let inside = root.join("note.txt");
    fs::write(&inside, "hello broker").unwrap();

    // Granted read returns bytes.
    let mut broker = Broker::new(root.clone(), grants("note.txt"));
    let out = broker.request(BrokerRequest::ReadFile("note.txt".to_string()))
        .unwrap();
    assert_eq!(out, BrokerOutcome::Read(b"hello broker".to_vec()));
    assert!(broker.receipts().is_empty(), "no receipt on success");

    // Ungranted read -> Denied + a receipt appended.
    let out = broker.request(BrokerRequest::ReadFile("other.txt".to_string()))
        .unwrap();
    assert!(matches!(out, BrokerOutcome::Denied(ref s) if s.contains("not granted")));
    assert_eq!(broker.receipts().len(), 1, "exactly one receipt appended");
    let r = &broker.receipts()[0];
    assert_eq!(r.op, "read");
    assert_eq!(r.rel_path, "other.txt");
    assert!(!r.outcome.is_empty());
}

/// Test 2: path-escape attempts are all Denied and don't leak --------------

#[test]
fn escape_attempts_denied_and_outside_file_not_read() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let secret = root.join("..").join("outside_secret.txt");
    fs::write(&secret, "TOPSECRET").unwrap();
    fs::write(root.join("inner.txt"), "inside").unwrap();

    // The outside file must be readable via its absolute path for setup, but
    // the broker must never return its bytes.
    let mut broker = Broker::new(root.clone(), grants("inner.txt"));

    // 2a. `../escape` component.
    let out = broker
        .request(BrokerRequest::ReadFile("../outside_secret.txt".to_string()))
        .unwrap();
    assert!(matches!(out, BrokerOutcome::Denied(_)));

    // 2b. absolute path.
    let abs = secret.to_string_lossy().to_string();
    let out = broker.request(BrokerRequest::ReadFile(abs.clone())).unwrap();
    match out {
        BrokerOutcome::Denied(_) => {}
        BrokerOutcome::Read(b) => panic!(
            "absolute path was NOT denied; leaked bytes: {:?}",
            String::from_utf8_lossy(&b)
        ),
        BrokerOutcome::Written => panic!("absolute path returned Written"),
    }

    // 2c. symlink pointing outside the root (created inside the root).
    let link = root.join("evil_link.txt");
    std::os::unix::fs::symlink(&secret, &link).unwrap();
    // Grant the link's relative name so the grant check passes and path
    // resolution is the only gate.
    let mut broker = Broker::new(root.clone(), grants("evil_link.txt"));
    let out = broker
        .request(BrokerRequest::ReadFile("evil_link.txt".to_string()))
        .unwrap();
    match out {
        BrokerOutcome::Denied(ref s) => assert!(s.contains("escape") || s.contains("symlink")),
        BrokerOutcome::Read(b) => panic!(
            "symlink escape was NOT denied; leaked bytes: {:?}",
            String::from_utf8_lossy(&b)
        ),
        BrokerOutcome::Written => panic!("symlink escape returned Written"),
    }

    // Every denied read must NOT have returned the outside bytes.
    for (i, r) in broker.receipts().iter().enumerate() {
        assert_ne!(
            r.outcome, "TOPSECRET",
            "receipt {} outcome leaked file bytes", i
        );
    }
    // Sanity: the outside file genuinely exists and contains the secret — the
    // broker (not the test) is what must fail to read it.
    assert_eq!(fs::read_to_string(&secret).unwrap(), "TOPSECRET");
}

/// Test 3: granted write lands bytes; ungranted write denied ---------------

#[test]
fn granted_write_lands_bytes_ungranted_denied() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let target = root.join("out.txt");

    // Granted write lands the bytes.
    let mut broker = Broker::new(root.clone(), grants("out.txt"));
    let out = broker
        .request(BrokerRequest::WriteFile {
            path: "out.txt".to_string(),
            bytes: b"written".to_vec(),
        })
        .unwrap();
    assert_eq!(out, BrokerOutcome::Written);
    assert_eq!(fs::read(&target).unwrap(), b"written");

    // Ungranted write to a fresh root (empty grants) -> Denied and the file is
    // not created. A separate tempdir isolates this from the granted write above,
    // whose `out.txt` already exists.
    let dir2 = tempdir().unwrap();
    let root2 = dir2.path().to_path_buf();
    let target2 = root2.join("out.txt");
    assert!(!target2.try_exists().unwrap_or(false));
    {
        let mut broker = Broker::new(root2.clone(), CapabilitySet::new());
        let out = broker
            .request(BrokerRequest::WriteFile {
                path: "out.txt".to_string(),
                bytes: b"no".to_vec(),
            })
            .unwrap();
        assert!(matches!(out, BrokerOutcome::Denied(_)));
        assert!(
            !target2.try_exists().unwrap_or(false),
            "denied write created the file"
        );
    }
}

/// Test 4: receipts contain no absolute paths and no file bytes ------------

#[test]
fn receipts_are_redacted_no_absolute_paths_no_bytes() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();
    fs::write(root.join("real.txt"), "RUST").unwrap();

    let mut broker = Broker::new(root.clone(), grants("real.txt"));
    let root_str = root_canon_str(&root);

    // A denial that names an escape path (absolute) and a missing file.
    let _ = broker
        .request(BrokerRequest::ReadFile("/etc/passwd".to_string()))
        .unwrap();
    let _ = broker
        .request(BrokerRequest::ReadFile("nonexistent.txt".to_string()))
        .unwrap();
    let _ = broker
        .request(BrokerRequest::ReadFile("real.txt".to_string()))
        .unwrap(); // success: no receipt

    let receipts = broker.receipts();
    assert!(!receipts.is_empty(), "at least one receipt exists");
    for r in receipts {
        // rel_path must not be absolute (no leading '/', no drive letters).
        assert!(
            !Path::new(&r.rel_path).is_absolute(),
            "receipt rel_path is absolute: {}",
            r.rel_path
        );
        // The canonical root string must not appear verbatim in the receipt
            // text (which would leak an absolute path).
        assert!(
            !r.rel_path.contains(&root_str),
            "receipt leaked absolute root: {}",
            r.rel_path
        );
        assert!(
            !r.outcome.contains(&root_str),
            "receipt outcome leaked absolute root: {}",
            r.outcome
        );
        // No file bytes: the secret content "RUST" must never appear in any
        // receipt field.
        assert!(
            !r.rel_path.contains("RUST") && !r.outcome.contains("RUST") && !r.op.contains("RUST"),
            "receipt leaked file bytes: {:?}",
            r
        );
    }
}

/// Test 5: InProcessWorker routes every op through the broker --------------

#[test]
fn in_process_worker_routes_through_broker() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();
    fs::write(root.join("w.txt"), "via-worker").unwrap();

    // Broker with a grant.
    let mut broker = Broker::new(root.clone(), grants("w.txt"));
    let mut worker = InProcessWorker::new();
    worker.start(&mut broker).unwrap();

    // Granted read through the worker returns bytes.
    let out = worker
        .invoke(WorkerOp {
            request: BrokerRequest::ReadFile("w.txt".to_string()),
        })
        .unwrap();
    assert_eq!(out, BrokerOutcome::Read(b"via-worker".to_vec()));

    // Ungranted read through the worker is Denied identically to the broker.
    let worker_out = worker
        .invoke(WorkerOp {
            request: BrokerRequest::ReadFile("nope.txt".to_string()),
        })
        .unwrap();
    let broker_out = broker
        .request(BrokerRequest::ReadFile("nope.txt".to_string()))
        .unwrap();
    assert_eq!(worker_out, broker_out, "worker denial must match broker");
    assert!(matches!(worker_out, BrokerOutcome::Denied(_)));

    // Escape through the worker is Denied identically to the broker.
    let worker_out = worker
        .invoke(WorkerOp {
            request: BrokerRequest::ReadFile("../outside.txt".to_string()),
        })
        .unwrap();
    let broker_out = broker
        .request(BrokerRequest::ReadFile("../outside.txt".to_string()))
        .unwrap();
    assert_eq!(worker_out, broker_out, "worker escape denial must match broker");
    assert!(matches!(worker_out, BrokerOutcome::Denied(_)));

    // Worker receipts mirror the broker's pattern (redacted).
    let wr = worker.receipts();
    assert!(wr.iter().all(|r| Path::new(&r.rel_path).is_absolute() == false));

    // cancel() is a documented no-op success.
    assert!(worker.cancel().is_ok());
}

/// Sanity: a granted-but-missing file surfaces as `KernelError::Io`, not a denial.
#[test]
fn granted_but_missing_file_is_io_error() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let mut broker = Broker::new(root.clone(), grants("missing.txt"));
    let out = broker.request(BrokerRequest::ReadFile("missing.txt".to_string()));
    match out {
        Err(KernelError::Io(_)) => {} // expected: genuine I/O failure
        other => panic!("expected KernelError::Io, got {:?}", other),
    }
}
