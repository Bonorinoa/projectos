use projectos_kernel::{document::DocumentStore, provider::stable_hash};
use std::fs;
use std::os::unix::fs::symlink;

#[test]
fn create_read_save_utf8_and_refuse_stale_or_overwrite() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("chapters")).unwrap();
    let store = DocumentStore::open(root.path()).unwrap();
    store.create("chapters/intro.md").unwrap();
    assert_eq!(store.read("chapters/intro.md").unwrap(), "");
    assert!(store.create("chapters/intro.md").is_err());
    store.save("chapters/intro.md", &stable_hash(b""), "A café ✨").unwrap();
    assert_eq!(store.read("chapters/intro.md").unwrap(), "A café ✨");
    assert!(store.save("chapters/intro.md", &stable_hash(b""), "stale").is_err());
    fs::write(root.path().join("chapters/intro.md"), "external edit").unwrap();
    assert!(store.save("chapters/intro.md", &stable_hash("A café ✨".as_bytes()), "overwrite").is_err());
    assert_eq!(store.read("chapters/intro.md").unwrap(), "external edit");
    assert!(store.create("missing/new.md").is_err());
}

#[test]
fn lexical_paths_and_symlinks_are_refused_for_every_operation() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("victim.md"), "untouched").unwrap();
    symlink(outside.path(), root.path().join("linked")).unwrap();
    symlink(outside.path().join("victim.md"), root.path().join("victim.md")).unwrap();
    let store = DocumentStore::open(root.path()).unwrap();
    for rel in ["../victim.md", "/tmp/victim.md", "./victim.md", "a/../victim.md", ".projectos/secret.md", "linked/victim.md", "victim.md", "a//victim.md", "bad.json", "C:/victim.md"] {
        assert!(store.read(rel).is_err(), "read {rel}");
        assert!(store.save(rel, &stable_hash(b"untouched"), "overwrite").is_err(), "save {rel}");
        assert!(store.create(rel).is_err(), "create {rel}");
    }
    assert_eq!(fs::read_to_string(outside.path().join("victim.md")).unwrap(), "untouched");
}

#[test]
fn discovery_is_bounded_and_skips_generated_hidden_links_and_invalid_text() {
    let root = tempfile::tempdir().unwrap();
    for dir in ["chapters", ".projectos", ".git", "node_modules", "vendor", "target"] {
        fs::create_dir(root.path().join(dir)).unwrap();
        fs::write(root.path().join(dir).join("draft.md"), "text").unwrap();
    }
    fs::write(root.path().join("README.MD"), "hello").unwrap();
    fs::write(root.path().join("raw.txt"), [0xff]).unwrap();
    fs::write(root.path().join("huge.txt"), vec![b'x'; 1024 * 1024 + 1]).unwrap();
    symlink(root.path().join("chapters"), root.path().join("alias")).unwrap();
    let store = DocumentStore::open(root.path()).unwrap();
    assert_eq!(store.list_text_files().unwrap(), ["README.MD", "chapters/draft.md"]);
    assert!(store.read("raw.txt").is_err());
    assert!(store.read("huge.txt").is_err());
    assert!(store.save("README.MD", &stable_hash(b"hello"), &"x".repeat(1024 * 1024 + 1)).is_err());
}

#[test]
fn metadata_scoped_atomic_and_symlink_safe() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("victim.toml"), "untouched").unwrap();
    let store = DocumentStore::open(root.path()).unwrap();
    assert!(store.read_metadata("graph/snapshot.toml").unwrap().is_none());
    store.write_metadata("graph/snapshot.toml", "objects = []").unwrap();
    store.write_metadata("graph/snapshot.toml", "objects = [1]").unwrap();
    assert_eq!(store.read_metadata("graph/snapshot.toml").unwrap().unwrap(), "objects = [1]");
    for rel in ["../victim.toml", "/tmp/victim.toml", ".projectos/foo.toml", "draft.md", "objects/deadbeef"] {
        assert!(store.write_metadata(rel, "bad").is_err());
    }
    symlink(outside.path().join("victim.toml"), root.path().join(".projectos/receipts.toml")).unwrap();
    assert!(store.read_metadata("receipts.toml").is_err());
    assert!(store.write_metadata("receipts.toml", "overwritten").is_err());
    symlink(outside.path(), root.path().join(".projectos/linked")).unwrap();
    assert!(store.write_metadata("linked/victim.toml", "overwritten").is_err());
    assert_eq!(fs::read_to_string(outside.path().join("victim.toml")).unwrap(), "untouched");
}

#[test]
fn pinned_store_refuses_a_replaced_root_and_swapped_directory() {
    let container = tempfile::tempdir().unwrap();
    let root = container.path().join("project");
    fs::create_dir_all(root.join("chapter")).unwrap();
    fs::write(root.join("chapter/draft.md"), "before").unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("draft.md"), "outside").unwrap();
    let store = DocumentStore::open(&root).unwrap();
    fs::rename(root.join("chapter"), root.join("old-chapter")).unwrap();
    symlink(outside.path(), root.join("chapter")).unwrap();
    assert!(store.save("chapter/draft.md", &stable_hash(b"before"), "bad").is_err());
    assert_eq!(fs::read_to_string(outside.path().join("draft.md")).unwrap(), "outside");
    fs::rename(&root, container.path().join("old-project")).unwrap();
    fs::create_dir(&root).unwrap();
    assert!(store.list_text_files().is_err());
    assert!(store.create("new.md").is_err());
    assert!(store.write_metadata("receipts.toml", "receipts = []").is_err());
}

#[test]
fn directory_swap_race_never_follows_outside_target() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("chapter")).unwrap();
    fs::write(root.path().join("chapter/draft.md"), "before").unwrap();
    fs::write(outside.path().join("draft.md"), "outside").unwrap();
    let store = DocumentStore::open(root.path()).unwrap();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            for _ in 0..100 {
                fs::rename(root.path().join("chapter"), root.path().join("held")).unwrap();
                symlink(outside.path(), root.path().join("chapter")).unwrap();
                std::thread::yield_now();
                fs::remove_file(root.path().join("chapter")).unwrap();
                fs::rename(root.path().join("held"), root.path().join("chapter")).unwrap();
            }
        });
        for _ in 0..100 {
            if let Ok(text) = store.read("chapter/draft.md") {
                let _ = store.save("chapter/draft.md", &stable_hash(text.as_bytes()), "inside");
            }
        }
    });
    assert_eq!(fs::read_to_string(outside.path().join("draft.md")).unwrap(), "outside");
}
