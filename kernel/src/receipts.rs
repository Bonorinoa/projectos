//! Redacted receipt writer (child R per `INTERFACES-M1b.md`).
//!
//! Durable, redacted receipt facts (brief §15: the project graph holds redacted
//! receipts; the sensitive trace vault stays out of scope this pass). A
//! `RedactedReceipt` is a small, immutable fact: what an action/recipe run did,
//! where (a project-relative path only), what happened, and optional hash /
//! route / cost metadata.
//!
//! ## Redaction invariants (never break these)
//!
//! - **No absolute paths ever reach disk.** Every `rel_path` is passed through
//!   [`redact_rel_path`] (same rule as `broker::Broker`): a leading `/` and a
//!   Windows drive prefix are stripped so an absolute attempt is archived in
//!   relative form, never verbatim.
//! - **No file bytes and no full-length opaque secrets ever reach disk.**
//!   Receipt facts carry digests (`content_hash`) and short reason strings, not
//!   content. The broker's `Receipt` never carries bytes to begin with, and
//!   `ingest_broker_receipts` re-redacts the path defensively.
//!
//! Persisted representation: `<project>/.projectos/receipts.toml`, rewritten
//! atomically (tmp + rename) on every change so no partial file is ever
//! observable and no `.tmp` is left behind.
//!
//! ## Receipt id derivation (deterministic, documented)
//!
//! `id` is a **FNV-1a 64-bit** hash of the stable string
//! `"{at}\u{1}{op}\u{1}{rel_path}"` (fields joined by the unit separator
//! `0x1F`), rendered as 16 lowercase hex digits. It depends only on the receipt
//! facts — never on insertion order, wall-clock time, or the file on disk — so
//! two identical runs produce identical ids. It is deliberately NOT a
//! cryptographic digest (the brief / interface call for "sha256-less"): it is a
//! stable identity determinism guarantee, not a content-authenticity claim.

use crate::error::{KernelError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A durable, redacted receipt fact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RedactedReceipt {
    /// Deterministic identity: FNV-1a-64 of `at \x1F op \x1F rel_path` (see module docs).
    pub id: String,
    pub at: u64,
    pub op: String,
    /// Redacted project-relative path. Never absolute.
    pub rel_path: String,
    pub outcome: String,
    /// A content digest provided by the caller (a hash, never raw bytes).
    pub content_hash: Option<String>,
    pub route: Option<String>,
    pub cost: Option<String>,
}

/// The on-disk TOML shape: a single `receipts` array.
#[derive(Debug, Serialize, Deserialize)]
struct ReceiptsFile {
    receipts: Vec<RedactedReceipt>,
}

/// Durable redacted receipt writer backed by `<project>/.projectos/receipts.toml`.
pub struct ReceiptWriter {
    path: PathBuf,
    receipts: Vec<RedactedReceipt>,
}

impl ReceiptWriter {
    /// Open (creating the `.projectos` sidecar dir and, if present, loading)
    /// the receipt store for `folder`.
    pub fn new(folder: &Path) -> Result<Self> {
        let sidecar = folder.join(".projectos");
        std::fs::create_dir_all(&sidecar)?;
        let path = sidecar.join("receipts.toml");
        let receipts = if path.exists() {
            let text = std::fs::read_to_string(&path)?;
            let file: ReceiptsFile = toml::from_str(&text).map_err(|e| {
                KernelError::InvalidSidecar(format!("receipts.toml is corrupt: {e}"))
            })?;
            file.receipts
        } else {
            Vec::new()
        };
        Ok(ReceiptWriter { path, receipts })
    }

    /// Append a redacted receipt. `rel_path` is redacted (absolute → relative)
    /// and `id` is recomputed deterministically from `(at, op, rel_path)`
    /// before the store is rewritten atomically.
    pub fn append(&mut self, r: RedactedReceipt) -> Result<()> {
        let rel = redact_rel_path(&r.rel_path);
        let mut stored = r;
        stored.rel_path = rel;
        stored.id = stable_id(stored.at, &stored.op, &stored.rel_path);
        self.receipts.push(stored);
        self.write_atomic()
    }

    /// The in-memory receipts, in append order.
    pub fn receipts(&self) -> &[RedactedReceipt] {
        &self.receipts
    }

    /// Ingest broker denials as durable redacted facts and return how many
    /// facts were written. Every broker `Receipt` becomes one `RedactedReceipt`
    /// (re-redacting the path defensively; no file bytes, no absolute paths).
    pub fn ingest_broker_receipts(
        folder: &Path,
        receipts: &[crate::broker::Receipt],
        at: u64,
    ) -> Result<usize> {
        let mut writer = ReceiptWriter::new(folder)?;
        for br in receipts {
            let rel = redact_rel_path(&br.rel_path);
            writer.receipts.push(RedactedReceipt {
                id: stable_id(at, &br.op, &rel),
                at,
                op: br.op.clone(),
                rel_path: rel,
                outcome: br.outcome.clone(),
                content_hash: None,
                route: None,
                cost: None,
            });
        }
        writer.write_atomic()?;
        Ok(receipts.len())
    }

    /// The redaction rule, exposed publicly and matching `broker::Broker`:
    /// strip a leading `/` and a Windows drive prefix so a path is never
    /// stored as absolute.
    pub fn redact_rel_path(rel: &str) -> String {
        redact_rel_path(rel)
    }

    /// Atomically rewrite the whole store: write to a sibling `.tmp` then
    /// rename over the real file, so a crash never leaves a partial receipt
    /// file visible and no `.tmp` remains afterwards.
    fn write_atomic(&self) -> Result<()> {
        let file = ReceiptsFile {
            receipts: self.receipts.clone(),
        };
        let text = toml::to_string(&file)
            .map_err(|e| KernelError::InvalidSidecar(format!("serialize receipts: {e}")))?;
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

/// Strip a leading `/` and a Windows drive prefix (e.g. `C:\` or `C:/`) from a
/// path so it can never be stored as absolute. Mirrors `broker::Broker`'s rule;
/// the writer applies it defensively in addition to the broker.
fn redact_rel_path(rel: &str) -> String {
    let mut s = rel.trim_start_matches('/').to_string();
    if s.len() >= 2 && s.as_bytes()[1] == b':' {
        // Drop the drive letter + ":" and any following separators.
        s = s[2..]
            .trim_start_matches('/')
            .trim_start_matches('\\')
            .to_string();
    }
    s
}

/// FNV-1a 64-bit stable hash of `at \x1F op \x1F rel_path` → 16 lowercase hex.
/// Deterministic across identical inputs and independent of file state.
fn stable_id(at: u64, op: &str, rel: &str) -> String {
    let mut state: u64 = 0xcbf2_9ce4_8422_2325;
    let joined = format!("{at}\u{1f}{op}\u{1f}{rel}");
    for b in joined.bytes() {
        state ^= u64::from(b);
        state = state.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{state:016x}")
}
