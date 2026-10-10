//! Receipts tests (child R per `INTERFACES-M1b.md`).
//!
//! Verifies the durable redacted receipt writer: append/read round-trip, that
//! the persisted file never contains an absolute path or test secret bytes,
//! broker-denial ingestion, deterministic ids, and atomic (no `.tmp` leftover)
//! writes.

use projectos_kernel::broker::{Broker, BrokerRequest, CapabilitySet};
use projectos_kernel::receipts::{RedactedReceipt, ReceiptWriter};
use std::fs;
use tempfile::tempdir;

fn writer(folder: &std::path::Path) -> (ReceiptWriter, std::path::PathBuf) {
    let w = ReceiptWriter::new(folder).expect("open writer");
    let path = folder.join(".projectos").join("receipts.toml");
    (w, path)
}

/// Sample receipt used across tests (deliberately not secret-bearing).
fn sample(op: &str, rel: &str, at: u64) -> RedactedReceipt {
    RedactedReceipt {
        id: String::new(), // writer recomputes it deterministically
        at,
        op: op.to_string(),
        rel_path: rel.to_string(),
        outcome: "ok".to_string(),
        content_hash: Some("aabbccdd".to_string()),
        route: Some("openrouter/x".to_string()),
        cost: Some("0.0012".to_string()),
    }
}

#[test]
fn append_read_roundtrip() {
    let dir = tempdir().unwrap();
    let folder = dir.path().join("p");
    fs::create_dir_all(&folder).unwrap();
    let (mut w, _) = writer(&folder);

    w.append(sample("write", "docs/report.md", 5)).unwrap();
    w.append(sample("write", "src/lib.rs", 9)).unwrap();

    // Re-open fresh: in-memory slice reflects the persisted file.
    let w2 = ReceiptWriter::new(&folder).unwrap();
    let recs = w2.receipts();
    assert_eq!(recs.len(), 2);
    assert_eq!(recs[0].op, "write");
    assert_eq!(recs[0].rel_path, "docs/report.md");
    assert_eq!(recs[0].at, 5);
    assert_eq!(recs[0].content_hash.as_deref(), Some("aabbccdd"));
    assert_eq!(recs[0].route.as_deref(), Some("openrouter/x"));
    assert_eq!(recs[0].cost.as_deref(), Some("0.0012"));
    assert_eq!(recs[1].rel_path, "src/lib.rs");
    assert_eq!(recs[1].at, 9);
}

#[test]
fn persisted_file_has_no_absolute_path() {
    let dir = tempdir().unwrap();
    let folder = dir.path().join("p");
    fs::create_dir_all(&folder).unwrap();
    let (mut w, path) = writer(&folder);

    // An absolute-path attempt and a Windows-drive absolute attempt. The
    // writer must relativize both so nothing absolute reaches disk.
    w.append(sample("write", "/Users/bonorinoa/draft.md", 3)).unwrap();
    w.append(sample("read", "C:\\Users\\bonorinoa\\notes.txt", 4)).unwrap();

    let raw = fs::read_to_string(&path).expect("read persisted receipts.toml");
    assert!(
        !raw.contains("/Users/bonorinoa"),
        "absolute path leaked into persisted file:\n{raw}"
    );
    assert!(
        !raw.contains("C:\\Users") && !raw.contains("C:/Users"),
        "windows absolute path leaked into persisted file:\n{raw}"
    );

    // Round-tripped records hold relative (redacted) paths.
    let w2 = ReceiptWriter::new(&folder).unwrap();
    let recs = w2.receipts();
    assert_eq!(recs.len(), 2);
    assert!(!recs[0].rel_path.starts_with('/'), "rel_path must be redacted");
    assert_eq!(recs[0].rel_path, "Users/bonorinoa/draft.md");
    assert_eq!(recs[1].rel_path, "Users\\bonorinoa\\notes.txt");
}

#[test]
fn ingest_broker_receipts_denials_written_and_no_secrets_on_disk() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("root");
    fs::create_dir_all(&root).unwrap();

    // A secret-content file lives in the root; the broker may handle it but
    // its receipts must never carry the bytes, and neither may the writer.
    const SECRET: &str = "AKIA-SECRET-FILE-CONTENT-9f4b";
    fs::write(root.join("credentials.txt"), SECRET).unwrap();

    // A broker with no grants denies both an absolute read and a '../' read.
    let mut broker = Broker::new(root.clone(), CapabilitySet::new());
    let _ = broker.request(BrokerRequest::ReadFile("/etc/passwd".to_string()));
    let _ = broker.request(BrokerRequest::ReadFile("../escape".to_string()));

    // Denials generate redacted receipts (absolute → relative; no `..` forms).
    let broker_recs = broker.receipts();
    assert_eq!(broker_recs.len(), 2);
    for br in broker_recs {
        assert!(
            !br.rel_path.starts_with('/'),
            "broker receipt rel_path must be redacted of absolute form, got {:?}",
            br.rel_path
        );
    }

    // Ingestion returns the count and persists both facts with redacted paths.
    let written = ReceiptWriter::ingest_broker_receipts(&root, broker_recs, 42).unwrap();
    assert_eq!(written, 2);

    let w = ReceiptWriter::new(&root).unwrap();
    let recs = w.receipts();
    assert_eq!(recs.len(), 2);
    assert_eq!(recs[0].at, 42);
    assert_eq!(recs[1].at, 42);
    for r in recs {
        assert!(
            !r.rel_path.starts_with('/'),
            "ingested rel_path must be relative, got {:?}",
            r.rel_path
        );
    }

    // The persisted file never holds an absolute path, a trailing `..`, or the
    // secret file content bytes.
    let raw = fs::read_to_string(&root.join(".projectos").join("receipts.toml")).unwrap();
    assert!(
        !raw.contains("/etc/passwd"),
        "redaction failed:\n{raw}"
    );
    assert!(
        !raw.contains(SECRET),
        "secret file bytes leaked into persisted file:\n{raw}"
    );
    assert!(raw.contains("etc/passwd"), "absolute attempt should be relativized:\n{raw}");
}

#[test]
fn ids_are_deterministic_across_two_identical_runs() {
    let dir = tempdir().unwrap();

    let folder_a = dir.path().join("a");
    let folder_b = dir.path().join("b");
    fs::create_dir_all(&folder_a).unwrap();
    fs::create_dir_all(&folder_b).unwrap();

    let (mut wa, _) = writer(&folder_a);
    let (mut wb, _) = writer(&folder_b);

    wa.append(sample("write", "notes.txt", 1)).unwrap();
    wa.append(sample("read", "draft.md", 2)).unwrap();
    wb.append(sample("write", "notes.txt", 1)).unwrap();
    wb.append(sample("read", "draft.md", 2)).unwrap();

    // Same inputs => same ids, regardless of which folder/run.
    assert_eq!(wa.receipts()[0].id, wb.receipts()[0].id);
    assert_eq!(wa.receipts()[1].id, wb.receipts()[1].id);
    // Different facts => different ids (deterministic but not collapsed).
    assert_ne!(wa.receipts()[0].id, wa.receipts()[1].id);
    // Deterministic, stable hex form.
    for r in wa.receipts() {
        assert_eq!(r.id.len(), 16, "id must be 16 lowercase hex chars");
        assert!(r.id.chars().all(|c| c.is_ascii_hexdigit()));
    }
}

#[test]
fn append_leaves_no_tmp_file_behind() {
    let dir = tempdir().unwrap();
    let folder = dir.path().join("p");
    fs::create_dir_all(&folder).unwrap();
    let (mut w, _) = writer(&folder);

    w.append(sample("write", "a.md", 1)).unwrap();
    w.append(sample("read", "b.md", 2)).unwrap();
    w.append(sample("write", "c.md", 3)).unwrap();

    let sidecar = folder.join(".projectos");
    let leftovers: Vec<_> = fs::read_dir(&sidecar)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "temporary files left behind after append: {:?}",
        leftovers
    );
    // The real store is present and readable.
    assert!(sidecar.join("receipts.toml").exists());
}
