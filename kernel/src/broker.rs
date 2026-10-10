//! Capability broker — descriptor-relative path resolution, grants, redacted receipts.
//!
//! This module is child B's slice per `INTERFACES.md`. It implements the frozen
//! public API: `Capability`, `CapabilitySet`, `BrokerRequest`, `BrokerOutcome`,
//! `Receipt`, and `Broker`.
//!
//! ## Design (port of the gate-c descriptor-relative pattern)
//!
//! Every path is resolved **relative to the root**, component by component:
//! 1. Absolute paths are rejected (`/` or `C:\`).
//! 2. `..` components are rejected.
//! 3. After canonicalizing the realpath, the result must stay inside the root;
//!    a symlink that escapes is rejected.
//!
//! A missing grant is a **denial** (`Ok(Denied(_))`), never an error. A denied
//! request appends a redacted `Receipt` and returns `BrokerOutcome::Denied`.
//!
//! ## Confinement note
//!
//! This is a *broker*, not a confinement boundary. It refuses to read anything
//! outside the root by contract (relative resolution + realpath check), but the
//! trait boundary is the proven surface; the WASM guest adapter is the second
//! confinement-capable implementation (deferred to Gate C).

use crate::error::{KernelError, Result};
use std::path::{Path, PathBuf};

/// A capability granted up front: read or write a relative path.
#[derive(Debug, Clone, PartialEq)]
pub enum Capability {
    ReadFile(String),
    WriteFile(String),
}

/// The set of capabilities granted to a worker.
#[derive(Debug, Clone)]
pub struct CapabilitySet(Vec<Capability>);

impl CapabilitySet {
    pub fn new() -> Self {
        CapabilitySet(Vec::new())
    }

    pub fn from_vec(v: Vec<Capability>) -> Self {
        CapabilitySet(v)
    }

    pub fn push(&mut self, c: Capability) {
        self.0.push(c);
    }

    /// Returns true if a matching `ReadFile` capability exists for `path`.
    pub fn has_read(&self, path: &str) -> bool {
        self.0
            .iter()
            .any(|c| matches!(c, Capability::ReadFile(p) if p == path))
    }

    /// Returns true if a matching `WriteFile` capability exists for `path`.
    pub fn has_write(&self, path: &str) -> bool {
        self.0
            .iter()
            .any(|c| matches!(c, Capability::WriteFile(p) if p == path))
    }
}

impl Default for CapabilitySet {
    fn default() -> Self {
        Self::new()
    }
}

/// A request to the broker: read or write a file by relative path.
#[derive(Debug, Clone)]
pub enum BrokerRequest {
    ReadFile(String),
    WriteFile { path: String, bytes: Vec<u8> },
}

/// The outcome of a brokered request.
#[derive(Debug, PartialEq)]
pub enum BrokerOutcome {
    Read(Vec<u8>),
    Written,
    /// The path is a short, redacted denial reason (no absolute paths, no bytes).
    Denied(String),
}

/// A redacted audit receipt appended on every denial.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Receipt {
    pub op: String,
    pub rel_path: String,
    pub outcome: String,
}

/// The capability broker: holds a root directory and the granted capability set,
/// and resolves every request relative to that root.
pub struct Broker {
    root: PathBuf,
    grants: CapabilitySet,
    receipts: Vec<Receipt>,
}

impl Broker {
    pub fn new(root: PathBuf, grants: CapabilitySet) -> Self {
        Broker {
            root,
            grants,
            receipts: Vec::new(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn grants(&self) -> &CapabilitySet {
        &self.grants
    }

    pub fn receipts(&self) -> &[Receipt] {
        &self.receipts
    }

    /// Resolve a relative path against the root, enforcing the gate-c
    /// descriptor-relative rules: reject absolute paths, `..` components, and
    /// any realpath that escapes the root (symlink escape). Returns the
    /// canonicalized realpath confirmed to be inside the root.
    fn resolve(&self, rel: &str) -> std::result::Result<PathBuf, String> {
        let p = Path::new(rel);
        use std::path::Component;

        // Rule 1: reject absolute paths / drive prefixes / root components.
        if p.is_absolute() {
            return Err("absolute path denied".to_string());
        }
        for comp in p.components() {
            match comp {
                Component::CurDir => continue,
                Component::ParentDir => {
                    return Err("parent traversal denied".to_string())
                }
                Component::Prefix(_) => {
                    return Err("prefixed path denied".to_string())
                }
                Component::RootDir => {
                    return Err("absolute path denied".to_string())
                }
                Component::Normal(_) => continue,
            }
        }

        let root_canon = std::fs::canonicalize(&self.root)
            .map_err(|_| "root not canonicalizable".to_string())?;
        let candidate = self.root.join(p);

        // Canonicalize the candidate. If the file does not exist yet (a write
        // target), canonicalize the parent directory and re-attach the final
        // component so symlink-escape on intermediate dirs is still detected.
        let candidate_canon = match std::fs::canonicalize(&candidate) {
            Ok(c) => c,
            Err(_) => {
                let parent = match candidate.parent() {
                    Some(parent) => parent,
                    None => return Err("path unresolvable".to_string()),
                };
                let parent_canon = std::fs::canonicalize(parent)
                    .map_err(|_| "path unresolvable".to_string())?;
                let tail = p
                    .file_name()
                    .map(|n| Path::new(n).to_path_buf())
                    .unwrap_or_default();
                if tail.as_os_str().is_empty() {
                    parent_canon
                } else {
                    parent_canon.join(tail)
                }
            }
        };

        // Rule 3: the canonical realpath must be the root or live inside it.
        // `starts_with` is a component-wise check, avoiding the "/a/b" vs
        // "/a/bc" string-prefix trap.
        if candidate_canon != root_canon && !candidate_canon.starts_with(&root_canon) {
            return Err("symlink escape denied".to_string());
        }
        Ok(candidate_canon)
    }

    /// Writes never follow a symlink. `canonicalize` cannot resolve a dangling
    /// (or chained) symlink in the FINAL component: it fails, and the fallback
    /// that canonicalizes the parent and re-attaches the tail name yields an
    /// in-root-looking path, so the subsequent open would follow the link and
    /// create or overwrite a file OUTSIDE the root. Refusing the write whenever
    /// the final component is a symlink is the only way to prove the ultimate
    /// target stays inside. Intermediate symlinked directories are still caught
    /// by canonicalize + `starts_with` in `resolve`. This rule is deliberately
    /// conservative: a legitimate symlink kept entirely inside the root is also
    /// refused as a write target (a trade-off accepted for confinement).
    fn final_component_is_symlink(&self, candidate: &PathBuf) -> bool {
        match std::fs::symlink_metadata(candidate) {
            Ok(stat) => stat.is_symlink(),
            Err(_) => false, // nonexistent or IO error: not an existing symlink
        }
    }

    /// Produce the `rel_path` value stored in a receipt. Receipts must never
    /// contain an absolute path (redaction). A leading separator (and a Windows
    /// drive letter) is stripped so an absolute attempt is recorded in relative
    /// form rather than leaked verbatim. No file bytes are ever placed in a
    /// receipt; `outcome` holds only a short reason string.
    fn redact_rel_path(rel: &str) -> String {
        let mut s = rel.trim_start_matches('/').to_string();
        if s.len() >= 2 && s.as_bytes()[1] == b':' {
            s = s[2..]
                .trim_start_matches('/')
                .trim_start_matches('\\')
                .to_string();
        }
        s
    }

    /// Append a redacted receipt. `rel_path` is the requested path, redacted to
    /// never be absolute; `reason` is a short human-readable denial string with
    /// no file bytes.
    fn append_receipt(&mut self, op: &str, rel_path: &str, reason: &str) {
        self.receipts.push(Receipt {
            op: op.to_string(),
            rel_path: Self::redact_rel_path(rel_path),
            outcome: reason.to_string(),
        });
    }

    /// Handle a brokered request. A missing grant or a path-resolution failure
    /// is a denial (`Ok(Denied(..))`) with a redacted receipt appended; genuine
    /// I/O failures (e.g. read of a granted-but-missing file) surface as
    /// `KernelError::Io`.
    pub fn request(&mut self, req: BrokerRequest) -> Result<BrokerOutcome> {
        match req {
            BrokerRequest::ReadFile(path) => {
                // Check grant first (missing grant => denial, not error).
                if !self.grants.has_read(&path) {
                    let reason = "read not granted".to_string();
                    self.append_receipt("read", &path, &reason);
                    return Ok(BrokerOutcome::Denied(reason));
                }
                // Resolve path safely.
                let resolved = match self.resolve(&path) {
                    Ok(r) => r,
                    Err(reason) => {
                        self.append_receipt("read", &path, &reason);
                        return Ok(BrokerOutcome::Denied(reason));
                    }
                };
                // Read the bytes.
                match std::fs::read(&resolved) {
                    Ok(bytes) => Ok(BrokerOutcome::Read(bytes)),
                    Err(e) => Err(KernelError::Io(e)),
                }
            }
            BrokerRequest::WriteFile { path, bytes } => {
                if !self.grants.has_write(&path) {
                    let reason = "write not granted".to_string();
                    self.append_receipt("write", &path, &reason);
                    return Ok(BrokerOutcome::Denied(reason));
                }
                // Confinement: never write through a symlink. If the final
                // component is a symlink, its ultimate target cannot be proven
                // to stay inside the root, so deny the write outright.
                let candidate = self.root.join(Path::new(&path));
                if self.final_component_is_symlink(&candidate) {
                    let reason = "symlink target denied".to_string();
                    self.append_receipt("write", &path, &reason);
                    return Ok(BrokerOutcome::Denied(reason));
                }
                let resolved = match self.resolve(&path) {
                    Ok(r) => r,
                    Err(reason) => {
                        self.append_receipt("write", &path, &reason);
                        return Ok(BrokerOutcome::Denied(reason));
                    }
                };
                // Write the bytes.
                match std::fs::write(&resolved, bytes) {
                    Ok(()) => Ok(BrokerOutcome::Written),
                    Err(e) => Err(KernelError::Io(e)),
                }
            }
        }
    }
}
