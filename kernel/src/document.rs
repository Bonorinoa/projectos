//! Native UTF-8 document access pinned to a project directory descriptor.
//!
//! Every descendant is opened with `openat(O_NOFOLLOW)`. Replacing a directory
//! with a symlink cannot redirect these operations. Saves use a unique,
//! exclusively created sibling and compare the current bytes again before
//! rename. The comparison detects ordinary external edits; it is not a lock
//! against an uncooperative writer editing in the final compare/rename window.

use crate::error::{KernelError, Result};
use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

pub const MAX_DOCUMENT_BYTES: usize = 1024 * 1024;
const MAX_FILES: usize = 2000;
const MAX_ENTRIES: usize = 20_000;
const INTERNAL_LIMIT: usize = 16 * 1024 * 1024;

fn invalid(reason: impl Into<String>) -> KernelError {
    KernelError::InvalidSidecar(reason.into())
}

fn cstring(value: &str) -> Result<CString> {
    CString::new(value).map_err(|_| invalid("path contains a NUL byte"))
}

fn open_at(dir: &File, name: &str, flags: i32, mode: libc::mode_t) -> Result<File> {
    let name = cstring(name)?;
    // SAFETY: CString is terminated and dir is a live descriptor. A successful
    // descriptor is immediately owned by File; failed descriptors are not used.
    let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), flags | libc::O_CLOEXEC | libc::O_NOFOLLOW, mode as libc::c_uint) };
    if fd < 0 { return Err(std::io::Error::last_os_error().into()); }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn directory_at(dir: &File, name: &str) -> Result<File> {
    open_at(dir, name, libc::O_RDONLY | libc::O_DIRECTORY, 0)
}

fn same_file(a: &File, b: &File) -> Result<bool> {
    let a = a.metadata()?;
    let b = b.metadata()?;
    Ok(a.dev() == b.dev() && a.ino() == b.ino())
}

fn read_file(mut file: File, limit: usize) -> Result<String> {
    let meta = file.metadata()?;
    if !meta.is_file() { return Err(invalid("document must be a regular file")); }
    if meta.len() > limit as u64 { return Err(invalid("file exceeds the size limit")); }
    let mut bytes = Vec::new();
    (&mut file).take((limit + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > limit { return Err(invalid("file exceeds the size limit")); }
    String::from_utf8(bytes).map_err(|_| invalid("document is not UTF-8"))
}

fn parts(rel: &str, internal: bool) -> Result<Vec<String>> {
    if rel.is_empty() || rel.contains('\\') || rel.contains(':') || rel.contains('\0') {
        return Err(invalid("invalid relative document path"));
    }
    let mut result = Vec::new();
    for component in Path::new(rel).components() {
        let Component::Normal(name) = component else { return Err(invalid("path must be relative without dot traversal")); };
        let name = name.to_str().ok_or_else(|| invalid("path must be UTF-8"))?;
        if name.starts_with('.') && !(internal && result.is_empty() && name == ".projectos") {
            return Err(invalid("hidden paths are not editable documents"));
        }
        result.push(name.to_string());
    }
    if result.is_empty() || rel.split('/').any(|c| c.is_empty() || c == "." || c == "..") {
        return Err(invalid("invalid relative document path"));
    }
    if !internal && !text_extension(rel) { return Err(invalid("supported document extensions are .md, .markdown and .txt")); }
    Ok(result)
}

fn text_extension(rel: &str) -> bool {
    matches!(Path::new(rel).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref(), Some("md" | "markdown" | "txt"))
}

pub(crate) fn validate_document_path(rel: &str) -> Result<()> {
    parts(rel, false).map(|_| ())
}

/// A project-scoped native store. Keep it for the lifetime of the writer session.
pub struct DocumentStore {
    root: File,
    root_path: PathBuf,
}

impl DocumentStore {
    pub fn open(root: &Path) -> Result<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC).open(root)?;
        Ok(Self { root: file, root_path: root.to_path_buf() })
    }

    fn parent(&self, components: &[String], create: bool) -> Result<File> {
        if create { self.validate_root()?; }
        let mut dir = directory_at(&self.root, ".")?;
        for name in &components[..components.len() - 1] {
            if create {
                let name_c = cstring(name)?;
                let ret = unsafe { libc::mkdirat(dir.as_raw_fd(), name_c.as_ptr(), 0o700) };
                if ret < 0 {
                    let e = std::io::Error::last_os_error();
                    if e.kind() != std::io::ErrorKind::AlreadyExists { return Err(e.into()); }
                }
            }
            dir = directory_at(&dir, name)?;
        }
        Ok(dir)
    }

    fn validate_parent(&self, components: &[String], parent: &File) -> Result<()> {
        self.validate_root()?;
        if !same_file(parent, &self.parent(components, false)?)? {
            return Err(invalid("project directory changed during document access"));
        }
        Ok(())
    }

    fn validate_root(&self) -> Result<()> {
        let current = Self::open(&self.root_path)?;
        if !same_file(&self.root, &current.root)? {
            return Err(invalid("project directory changed during document access"));
        }
        Ok(())
    }

    fn read_at(&self, rel: &str, internal: bool) -> Result<String> {
        let components = parts(rel, internal)?;
        let parent = self.parent(&components, false)?;
        self.validate_parent(&components, &parent)?;
        read_file(open_at(&parent, components.last().unwrap(), libc::O_RDONLY | libc::O_NONBLOCK, 0)?, if internal { INTERNAL_LIMIT } else { MAX_DOCUMENT_BYTES })
    }

    pub fn read(&self, rel: &str) -> Result<String> { self.read_at(rel, false) }

    /// Create an empty file in existing directories, refusing overwrite/symlinks.
    pub fn create(&self, rel: &str) -> Result<()> { self.create_at(rel, "", false) }

    fn create_at(&self, rel: &str, text: &str, internal: bool) -> Result<()> {
        let components = parts(rel, internal)?;
        let parent = self.parent(&components, internal)?;
        self.validate_parent(&components, &parent)?;
        let mut file = open_at(&parent, components.last().unwrap(), libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL, 0o600)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        parent.sync_all()?;
        Ok(())
    }

    /// Save only if the document still has `expected_hash`; refuse stale bytes.
    pub fn save(&self, rel: &str, expected_hash: &str, text: &str) -> Result<()> {
        if text.len() > MAX_DOCUMENT_BYTES { return Err(invalid("document exceeds 1 MiB")); }
        self.replace(rel, expected_hash, text, false)
    }

    fn replace(&self, rel: &str, expected_hash: &str, text: &str, internal: bool) -> Result<()> {
        let components = parts(rel, internal)?;
        let parent = self.parent(&components, false)?;
        self.validate_parent(&components, &parent)?;
        let leaf = components.last().unwrap();
        let original = open_at(&parent, leaf, libc::O_RDONLY | libc::O_NONBLOCK, 0)?;
        let permissions = original.metadata()?.permissions();
        let limit = if internal { INTERNAL_LIMIT } else { MAX_DOCUMENT_BYTES };
        if crate::provider::stable_hash(read_file(original, limit)?.as_bytes()) != expected_hash {
            return Err(invalid("document changed externally; reload before saving"));
        }
        let temp = format!(".projectos-save-{}", uuid::Uuid::new_v4());
        let mut file = open_at(&parent, &temp, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL, 0o600)?;
        let result = (|| {
            file.write_all(text.as_bytes())?;
            file.set_permissions(permissions)?;
            file.sync_all()?;
            self.validate_parent(&components, &parent)?;
            let current = read_file(open_at(&parent, leaf, libc::O_RDONLY | libc::O_NONBLOCK, 0)?, limit)?;
            if crate::provider::stable_hash(current.as_bytes()) != expected_hash { return Err(invalid("document changed externally; reload before saving")); }
            let temp_c = cstring(&temp)?;
            let leaf_c = cstring(leaf)?;
            if unsafe { libc::renameat(parent.as_raw_fd(), temp_c.as_ptr(), parent.as_raw_fd(), leaf_c.as_ptr()) } < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            parent.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let temp_c = cstring(&temp)?;
            unsafe { libc::unlinkat(parent.as_raw_fd(), temp_c.as_ptr(), 0); }
        }
        result
    }

    pub(crate) fn read_internal(&self, rel: &str) -> Result<Option<String>> {
        match self.read_at(rel, true) {
            Ok(text) => Ok(Some(text)),
            Err(KernelError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Read UTF-8 TOML metadata relative to `.projectos`, without following links.
    pub fn read_metadata(&self, rel: &str) -> Result<Option<String>> {
        self.read_internal(&metadata_path(rel)?)
    }

    /// Atomically write TOML metadata, creating its descriptor-scoped directories.
    pub fn write_metadata(&self, rel: &str, text: &str) -> Result<()> {
        self.write_internal(&metadata_path(rel)?, text)
    }

    /// Read and verify a content-addressed before-image for revision restore.
    pub fn read_revision_blob(&self, hash: &str) -> Result<Vec<u8>> {
        if hash.len() != 16 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid("invalid revision content hash"));
        }
        let text = self.read_internal(&format!(".projectos/objects/{hash}"))?.ok_or_else(|| invalid("revision before-image is missing"))?;
        if crate::provider::stable_hash(text.as_bytes()) != hash { return Err(invalid("revision before-image is corrupt")); }
        Ok(text.into_bytes())
    }

    pub(crate) fn write_internal(&self, rel: &str, text: &str) -> Result<()> {
        if text.len() > INTERNAL_LIMIT { return Err(invalid("sidecar exceeds size limit")); }
        match self.read_internal(rel)? {
            Some(before) => self.replace(rel, &crate::provider::stable_hash(before.as_bytes()), text, true),
            None => self.create_at(rel, text, true),
        }
    }

    pub(crate) fn preserve_before_image(&self, hash: &str, text: &str) -> Result<()> {
        if hash != crate::provider::stable_hash(text.as_bytes()) { return Err(invalid("before-image hash mismatch")); }
        let rel = format!(".projectos/objects/{hash}");
        if let Some(existing) = self.read_internal(&rel)? {
            if existing != text { return Err(invalid("before-image object collision or corruption")); }
            return Ok(());
        }
        self.create_at(&rel, text, true)
    }

    /// Bounded deterministic discovery; symlinks, generated and hidden dirs skip.
    pub fn list_text_files(&self) -> Result<Vec<String>> {
        let current = Self::open(&self.root_path)?;
        if !same_file(&current.root, &self.root)? { return Err(invalid("project directory changed")); }
        let mut files = Vec::new();
        let mut visited = 0;
        self.walk(&self.root, "", 0, &mut visited, &mut files)?;
        files.sort();
        Ok(files)
    }

    fn walk(&self, dir: &File, prefix: &str, depth: usize, visited: &mut usize, files: &mut Vec<String>) -> Result<()> {
        if depth > 32 || *visited >= MAX_ENTRIES || files.len() >= MAX_FILES { return Ok(()); }
        // fdopendir takes ownership; open a separate directory description so its
        // seek offset does not interfere with the pinned root or recursive fds.
        let stream_file = directory_at(dir, ".")?;
        use std::os::fd::IntoRawFd;
        let fd = stream_file.into_raw_fd();
        let stream = unsafe { libc::fdopendir(fd) };
        if stream.is_null() { unsafe { libc::close(fd); } return Err(std::io::Error::last_os_error().into()); }
        struct Stream(*mut libc::DIR);
        impl Drop for Stream { fn drop(&mut self) { unsafe { libc::closedir(self.0); } } }
        let _guard = Stream(stream);
        let mut names = Vec::new();
        loop {
            let entry = unsafe { libc::readdir(stream) };
            if entry.is_null() { break; }
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_str().ok().map(str::to_string);
            *visited += 1;
            if let Some(name) = name { if !name.starts_with('.') { names.push(name); } }
            if *visited >= MAX_ENTRIES { break; }
        }
        names.sort();
        for name in names {
            if files.len() >= MAX_FILES { break; }
            if matches!(name.as_str(), "target" | "node_modules" | "vendor") { continue; }
            let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            // O_NOFOLLOW denies both directory and file symlinks; inaccessible
            // entries do not make the entire project browser fail.
            if let Ok(child) = directory_at(dir, &name) {
                self.walk(&child, &rel, depth + 1, visited, files)?;
            } else if text_extension(&name) {
                if let Ok(file) = open_at(dir, &name, libc::O_RDONLY | libc::O_NONBLOCK, 0) {
                    if read_file(file, MAX_DOCUMENT_BYTES).is_ok() { files.push(rel); }
                }
            }
        }
        Ok(())
    }
}

fn metadata_path(rel: &str) -> Result<String> {
    // Only normal TOML descendants are exposed, never document paths or blobs.
    parts(rel, true)?;
    if rel.starts_with(".projectos") || Path::new(rel).extension().and_then(|e| e.to_str()) != Some("toml") {
        return Err(invalid("metadata path must be relative to .projectos and end in .toml"));
    }
    Ok(format!(".projectos/{rel}"))
}
