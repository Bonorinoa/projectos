//! M2b reviewer-owned adversarial suite (native DocumentStore + proposals).
//!
//! The M2b slice pins every document operation to a project directory
//! descriptor: descendants are opened with `openat(O_NOFOLLOW)`, saves use a
//! unique `O_EXCL` sibling plus `renameat`, and revisions keep a
//! content-addressed before-image under `.projectos/objects/`.
//!
//! This file ATTACKS the claimed properties independently of `tests/document.rs`,
//! `tests/proposal_editor.rs` and `tests/m2_acceptance.rs`. It intentionally
//! compiles against *public* API only, uses `tempfile::tempdir` for
//! determinism, and never depends on timing or wall-clock ordering. Where a
//! claim does NOT hold, the test is kept and the review note
//! (`kernel/REVIEW-m2b-kernel.md`) names the exact assertion that fails.
//!
//! The one deliberate failing assertion is `metadata_refuses_the_objects_subtree`:
//! a `.toml` name under `objects/` is accepted by `write_metadata`/`read_metadata`.
//! Everything else in this file is expected to be green.

use projectos_kernel::{
    document::DocumentStore,
    proposals::{apply, Decision, Proposal, ProposalKind, RevisionLog},
    provider::stable_hash,
    selection::anchor,
};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

const MAX: usize = 1024 * 1024;

/// Any leftover `O_EXCL` save sibling under the store root is a leak that
/// proves a non-atomic write path ran.
fn save_temps(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(root) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with(".projectos-save-") {
                out.push(n);
            }
        }
    }
    out.sort();
    out
}

// ---------------------------------------------------------------------------
// Open / root pinning
// ---------------------------------------------------------------------------

/// Opening a descriptor through a symlinked root must be refused (O_NOFOLLOW).
#[test]
fn open_refuses_a_symlinked_root() {
    let container = tempfile::tempdir().unwrap();
    let real = container.path().join("real");
    fs::create_dir(&real).unwrap();
    let link = container.path().join("link");
    symlink(&real, &link).unwrap();
    assert!(
        DocumentStore::open(&link).is_err(),
        "open() must refuse a symlinked project root"
    );
}

/// After the project root is renamed + recreated as a symlink to an outside
/// dir, every operation fails closed and reaches nothing outside.
#[test]
fn root_swapped_for_symlink_fails_closed_and_touches_nothing_outside() {
    let container = tempfile::tempdir().unwrap();
    let root = container.path().join("project");
    fs::create_dir_all(root.join("chapter")).unwrap();
    fs::write(root.join("chapter/draft.md"), "before").unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("draft.md"), "outside").unwrap();

    let store = DocumentStore::open(&root).unwrap();
    // A healthy project already has a sidecar before the attack.
    store.write_metadata("revisions.toml", "revisions = []").unwrap();

    fs::rename(&root, container.path().join("held")).unwrap();
    symlink(outside.path(), &root).unwrap();

    assert!(store.list_text_files().is_err(), "list must fail closed");
    assert!(store.read("chapter/draft.md").is_err(), "read must fail closed");
    assert!(store.create("new.md").is_err(), "create must fail closed");
    assert!(
        store.save("chapter/draft.md", &stable_hash(b"before"), "x").is_err(),
        "save must fail closed"
    );
    assert!(
        store.write_metadata("receipts.toml", "receipts = []").is_err(),
        "write_metadata must fail closed"
    );
    assert!(
        store.read_metadata("revisions.toml").is_err(),
        "read_metadata must fail closed"
    );
    assert_eq!(
        fs::read_to_string(outside.path().join("draft.md")).unwrap(),
        "outside",
        "the outside target must stay untouched"
    );
}

/// A subdirectory swapped for a symlink is refused even for a READ of a file
/// that legitimately exists on the other side; restoring the real dir works.
#[test]
fn subdirectory_swapped_for_symlink_is_refused_for_read() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("chapter")).unwrap();
    fs::write(root.path().join("chapter/draft.md"), "inside").unwrap();
    fs::write(outside.path().join("secret.md"), "outside").unwrap();

    let store = DocumentStore::open(root.path()).unwrap();
    fs::rename(root.path().join("chapter"), root.path().join("held")).unwrap();
    symlink(outside.path(), root.path().join("chapter")).unwrap();

    assert!(store.read("chapter/draft.md").is_err(), "read through a swapped dir must be refused");
    assert!(store.create("chapter/other.md").is_err(), "create through a swapped dir must be refused");
    assert_eq!(fs::read_to_string(outside.path().join("secret.md")).unwrap(), "outside");

    // Restoring the directory makes the pinned store work again on the real bytes.
    fs::remove_file(root.path().join("chapter")).unwrap();
    fs::rename(root.path().join("held"), root.path().join("chapter")).unwrap();
    assert_eq!(store.read("chapter/draft.md").unwrap(), "inside");
}

// ---------------------------------------------------------------------------
// Lexical path rejection
// ---------------------------------------------------------------------------

#[test]
fn lexical_rejections_cover_slashes_dots_windows_and_hidden_names() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("pwn.txt"), "untouched").unwrap();
    let store = DocumentStore::open(root.path()).unwrap();
    store.create("draft.md").unwrap();
    let empty = stable_hash(b"");

    let bad = [
        "draft.md/",       // trailing slash
        "//draft.md",      // leading double slash
        "a\\b.md",         // Windows backslash
        "a\0b.md",         // NUL byte
        ".hidden.md",      // leading-dot file
        "dir/.hidden.md",  // hidden component
        ".projectos/x.md", // .projectos as a document
        ".projectos",      // bare .projectos
        "draft",           // extensionless text
        "draft.md.",       // trailing-dot extension
        "../draft.md",     // parent traversal
        "a/../draft.md",   // embedded parent
        "a/./draft.md",    // embedded dot
        "a//b.md",         // double separator
        "a/..",            // traversal-only
        "/draft.md",       // absolute
        "C:/draft.md",     // windows drive
        "../pwn.txt",      // traversal to a real outside file
    ];
    for rel in bad {
        assert!(store.read(rel).is_err(), "read {rel:?}");
        assert!(store.save(rel, &empty, "x").is_err(), "save {rel:?}");
        assert!(store.create(rel).is_err(), "create {rel:?}");
    }
    assert_eq!(
        fs::read_to_string(outside.path().join("pwn.txt")).unwrap(),
        "untouched",
        "the outside victim file must never be created or overwritten"
    );
}

#[test]
fn extensions_are_case_insensitive_and_only_text_extensions_are_accepted() {
    let root = tempfile::tempdir().unwrap();
    let store = DocumentStore::open(root.path()).unwrap();
    store.create("UPPER.MD").unwrap();
    store.save("UPPER.MD", &stable_hash(b""), "ok").unwrap();
    assert_eq!(store.read("UPPER.MD").unwrap(), "ok");
    store.create("note.txT").unwrap();
    assert_eq!(store.read("note.txT").unwrap(), "");
    for rel in ["x.json", "y.md.exe", "z.html", "a.c", "noext"] {
        assert!(store.create(rel).is_err(), "create {rel}");
        assert!(store.save(rel, &stable_hash(b""), "x").is_err(), "save {rel}");
        assert!(store.read(rel).is_err(), "read {rel}");
    }
}

// ---------------------------------------------------------------------------
// Bounds: size, UTF-8, atomicity
// ---------------------------------------------------------------------------

#[test]
fn size_bounds_refuse_over_limit_and_exactly_the_limit_is_writable() {
    let root = tempfile::tempdir().unwrap();
    let store = DocumentStore::open(root.path()).unwrap();
    store.create("cap.md").unwrap();
    store.save("cap.md", &stable_hash(b""), &"a".repeat(MAX)).unwrap();
    assert_eq!(store.read("cap.md").unwrap().len(), MAX, "the exact 1 MiB limit is legal");

    let capped = fs::read(root.path().join("cap.md")).unwrap();
    assert!(
        store.save("cap.md", &stable_hash(&capped), &"b".repeat(MAX + 1)).is_err(),
        "a save over 1 MiB must be refused"
    );
    assert_eq!(fs::read(root.path().join("cap.md")).unwrap(), capped, "over-limit save must not touch the file");

    fs::write(root.path().join("big.md"), vec![b'x'; MAX + 1]).unwrap();
    assert!(store.read("big.md").is_err(), "read of an over-limit file must be refused");
    assert!(
        store.save("big.md", &stable_hash(&vec![b'x'; MAX + 1]), "y").is_err(),
        "save of the over-limit file must be refused"
    );
    assert_eq!(fs::read(root.path().join("big.md")).unwrap().len(), MAX + 1, "the over-limit file is untouched");
}

#[test]
fn invalid_utf8_document_is_refused_and_never_partially_overwritten() {
    let root = tempfile::tempdir().unwrap();
    let bad: Vec<u8> = vec![b'h', b'i', 0xff, b'\n']; // invalid UTF-8
    fs::write(root.path().join("bin.txt"), &bad).unwrap();
    let store = DocumentStore::open(root.path()).unwrap();
    assert!(store.read("bin.txt").is_err(), "non-UTF-8 read must be refused");
    assert!(
        store.save("bin.txt", &stable_hash(&bad), "clean").is_err(),
        "saving over a non-UTF-8 document must be refused"
    );
    assert_eq!(fs::read(root.path().join("bin.txt")).unwrap(), bad, "non-UTF-8 bytes are untouched");
}

#[test]
fn failed_saves_leave_original_bytes_and_never_leak_a_temp_sibling() {
    let root = tempfile::tempdir().unwrap();
    let store = DocumentStore::open(root.path()).unwrap();
    store.create("draft.md").unwrap();
    store.save("draft.md", &stable_hash(b""), "v1").unwrap();
    let v1 = fs::read(root.path().join("draft.md")).unwrap();

    assert!(store.save("draft.md", &stable_hash(b"wrong"), "v2").is_err(), "stale hash refused");
    assert_eq!(fs::read(root.path().join("draft.md")).unwrap(), v1, "bytes unchanged after a stale save");
    assert!(save_temps(root.path()).is_empty(), "stale save leaked: {:?}", save_temps(root.path()));

    store.save("draft.md", &stable_hash(&v1), "v3").unwrap();
    assert_eq!(store.read("draft.md").unwrap(), "v3");
    assert!(save_temps(root.path()).is_empty(), "successful save leaked: {:?}", save_temps(root.path()));
}

#[test]
fn save_with_wrong_expected_hash_is_refused_and_bytes_untouched() {
    let root = tempfile::tempdir().unwrap();
    let store = DocumentStore::open(root.path()).unwrap();
    store.create("draft.md").unwrap();
    store.save("draft.md", &stable_hash(b""), "original").unwrap();
    for wrong in [
        stable_hash(b"not-original"),
        stable_hash(b"ORIGINAL"),
        "0000000000000000".to_string(),
        stable_hash(b"original\n"),
    ] {
        let before = fs::read(root.path().join("draft.md")).unwrap();
        assert!(store.save("draft.md", &wrong, "overwrite").is_err(), "wrong hash {wrong} accepted");
        assert_eq!(fs::read(root.path().join("draft.md")).unwrap(), before);
    }
    store.save("draft.md", &stable_hash(b"original"), "revised").unwrap();
    assert_eq!(store.read("draft.md").unwrap(), "revised");
}

// ---------------------------------------------------------------------------
// Metadata scoping
// ---------------------------------------------------------------------------

/// Claim: metadata paths are `.projectos`-relative TOML names; draft paths,
/// escapes and symlinked targets are refused. NOTE: the `objects/...` subtree
/// is claimed to be refused as well — that specific assertion FAILS (see note)
/// and this test documents the deviation.
#[test]
fn metadata_refuses_the_objects_subtree() {
    let root = tempfile::tempdir().unwrap();
    let store = DocumentStore::open(root.path()).unwrap();
    // Claim: write_metadata/read_metadata refuse any name under objects/.
    assert!(
        store.write_metadata("objects/probe.toml", "x = 1").is_err(),
        "write_metadata MUST refuse a .toml name under objects/"
    );
    assert!(
        store.read_metadata("objects/probe.toml").is_err(),
        "read_metadata MUST refuse a .toml name under objects/"
    );
    assert!(
        store.write_metadata("objects/nested/probe.toml", "x = 1").is_err(),
        "write_metadata MUST refuse a nested objects/ name"
    );
    assert!(
        !root.path().join(".projectos/objects/probe.toml").exists(),
        "objects/ must never be written by write_metadata"
    );
}

#[test]
fn metadata_paths_are_scoped_and_escape_and_symlinked_targets_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let store = DocumentStore::open(root.path()).unwrap();
    store.write_metadata("graph/snapshot.toml", "objects = []").unwrap();
    assert_eq!(
        store.read_metadata("graph/snapshot.toml").unwrap().unwrap(),
        "objects = []"
    );

    for rel in [
        ".projectos/foo.toml", // double-prefix
        "draft.md",            // a document path
        "../x.toml",           // parent escape
        "a/../x.toml",         // embedded parent
        "/abs/x.toml",         // absolute
        "..",                  // pure traversal
        ".",                   // current dir
        "objects",             // objects dir without .toml extension
    ] {
        assert!(store.write_metadata(rel, "bad").is_err(), "write_metadata {rel:?}");
    }

    // A symlinked metadata target (and symlinked parent) inside .projectos is refused.
    fs::create_dir_all(root.path().join(".projectos")).unwrap();
    fs::write(outside.path().join("v.toml"), "outside").unwrap();
    symlink(outside.path().join("v.toml"), root.path().join(".projectos/links.toml")).unwrap();
    assert!(store.read_metadata("links.toml").is_err(), "symlinked metadata target read refused");
    assert!(store.write_metadata("links.toml", "x=1").is_err(), "symlinked metadata target write refused");
    symlink(outside.path(), root.path().join(".projectos/linkeddir")).unwrap();
    assert!(store.write_metadata("linkeddir/rev.toml", "x=1").is_err(), "symlinked metadata parent refused");
    assert_eq!(fs::read_to_string(outside.path().join("v.toml")).unwrap(), "outside");
}

// ---------------------------------------------------------------------------
// Stale-write protection, before-images, proposals
// ---------------------------------------------------------------------------

#[test]
fn replacement_after_hash_is_the_hash_of_the_whole_new_document() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("draft.md"), "aaa café bbb").unwrap();
    let a = anchor(root.path(), "draft.md", 4, 9).unwrap(); // "café" spans bytes 4..9
    let p = Proposal::new("draft.md", ProposalKind::Replacement, "café", "tea", &a.doc_hash);
    let rev = apply(root.path(), &a, &p, Decision::Accept).unwrap().unwrap();
    assert_eq!(rev.before_hash, a.doc_hash);
    let on_disk = fs::read_to_string(root.path().join("draft.md")).unwrap();
    assert_eq!(on_disk, "aaa tea bbb", "selected bytes replaced");
    // The M2 property: revision after_hash == hash of the whole new document.
    assert_eq!(rev.after_hash, stable_hash(b"aaa tea bbb"));
    assert_ne!(rev.after_hash, p.after_hash, "after_hash is the proposed-snippet hash, not the document hash");
    assert!(save_temps(root.path()).is_empty(), "accept leaked a temp: {:?}", save_temps(root.path()));
}

#[test]
fn annotation_acceptance_keeps_draft_unchanged_and_requires_a_fresh_document() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("draft.md"), "claim topic").unwrap();
    let a = anchor(root.path(), "draft.md", 0, 5).unwrap(); // "claim"
    let p = Proposal::new("draft.md", ProposalKind::Annotation, "claim", "an explanation", &a.doc_hash);
    assert!(apply(root.path(), &a, &p, Decision::Accept).unwrap().is_none());
    assert_eq!(fs::read_to_string(root.path().join("draft.md")).unwrap(), "claim topic");
    let revs = RevisionLog::new(root.path()).unwrap();
    assert!(revs.revisions().is_empty(), "an annotation records no revision");
    let receipts = fs::read_to_string(root.path().join(".projectos/receipts.toml")).unwrap();
    assert!(receipts.contains("accepted-annotation"));

    // A stale document makes even an annotation acceptance fail before mutation.
    fs::write(root.path().join("draft.md"), "changed").unwrap();
    let stale = fs::read(root.path().join("draft.md")).unwrap();
    assert!(apply(root.path(), &a, &p, Decision::Accept).is_err(), "stale annotation accept refused");
    assert_eq!(fs::read(root.path().join("draft.md")).unwrap(), stale, "stale annotation mutates nothing");
    assert!(
        RevisionLog::new(root.path()).unwrap().revisions().is_empty(),
        "stale annotation records no revision"
    );
}

#[test]
fn rejection_mutates_nothing_and_records_a_rejected_receipt() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("draft.md"), "sentinel body").unwrap();
    let a = anchor(root.path(), "draft.md", 0, 8).unwrap(); // "sentinel"
    let p = Proposal::new("draft.md", ProposalKind::Annotation, "sentinel", "note", &a.doc_hash);
    let draft = fs::read(root.path().join("draft.md")).unwrap();
    assert!(apply(root.path(), &a, &p, Decision::Reject).unwrap().is_none());
    assert_eq!(fs::read(root.path().join("draft.md")).unwrap(), draft, "a rejection mutates nothing");
    assert!(RevisionLog::new(root.path()).unwrap().revisions().is_empty());
    let receipts = fs::read_to_string(root.path().join(".projectos/receipts.toml")).unwrap();
    assert!(receipts.contains("rejected"));
    assert!(!receipts.contains("accepted"), "a rejection must not be recorded as an acceptance");
}

#[test]
fn malformed_proposals_are_refused_before_mutation_for_accept_and_reject() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("draft.md"), "abcdef").unwrap();
    let a = anchor(root.path(), "draft.md", 0, 3).unwrap(); // "abc"
    let base = Proposal::new("draft.md", ProposalKind::Replacement, "abc", "XYZ", &a.doc_hash);
    let draft = fs::read(root.path().join("draft.md")).unwrap();

    for dec in [Decision::Accept, Decision::Reject] {
        let mut p = base.clone();
        p.before_hash = "0000000000000000".into();
        assert!(apply(root.path(), &a, &p, dec).is_err(), "{dec:?}: wrong before_hash");
        let mut p = base.clone();
        p.original = "zzz".into();
        assert!(apply(root.path(), &a, &p, dec).is_err(), "{dec:?}: wrong original");
        let mut p = base.clone();
        p.after_hash = "0000000000000000".into();
        assert!(apply(root.path(), &a, &p, dec).is_err(), "{dec:?}: wrong after_hash");
        let mut p = base.clone();
        p.id = "forged-id".into();
        assert!(apply(root.path(), &a, &p, dec).is_err(), "{dec:?}: wrong id");
        let mut p = base.clone();
        p.rel_path = "other.md".into();
        assert!(apply(root.path(), &a, &p, dec).is_err(), "{dec:?}: wrong rel_path");
        // An out-of-bounds / off-boundary anchor is refused for accept.
        let mut a2 = a.clone();
        a2.byte_end = 999;
        assert!(apply(root.path(), &a2, &base, dec).is_err(), "{dec:?}: out-of-bounds anchor");
    }

    assert_eq!(fs::read(root.path().join("draft.md")).unwrap(), draft, "no failed apply mutates the draft");
    assert!(RevisionLog::new(root.path()).unwrap().revisions().is_empty());
    if let Ok(text) = fs::read_to_string(root.path().join(".projectos/receipts.toml")) {
        assert!(!text.contains("rejected") && !text.contains("accepted"));
    }
}

#[test]
fn before_images_are_retained_deduped_and_only_the_exact_hash_is_verified() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("draft.md"), "hello world").unwrap();
    // A corrupt blob under a DIFFERENT hash must not block a valid apply.
    fs::create_dir_all(root.path().join(".projectos/objects")).unwrap();
    fs::write(root.path().join(".projectos/objects/aaaaaaaaaaaaaaaa"), "corrupt").unwrap();

    let a = anchor(root.path(), "draft.md", 0, 5).unwrap(); // "hello"
    let p = Proposal::new("draft.md", ProposalKind::Replacement, "hello", "greetings", &a.doc_hash);
    apply(root.path(), &a, &p, Decision::Accept).unwrap();
    assert_eq!(fs::read_to_string(root.path().join("draft.md")).unwrap(), "greetings world");
    let blob = fs::read(root.path().join(".projectos/objects").join(&a.doc_hash)).unwrap();
    assert_eq!(blob, b"hello world", "before-image is byte-identical and content-addressed");

    // A second distinct revision keeps its own before-image (no overwrite).
    let a2 = anchor(root.path(), "draft.md", 0, 9).unwrap(); // "greetings"
    let p2 = Proposal::new("draft.md", ProposalKind::Replacement, "greetings", "hi", &a2.doc_hash);
    apply(root.path(), &a2, &p2, Decision::Accept).unwrap();
    assert_eq!(fs::read(root.path().join(".projectos/objects").join(&a2.doc_hash)).unwrap(), b"greetings world");
    assert_eq!(RevisionLog::new(root.path()).unwrap().revisions().len(), 2);
}

#[test]
fn corrupt_before_image_at_the_exact_hash_fails_before_any_mutation() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("draft.md"), "abcdefgh").unwrap();
    let a = anchor(root.path(), "draft.md", 0, 3).unwrap(); // "abc"
    let p = Proposal::new("draft.md", ProposalKind::Replacement, "abc", "XYZ", &a.doc_hash);
    let before = fs::read(root.path().join("draft.md")).unwrap();
    fs::create_dir_all(root.path().join(".projectos/objects")).unwrap();
    fs::write(root.path().join(".projectos/objects").join(&a.doc_hash), "tampered-content").unwrap();

    assert!(apply(root.path(), &a, &p, Decision::Accept).is_err(), "corrupt exact before-image refused");
    assert_eq!(fs::read(root.path().join("draft.md")).unwrap(), before, "a failed apply never touches the draft");
    assert!(RevisionLog::new(root.path()).unwrap().revisions().is_empty());

    // The same hash with the correct content dedups and the apply proceeds.
    fs::write(root.path().join(".projectos/objects").join(&a.doc_hash), b"abcdefgh").unwrap();
    apply(root.path(), &a, &p, Decision::Accept).unwrap();
    assert_eq!(fs::read_to_string(root.path().join("draft.md")).unwrap(), "XYZdefgh");
}

#[test]
fn an_accepted_replacement_cannot_be_reapplied_to_a_now_stale_document() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("draft.md"), "start here").unwrap();
    let a = anchor(root.path(), "draft.md", 0, 5).unwrap(); // "start"
    let p = Proposal::new("draft.md", ProposalKind::Replacement, "start", "launch", &a.doc_hash);
    apply(root.path(), &a, &p, Decision::Accept).unwrap();
    assert_eq!(fs::read_to_string(root.path().join("draft.md")).unwrap(), "launch here");

    // The same anchor/proposal is now stale: it must be refused, not double-applied.
    assert!(apply(root.path(), &a, &p, Decision::Accept).is_err(), "reapply of a consumed proposal refused");
    assert_eq!(fs::read_to_string(root.path().join("draft.md")).unwrap(), "launch here");
    assert_eq!(RevisionLog::new(root.path()).unwrap().revisions().len(), 1);
}