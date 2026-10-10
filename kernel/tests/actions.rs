//! Tests for the typed selection actions (M2 §A).
//!
//! Uses a small real text file + an [`crate::selection::anchor`] over a
//! sub-range,and the deterministic [`MockProvider`] so every expectation is
//! reproducible offline.

use projectos_kernel::actions::{ActionOutcome, OutputShape, run, SelectionAction};
use projectos_kernel::provider::MockProvider;
use projectos_kernel::proposals::ProposalKind;
use tempfile::tempdir;

/// Selection text = bytes 0..19 of [`TEXT`] ("The quick brown fox").
const TEXT: &str = "The quick brown fox jumps over the lazy dog. End.";

fn setup() -> (tempfile::TempDir, projectos_kernel::selection::DocAnchor) {
    let dir = tempdir().unwrap();
    let rel = "doc.txt";
    std::fs::write(dir.path().join(rel), TEXT).unwrap();
    let a = projectos_kernel::selection::anchor(dir.path(), rel, 0, 19).unwrap();
    (dir, a)
}

fn run_action(dir: &tempfile::TempDir, a: &projectos_kernel::selection::DocAnchor, act: SelectionAction) -> ActionOutcome {
    let provider = MockProvider::new();
    run(dir.path(), a, act, &provider, "gpt-neo.test").unwrap()
}

#[test]
fn contracts_are_frozen() {
    assert_eq!("Rephrase", SelectionAction::Rephrase.contract().id);
    assert_eq!("Explain", SelectionAction::Explain.contract().id);
    assert_eq!("Complete", SelectionAction::Complete.contract().id);
    assert_eq!("FactCheck", SelectionAction::FactCheck.contract().id);

    assert_eq!(OutputShape::Replacement, SelectionAction::Rephrase.contract().output);
    assert!(!SelectionAction::Rephrase.contract().read_only);
    assert_eq!(OutputShape::Annotation, SelectionAction::Explain.contract().output);
    assert!(SelectionAction::Explain.contract().read_only);
    assert_eq!(OutputShape::Replacement, SelectionAction::Complete.contract().output);
    assert!(!SelectionAction::Complete.contract().read_only);
    assert_eq!(OutputShape::Annotation, SelectionAction::FactCheck.contract().output);
    assert!(SelectionAction::FactCheck.contract().read_only);
}

#[test]
fn rephrase_yields_replacement_proposal_with_original_selection() {
    let (dir, a) = setup();
    let out = run_action(&dir, &a, SelectionAction::Rephrase);
    assert_eq!(out.action, SelectionAction::Rephrase);
    assert_eq!(out.proposal.kind, ProposalKind::Replacement);
    assert_eq!(out.proposal.original, "The quick brown fox");
    assert_eq!(out.proposal.proposed, "[mock:gpt-neo.test] Rephrase::The quick brown fox");
    assert_eq!(out.proposal.before_hash, a.doc_hash);
}

#[test]
fn complete_yields_replacement() {
    let (dir, a) = setup();
    let out = run_action(&dir, &a, SelectionAction::Complete);
    assert_eq!(out.proposal.kind, ProposalKind::Replacement);
}

#[test]
fn annotation_actions_yield_annotation_proposal() {
    let (dir, a) = setup();
    let out = run_action(&dir, &a, SelectionAction::Explain);
    assert_eq!(out.proposal.kind, ProposalKind::Annotation);
    assert_eq!(out.proposal.original, "The quick brown fox");

    let (dir2, a2) = setup();
    let out2 = run_action(&dir2, &a2, SelectionAction::FactCheck);
    assert_eq!(out2.proposal.kind, ProposalKind::Annotation);
}

#[test]
fn stale_anchor_is_refused() {
    let (dir, a) = setup();
    // Overwrite the file with different bytes: the doc hash no longer matches,
    // so the anchor is stale and the action must be refused-with a legible reason.

    std::fs::write(dir.path().join("doc.txt"), "COMPLETELY DIFFERENT CONTENT").unwrap();
    let provider = MockProvider::new();
    let err = run(dir.path(), &a, SelectionAction::Rephrase,&provider, "m").unwrap_err();
    match err {
        projectos_kernel::error::KernelError::InvalidSidecar(m) => assert!(
            m.starts_with("stale selection:"),
            "expected stale selection, got {m:?}"
        ),
        other => panic!("expected InvalidSidecar, got {other:?}"),
    }
}

#[test]
fn deleted_file_is_stale() {
    let (dir, a) = setup();
    std::fs::remove_file(dir.path().join("doc.txt")).unwrap();
    let provider = MockProvider::new();
    let err = run(dir.path(), &a, SelectionAction::Explain, &provider, "m").unwrap_err();
    assert!(matches!(err, projectos_kernel::error::KernelError::InvalidSidecar(_)));
}

#[test]
fn outcome_records_route_resolved_model_and_usage() {
    let (dir, a) = setup();
    let out = run_action(&dir, &a, SelectionAction::FactCheck);
    let model = "gpt-neo.test";
    assert_eq!(out.route, model);
    assert_eq!(out.resolved_model, model);
    assert_eq!(out.usage_tokens, Some(("FactCheck::The quick brown fox").len() as u64));
}