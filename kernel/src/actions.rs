//! Typed selection actions (M2 child A).
//!
//! Frozen contract: `kernel/INTERFACES-M2.md` §A. An [`ActionContract`]
//! declares the shape and write-behaviour of each [`SelectionAction`]: the
//! mutating `Rephrase`/`Complete` produce a `Replacement` (a proposed write
//! back to the artifact), while the read-only `Explain`/`FactCheck` produce an
//! `Annotation`. [`run`] resolves the anchor first (a stale selection is
//! refused — no silent action on changed bytes), reads the selected text,
//! assembles a [`ProviderRequest`] carrying the action id + selected text,
//! asks the provider, and records the route/resolved-model/usage in the
//! [`ActionOutcome`].

use crate::error::{KernelError, Result};
use crate::selection::{AnchorState, DocAnchor};
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
    /// Declare the action's contract: id, output shape, and whether its
    /// proposal rewrites the artifact (`read_only = false`) or only annotates
    /// it (`read_only = true`).
    pub fn contract(self) -> ActionContract {
        match self {
            SelectionAction::Rephrase => ActionContract {
                id: "Rephrase",
                output: OutputShape::Replacement,
                read_only: false,
            },
            SelectionAction::Explain => ActionContract {
                id: "Explain",
                output: OutputShape::Annotation,
                read_only: true,
            },
            SelectionAction::Complete => ActionContract {
                id: "Complete",
                output: OutputShape::Replacement,
                read_only: false,
            },
            SelectionAction::FactCheck => ActionContract {
                id: "FactCheck",
                output: OutputShape::Annotation,
                read_only: true,
            },
        }
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

fn read_selection(folder: &Path, a: &DocAnchor) -> Result<String> {
    let bytes = std::fs::read(folder.join(&a.rel_path))?;
    if a.byte_start > a.byte_end
        || a.byte_end > bytes.len()
    {
        return Err(KernelError::InvalidSidecar(format!(
            "anchor range {}..{} out of bounds (len {})",
            a.byte_start,
            a.byte_end,
            bytes.len()
        )));
    }
    Ok(String::from_utf8_lossy(&bytes[a.byte_start..a.byte_end]).into_owned())
}

/// Run `action` over the anchored selection, via `provider`.
///
/// First resolves the anchor: a `Stale` selection is refused, never acted
/// on. Otherwise reads the selected bytes, asks the provider with the
/// prompt `"{id}::{selected}"`, and builds a [`Proposal`] whose `original` is
/// the selected text, `proposed` is the provider's text, and `before_hash` is
/// the anchor's `doc_hash`.
pub fn run(
    folder: &Path,
    a: &crate::selection::DocAnchor,
    action: SelectionAction,
    provider: &dyn crate::provider::Provider,
    model: &str,
) -> Result<ActionOutcome> {
    match crate::selection::resolve(folder, a)? {
        AnchorState::Fresh => {}
        AnchorState::Stale(reason) => {
            return Err(KernelError::InvalidSidecar(format!(
                "stale selection: {reason}"
            )));
        }
    }

    let contract = action.contract();
    let selected = read_selection(folder, a)?;
    let req = crate::provider::ProviderRequest {
        model: model.to_string(),
        prompt: format!("{}::{selected}", contract.id),
    };
    let resp = provider.complete(&req)?;

    let kind = match contract.output {
        OutputShape::Replacement => crate::proposals::ProposalKind::Replacement,
        OutputShape::Annotation => crate::proposals::ProposalKind::Annotation,
    };
    let proposal = crate::proposals::Proposal::new(
        &a.rel_path,
        kind,
        &selected,
        &resp.text,
        &a.doc_hash,
    );

    Ok(ActionOutcome {
        action,
        proposal,
        route: model.to_string(),
        resolved_model: resp.resolved_model,
        usage_tokens: resp.usage_tokens,
    })
}