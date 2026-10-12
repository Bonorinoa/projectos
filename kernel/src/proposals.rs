//! Proposal → diff → approval → revision loop (M2 core, `m2.diff`).
//!
//! A [`Proposal`] is a typed, reviewable suggestion (a replacement or an
//! annotation). Nothing is inserted silently: a proposal is applied only through
//! [`apply`], which **re-resolves the anchor** (a stale selection is refused),
//! keeps the **before-image** content-addressed under `.projectos/objects/`,
//! replaces the file atomically (tmp + rename), and records a journal `Mutated`
//! entry, a redacted receipt, and a durable [`Revision`]. Rejection mutates
//! nothing and records a `rejected` receipt.

use crate::document::DocumentStore;
use crate::error::{KernelError, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Whether a proposal rewrites the artifact bytes or only annotates them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ProposalKind {
    Replacement,
    Annotation,
}

/// A typed, reviewable proposal produced by an action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub rel_path: String,
    pub kind: ProposalKind,
    pub original: String,
    pub proposed: String,
    /// Document hash the proposal was computed against.
    pub before_hash: String,
    /// Hash of the proposed content.
    pub after_hash: String,
}

/// A minimal line-level diff of a proposal.
#[derive(Debug, Clone, PartialEq)]
pub struct Diff {
    pub removed: Vec<String>,
    pub added: Vec<String>,
}

/// The user's decision on a proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Accept,
    Reject,
}

/// A durable record of an accepted revision (before/after content hashes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Revision {
    pub id: String,
    pub proposal_id: String,
    pub rel_path: String,
    pub before_hash: String,
    pub after_hash: String,
    pub at: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RevisionsFile {
    revisions: Vec<Revision>,
}

/// The durable revision timeline at `.projectos/revisions.toml`.
pub struct RevisionLog {
    store: DocumentStore,
    revisions: Vec<Revision>,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn lines(s: &str) -> Vec<String> {
    if s.is_empty() {
        Vec::new()
    } else {
        s.lines().map(|l| l.to_string()).collect()
    }
}

impl Proposal {
    /// Build a proposal, deriving `after_hash` and a deterministic `id`.
    pub fn new(rel_path: &str, kind: ProposalKind, original: &str, proposed: &str, before_hash: &str) -> Self {
        let after_hash = crate::provider::stable_hash(proposed.as_bytes());
        let id = crate::provider::stable_hash(
            format!("{rel_path}\u{1f}{original}\u{1f}{proposed}").as_bytes(),
        );
        Proposal {
            id,
            rel_path: rel_path.to_string(),
            kind,
            original: original.to_string(),
            proposed: proposed.to_string(),
            before_hash: before_hash.to_string(),
            after_hash,
        }
    }

    pub fn diff(&self) -> Diff {
        Diff {
            removed: lines(&self.original),
            added: lines(&self.proposed),
        }
    }
}

impl RevisionLog {
    pub fn new(folder: &Path) -> Result<Self> {
        let store = DocumentStore::open(folder)?;
        let revisions = if let Some(text) = store.read_metadata("revisions.toml")? {
            if text.trim().is_empty() {
                Vec::new()
            } else {
                let f: RevisionsFile = toml::from_str(&text).map_err(|e| {
                    KernelError::InvalidSidecar(format!("revisions.toml is corrupt: {e}"))
                })?;
                f.revisions
            }
        } else {
            Vec::new()
        };
        Ok(RevisionLog { store, revisions })
    }

    pub fn revisions(&self) -> &[Revision] {
        &self.revisions
    }

    fn append(&mut self, r: Revision) -> Result<()> {
        let mut revisions = self.revisions.clone();
        revisions.push(r);
        let f = RevisionsFile {
            revisions: revisions.clone(),
        };
        let text = toml::to_string(&f)
            .map_err(|e| KernelError::InvalidSidecar(format!("serialize revisions: {e}")))?;
        self.store.write_metadata("revisions.toml", &text)?;
        self.revisions = revisions;
        Ok(())
    }
}

/// Apply a decision to a proposal.
///
/// `Accept` re-resolves the anchor (**stale → error, never applied**), stores the
/// before-image content-addressed, replaces the anchored range atomically,
/// records journal + receipt + revision, and returns the `Revision`. `Reject`
/// mutates nothing, records a `rejected` receipt, and returns `None`.
pub fn apply(
    folder: &Path,
    anchor: &crate::selection::DocAnchor,
    p: &Proposal,
    d: Decision,
) -> Result<Option<Revision>> {
    let store = DocumentStore::open(folder)?;
    crate::document::validate_document_path(&p.rel_path)?;
    if p.rel_path != anchor.rel_path {
        return Err(KernelError::InvalidSidecar("proposal path does not match the anchor".into()));
    }
    let expected = Proposal::new(&p.rel_path, p.kind.clone(), &p.original, &p.proposed, &p.before_hash);
    if anchor.byte_start > anchor.byte_end
        || anchor.byte_end - anchor.byte_start != p.original.len()
        || p.before_hash != anchor.doc_hash
        || crate::provider::stable_hash(p.original.as_bytes()) != anchor.text_hash
        || p.after_hash != expected.after_hash || p.id != expected.id
    {
        return Err(KernelError::InvalidSidecar("malformed proposal or selection mismatch".into()));
    }
    // Rejection discards a native-owned proposal. It must remain possible after
    // an external edit, deletion, or replacement of the original document.
    if d == Decision::Reject {
        let mut receipts: ApplyReceipts = read_records(&store, "receipts.toml")?;
        let at = now();
        receipts.receipts.push(crate::receipts::RedactedReceipt {
            id: crate::provider::stable_hash(format!("{at}\u{1f}apply\u{1f}{}", p.rel_path).as_bytes()),
            at, op: "apply".into(), rel_path: p.rel_path.clone(), outcome: "rejected".into(),
            content_hash: Some(p.after_hash.clone()), route: None, cost: None,
        });
        store.write_metadata("receipts.toml", &serialize(&receipts)?)?;
        return Ok(None);
    }
    let text = match store.read(&anchor.rel_path) {
        Err(KernelError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(KernelError::InvalidSidecar("stale selection: document missing".into()));
        }
        result => result?,
    };
    if p.before_hash != anchor.doc_hash
        || crate::provider::stable_hash(text.as_bytes()) != anchor.doc_hash
    {
        return Err(KernelError::InvalidSidecar("stale selection: document changed or proposal hash mismatched".into()));
    }
    if anchor.byte_start > anchor.byte_end || anchor.byte_end > text.len()
        || !text.is_char_boundary(anchor.byte_start) || !text.is_char_boundary(anchor.byte_end)
    {
        return Err(KernelError::InvalidSidecar("anchor range out of bounds or not on UTF-8 boundaries".into()));
    }
    let selected = &text[anchor.byte_start..anchor.byte_end];
    if p.original != selected || crate::provider::stable_hash(selected.as_bytes()) != anchor.text_hash
    {
        return Err(KernelError::InvalidSidecar("malformed proposal or selection mismatch".into()));
    }

    // Parse all records before changing document bytes. The apply path uses
    // descriptor-scoped persistence, rather than the legacy path-based writers.
    let mut receipts: ApplyReceipts = read_records(&store, "receipts.toml")?;
    let mut journal: ApplyJournal = read_records(&store, "journal.toml")?;
    let mut revisions = RevisionLog::new(folder)?;
    let at = now();
    let (outcome, content_hash) = match p.kind {
        ProposalKind::Annotation => ("accepted-annotation", p.after_hash.clone()),
        ProposalKind::Replacement => ("accepted", String::new()),
    };
    let mut receipt = crate::receipts::RedactedReceipt {
        id: crate::provider::stable_hash(format!("{at}\u{1f}apply\u{1f}{}", p.rel_path).as_bytes()),
        at, op: "apply".into(), rel_path: p.rel_path.clone(), outcome: outcome.into(),
        content_hash: Some(content_hash), route: None, cost: None,
    };
    if p.kind == ProposalKind::Annotation {
        receipts.receipts.push(receipt);
        store.write_metadata("receipts.toml", &serialize(&receipts)?)?;
        // An annotation is linked evidence, never a replacement of draft bytes.
        // The application persists the annotation artifact before accepting it.
        return Ok(None);
    }

    let new_text = format!("{}{}{}", &text[..anchor.byte_start], p.proposed, &text[anchor.byte_end..]);
    let full_after_hash = crate::provider::stable_hash(new_text.as_bytes());
    let rev = Revision {
        id: uuid::Uuid::new_v4().to_string(),
        proposal_id: p.id.clone(), rel_path: p.rel_path.clone(),
        before_hash: p.before_hash.clone(), after_hash: full_after_hash.clone(), at,
    };
    receipt.content_hash = Some(full_after_hash);
    receipts.receipts.push(receipt);
    journal.records.push(crate::journal::Record::Mutated { object: p.rel_path.clone(), at });
    // Serialize before mutation as another preflight. The before-image survives
    // a crash even if a later audit-record persistence step cannot complete.
    let receipt_text = serialize(&receipts)?;
    let journal_text = serialize(&journal)?;
    store.preserve_before_image(&p.before_hash, &text)?;
    store.save(&p.rel_path, &p.before_hash, &new_text)?;
    store.write_metadata("journal.toml", &journal_text)?;
    store.write_metadata("receipts.toml", &receipt_text)?;
    revisions.append(rev.clone())?;
    Ok(Some(rev))
}

#[derive(Default, Serialize, Deserialize)]
struct ApplyReceipts { receipts: Vec<crate::receipts::RedactedReceipt> }
#[derive(Default, Serialize, Deserialize)]
struct ApplyJournal { records: Vec<crate::journal::Record> }

fn read_records<T: serde::de::DeserializeOwned + Default>(store: &DocumentStore, name: &str) -> Result<T> {
    match store.read_metadata(name)? {
        Some(text) if !text.trim().is_empty() => toml::from_str(&text)
            .map_err(|e| KernelError::InvalidSidecar(format!("corrupt {name}: {e}"))),
        _ => Ok(T::default()),
    }
}

fn serialize<T: Serialize>(value: &T) -> Result<String> {
    toml::to_string(value).map_err(|e| KernelError::InvalidSidecar(format!("cannot serialize audit record: {e}")))
}
