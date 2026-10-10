//! Document selection anchors (M2 core, `m2.editor`).
//!
//! A [`DocAnchor`] identifies a byte range inside a project-relative document
//! **and** records the hashes that were true when the selection was made, so a
//! later action can refuse a selection that no longer applies. A stale
//! selection must **fail safely** — never be applied to changed bytes.
//!
//! `rel_path` is relative and non-escaping (absolute / `..` → `InvalidSidecar`).
//! The range must be in bounds and on UTF-8 char boundaries. `resolve` re-hashes
//! the document and returns [`AnchorState::Stale`] (not an error) when the file
//! is missing or changed, so callers can report a legible reason.

use crate::error::{KernelError, Result};
use std::path::{Component, Path};

/// A byte-range selection anchor with freshness hashes.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DocAnchor {
    /// Project-relative document path (never absolute).
    pub rel_path: String,
    pub byte_start: usize,
    pub byte_end: usize,
    /// Hash of the selected bytes at anchor time.
    pub text_hash: String,
    /// Hash of the whole document at anchor time.
    pub doc_hash: String,
}

/// The freshness of a [`DocAnchor`] against the current document.
#[derive(Debug, Clone, PartialEq)]
pub enum AnchorState {
    Fresh,
    Stale(String),
}

fn hash(bytes: &[u8]) -> String {
    crate::provider::stable_hash(bytes)
}

/// Reject absolute or `..`-containing relative paths.
fn safe_rel(rel: &str) -> Result<()> {
    let p = Path::new(rel);
    if p.is_absolute() {
        return Err(KernelError::InvalidSidecar(format!("absolute path: {rel}")));
    }
    for c in p.components() {
        match c {
            Component::ParentDir => {
                return Err(KernelError::InvalidSidecar(format!("parent traversal: {rel}")))
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err(KernelError::InvalidSidecar(format!("absolute path: {rel}")))
            }
            _ => {}
        }
    }
    Ok(())
}

/// Create an anchor for `[byte_start, byte_end)` inside `folder/rel_path`.
pub fn anchor(folder: &Path, rel_path: &str, byte_start: usize, byte_end: usize) -> Result<DocAnchor> {
    safe_rel(rel_path)?;
    let bytes = std::fs::read(folder.join(rel_path))?;
    if byte_start > byte_end || byte_end > bytes.len() {
        return Err(KernelError::InvalidSidecar(format!(
            "range {byte_start}..{byte_end} out of bounds (len {})",
            bytes.len()
        )));
    }
    if let Ok(text) = std::str::from_utf8(&bytes) {
        if !text.is_char_boundary(byte_start) || !text.is_char_boundary(byte_end) {
            return Err(KernelError::InvalidSidecar(format!(
                "range {byte_start}..{byte_end} is not on char boundaries"
            )));
        }
    }
    Ok(DocAnchor {
        rel_path: rel_path.to_string(),
        byte_start,
        byte_end,
        text_hash: hash(&bytes[byte_start..byte_end]),
        doc_hash: hash(&bytes),
    })
}

/// Re-check an anchor against the current document. `Fresh` iff the document
/// hash matches; a missing or changed document is `Stale` with a reason.
pub fn resolve(folder: &Path, a: &DocAnchor) -> Result<AnchorState> {
    safe_rel(&a.rel_path)?;
    let bytes = match std::fs::read(folder.join(&a.rel_path)) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AnchorState::Stale("document missing".to_string()))
        }
        Err(e) => return Err(e.into()),
    };
    if hash(&bytes) != a.doc_hash {
        return Ok(AnchorState::Stale("document changed".to_string()));
    }
    if a.byte_start > a.byte_end
        || a.byte_end > bytes.len()
        || hash(&bytes[a.byte_start..a.byte_end]) != a.text_hash
    {
        return Ok(AnchorState::Stale("selection changed".to_string()));
    }
    Ok(AnchorState::Fresh)
}
