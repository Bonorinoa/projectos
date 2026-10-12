use projectos_kernel::{document::DocumentStore, proposals::{apply, Decision, Proposal, ProposalKind, RevisionLog}, provider::stable_hash, selection::anchor};
use std::{fs, os::unix::fs::symlink};

#[test]
fn stale_and_deleted_proposals_can_be_rejected_without_touching_document() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("draft.md"), "before").unwrap();
    let a = anchor(root.path(), "draft.md", 0, 6).unwrap();
    let p = Proposal::new("draft.md", ProposalKind::Replacement, "before", "after", &a.doc_hash);
    fs::write(root.path().join("draft.md"), "external change").unwrap();
    assert!(apply(root.path(), &a, &p, Decision::Reject).unwrap().is_none());
    assert_eq!(fs::read_to_string(root.path().join("draft.md")).unwrap(), "external change");
    fs::remove_file(root.path().join("draft.md")).unwrap();
    assert!(apply(root.path(), &a, &p, Decision::Reject).unwrap().is_none());
    assert!(!root.path().join("draft.md").exists());
    let receipts = projectos_kernel::receipts::ReceiptWriter::new(root.path()).unwrap();
    assert_eq!(receipts.receipts().len(), 2);
    assert!(receipts.receipts().iter().all(|r| r.outcome == "rejected"));
    assert!(RevisionLog::new(root.path()).unwrap().revisions().is_empty());
    let mut malformed = p.clone();
    malformed.original = "wrong".into();
    assert!(apply(root.path(), &a, &malformed, Decision::Reject).is_err());
}

#[test]
fn subsection_revision_hashes_complete_document_and_preserves_before_image() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("draft.md"), "Before café after").unwrap();
    let a = anchor(root.path(), "draft.md", 7, 12).unwrap();
    let p = Proposal::new("draft.md", ProposalKind::Replacement, "café", "tea", &a.doc_hash);
    let rev = apply(root.path(), &a, &p, Decision::Accept).unwrap().unwrap();
    assert_eq!(rev.after_hash, stable_hash(b"Before tea after"));
    assert_ne!(rev.after_hash, p.after_hash);
    let store = DocumentStore::open(root.path()).unwrap();
    assert_eq!(store.read_revision_blob(&a.doc_hash).unwrap(), "Before café after".as_bytes());
    assert_eq!(RevisionLog::new(root.path()).unwrap().revisions(), &[rev]);
}

#[test]
fn annotation_acceptance_keeps_draft_unchanged_and_records_no_revision() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("draft.md"), "claim").unwrap();
    let a = anchor(root.path(), "draft.md", 0, 5).unwrap();
    let p = Proposal::new("draft.md", ProposalKind::Annotation, "claim", "an explanation", &a.doc_hash);
    assert!(apply(root.path(), &a, &p, Decision::Accept).unwrap().is_none());
    assert_eq!(fs::read_to_string(root.path().join("draft.md")).unwrap(), "claim");
    assert!(RevisionLog::new(root.path()).unwrap().revisions().is_empty());
    assert!(fs::read_to_string(root.path().join(".projectos/receipts.toml")).unwrap().contains("accepted-annotation"));
}

#[test]
fn malformed_proposal_anchor_and_corrupt_before_image_fail_before_mutation() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("draft.md"), "café").unwrap();
    let a = anchor(root.path(), "draft.md", 0, 5).unwrap();
    let p = Proposal::new("draft.md", ProposalKind::Replacement, "café", "tea", &a.doc_hash);
    for field in 0..4 {
        let mut bad = p.clone();
        match field { 0 => bad.before_hash = "bad".into(), 1 => bad.original = "other".into(), 2 => bad.after_hash = "bad".into(), _ => bad.id = "bad".into() }
        assert!(apply(root.path(), &a, &bad, Decision::Accept).is_err());
    }
    let mut bad = a.clone();
    bad.byte_end = 4;
    bad.text_hash = stable_hash(&"café".as_bytes()[..4]);
    assert!(apply(root.path(), &bad, &p, Decision::Accept).is_err());
    fs::create_dir_all(root.path().join(".projectos/objects")).unwrap();
    fs::write(root.path().join(".projectos/objects").join(&a.doc_hash), "corrupt").unwrap();
    assert!(apply(root.path(), &a, &p, Decision::Accept).is_err());
    assert_eq!(fs::read_to_string(root.path().join("draft.md")).unwrap(), "café");
}

#[test]
fn old_fixed_temp_symlink_is_not_used_and_sidecar_symlink_is_denied() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let victim = outside.path().join("victim");
    fs::write(&victim, "outside").unwrap();
    fs::write(root.path().join("draft.md"), "before").unwrap();
    symlink(&victim, root.path().join("draft.projectos-tmp")).unwrap();
    let a = anchor(root.path(), "draft.md", 0, 6).unwrap();
    let p = Proposal::new("draft.md", ProposalKind::Replacement, "before", "after", &a.doc_hash);
    apply(root.path(), &a, &p, Decision::Accept).unwrap();
    assert_eq!(fs::read_to_string(&victim).unwrap(), "outside");
    fs::remove_dir_all(root.path().join(".projectos")).unwrap();
    symlink(outside.path(), root.path().join(".projectos")).unwrap();
    let a = anchor(root.path(), "draft.md", 0, 5).unwrap();
    let p = Proposal::new("draft.md", ProposalKind::Replacement, "after", "bad", &a.doc_hash);
    assert!(apply(root.path(), &a, &p, Decision::Accept).is_err());
    assert_eq!(fs::read_to_string(root.path().join("draft.md")).unwrap(), "after");
}
