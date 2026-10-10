//! Typed selection actions (M2 child A) — SCAFFOLD STUB.
//!
//! Frozen contract: `kernel/INTERFACES-M2.md` §A. The owner of this file
//! replaces every `unimplemented!()` body with the real behaviour + tests in
//! `tests/actions.rs`. Do not change the public signatures.

use crate::error::Result;
use std::path::Path;

/// The four MVP selected-text actions (brief §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionAction {
    Rephrase,
    Explain,
    Complete,
    FactCheck,
}

/// The shape of a proposal an action produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputShape {
    Replacement,
    Annotation,
}

/// The declared contract of an action.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionContract {
    pub id: &'static str,
    pub output: OutputShape,
    pub read_only: bool,
}

impl SelectionAction {
    pub fn contract(self) -> ActionContract {
        unimplemented!()
    }
}

/// The result of running an action through a provider.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionOutcome {
    pub action: SelectionAction,
    pub proposal: crate::proposals::Proposal,
    pub route: String,
    pub resolved_model: String,
    pub usage_tokens: Option<u64>,
}

/// Run `action` over the anchored selection, via `provider`.
pub fn run(
    folder: &Path,
    a: &crate::selection::DocAnchor,
    action: SelectionAction,
    provider: &dyn crate::provider::Provider,
    model: &str,
) -> Result<ActionOutcome> {
    let _ = (folder, a, action, provider, model);
    unimplemented!()
}
