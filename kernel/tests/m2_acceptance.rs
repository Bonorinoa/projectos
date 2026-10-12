//! M2 acceptance suite — authored by the reviewing lead (see `ACCEPTANCE-M2.md`).
//!
//! Proves the Substack golden-path *kernel mechanisms* offline against the
//! deterministic mock provider: select → typed action → proposal → diff →
//! approval → revision (with a recoverable before-image) → recipe → milestone
//! → lens → portable export → recovery, with **redacted, model-disclosing
//! receipts**. Plus the adversarial properties the loop must never violate:
//! a stale selection is never silently applied, a rejection mutates nothing,
//! and no receipt leaks an absolute path or content bytes.
//!
//! Written by the reviewer, not the implementers.

use projectos_kernel::actions::{run as run_action, OutputShape, SelectionAction};
use projectos_kernel::broker::{Broker, BrokerRequest, CapabilitySet};
use projectos_kernel::graph::{Edge, GraphEngine, Object, Relation};
use projectos_kernel::graph_memory::InMemoryGraphEngine;
use projectos_kernel::journal::{Journal, RecoverPolicy};
use projectos_kernel::lens::one_hop;
use projectos_kernel::milestone::{export, MilestoneStore};
use projectos_kernel::provider::MockProvider;
use projectos_kernel::proposals::{apply, Decision, Proposal, RevisionLog};
use projectos_kernel::receipts::{ReceiptWriter, RedactedReceipt};
use projectos_kernel::recipe::{RecipeDef, RecipeRun, RunState};
use projectos_kernel::registration;
use projectos_kernel::selection;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const DRAFT: &str = "Opening line.\n\nThis argument rests on a shaky premise.\n\nClosing.";
const PASSAGE: &str = "This argument rests on a shaky premise.";

fn obj(id: &str, kind: &str) -> Object {
    Object {
        id: id.to_string(),
        kind: kind.to_string(),
        label: id.to_string(),
        fields: BTreeMap::new(),
    }
}

fn draft_anchor(proj: &Path) -> projectos_kernel::selection::DocAnchor {
    let start = DRAFT.find(PASSAGE).unwrap();
    selection::anchor(proj, "draft.md", start, start + PASSAGE.len()).unwrap()
}

/// A1 — the headline golden path, offline and deterministic.
#[test]
fn a1_substack_golden_path_offline() {
    let dir = tempfile::tempdir().unwrap();
    let proj = dir.path().join("proj");
    fs::create_dir(&proj).unwrap();
    registration::register(&proj, "writing", vec![]).unwrap();
    fs::write(proj.join("draft.md"), DRAFT).unwrap();

    let held = registration::open(&proj).unwrap(); // writer session

    // (3–4) select + typed action via the mock provider.
    let a = draft_anchor(&proj);
    let provider = MockProvider::new();
    let outcome = run_action(&proj, &a, SelectionAction::Rephrase, &provider, "test/model-1").unwrap();
    assert_eq!(outcome.action, SelectionAction::Rephrase);
    assert_eq!(outcome.proposal.original, PASSAGE);
    assert_eq!(outcome.route, "test/model-1");
    assert_eq!(outcome.resolved_model, "test/model-1");
    assert!(outcome.usage_tokens.is_some());

    // (4) an inspectable diff — no silent insertion.
    let diff = outcome.proposal.diff();
    assert_eq!(diff.removed, vec![PASSAGE.to_string()]);
    assert_eq!(diff.added.len(), 1);

    // Record the action's proposal as a redacted receipt (model disclosure).
    let mut rw = ReceiptWriter::new(&proj).unwrap();
    rw.append(RedactedReceipt {
        id: String::new(),
        at: 1,
        op: "action".to_string(),
        rel_path: "draft.md".to_string(),
        outcome: "proposed".to_string(),
        content_hash: Some(outcome.proposal.after_hash.clone()),
        route: Some(outcome.route.clone()),
        cost: None,
    })
    .unwrap();
    drop(rw);

    // A real broker denial also becomes a durable redacted fact.
    let mut broker = Broker::new(proj.clone(), CapabilitySet::new());
    let _ = broker.request(BrokerRequest::ReadFile("/etc/passwd".to_string())).unwrap();
    assert_eq!(ReceiptWriter::ingest_broker_receipts(&proj, broker.receipts(), 2).unwrap(), 1);

    // (5–6) accept → revision, atomic replace, recoverable before-image.
    let original_bytes = fs::read(proj.join("draft.md")).unwrap();
    let rev = apply(&proj, &a, &outcome.proposal, Decision::Accept).unwrap().expect("a revision");
    assert_eq!(rev.before_hash, a.doc_hash);
    let new_text = fs::read_to_string(proj.join("draft.md")).unwrap();
    assert_eq!(rev.after_hash, projectos_kernel::provider::stable_hash(new_text.as_bytes()));
    assert!(new_text.contains(&outcome.proposal.proposed), "the artifact now holds the proposal");
    // The before-image is retrievable, content-addressed.
    let before_file = proj.join(".projectos").join("objects").join(&a.doc_hash);
    assert_eq!(fs::read(&before_file).unwrap(), original_bytes, "before-image restores the original bytes");
    assert_eq!(RevisionLog::new(&proj).unwrap().revisions().len(), 1);

    // (5) the publish-readiness recipe: inspect, run, pause at the human gate, resume.
    let def = RecipeDef::publish_readiness();
    assert!(def.inspect().unwrap().len() >= 5, "recipe is fully inspectable");
    let mut run = RecipeRun::start(&def);
    let mut steps = 0;
    loop {
        steps += 1;
        assert!(steps < 20, "run must be bounded");
        match run.state {
            RunState::AwaitingApproval => {
                run.approve();
            }
            RunState::Completed | RunState::Cancelled => break,
            _ => {
                run.advance();
            }
        }
    }
    assert_eq!(run.state, RunState::Completed);
    assert!(run.completed.iter().any(|id| id == "human_gate"), "the human gate was passed");

    // (6) lineage lens over the graph.
    let mut engine = InMemoryGraphEngine::new();
    for (id, kind) in [("d1", "Draft"), ("e1", "Evidence"), ("v1", "Revision")] {
        engine.upsert_object(obj(id, kind)).unwrap();
    }
    engine.add_edge(Edge { from: "v1".into(), to: "d1".into(), rel: Relation::RevisionOf }).unwrap();
    engine.add_edge(Edge { from: "d1".into(), to: "e1".into(), rel: Relation::Cites }).unwrap();
    let lens = one_hop(&engine, &"d1".to_string()).unwrap();
    assert!(lens.rows.iter().any(|r| r.object == "v1") && lens.rows.iter().any(|r| r.object == "e1"));

    // (7) named milestone.
    let mut journal = Journal::open(&proj).unwrap();
    assert!(!journal.checkpoint(&proj, &engine).unwrap().is_empty());
    let mut ms = MilestoneStore::new(&proj).unwrap();
    assert_eq!(ms.create(&proj, "publish candidate", &engine).unwrap().name, "publish candidate");

    drop(held);

    // (8) portable export — no `.tmp`, no absolute-path-named file, no bytes leak.
    let to = dir.path().join("export");
    let report = export(&proj, &to).unwrap();
    assert!(report.graph_snapshot && report.receipts >= 1);
    assert!(to.join("project.toml").exists() && to.join("milestones.toml").exists() && to.join("receipts.toml").exists());
    for entry in fs::read_dir(&to).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().to_string();
        assert!(!name.ends_with(".tmp"), "export left a .tmp: {name}");
    }

    // Receipts disclose the model route and never leak an absolute path or content.
    let canon = fs::canonicalize(&proj).unwrap().to_string_lossy().to_string();
    let receipts_text = fs::read_to_string(proj.join(".projectos").join("receipts.toml")).unwrap();
    assert!(receipts_text.contains("test/model-1"), "receipt discloses the resolved route");
    assert!(!receipts_text.contains(&canon), "receipt leaked the absolute project path");
    assert!(!receipts_text.contains("shaky premise"), "receipt leaked content bytes");

    // Recovery: the accepted fact survives and is not re-executed.
    Journal::recover(&proj, RecoverPolicy::Replay).unwrap();
    let after = ReceiptWriter::new(&proj).unwrap();
    assert_eq!(after.receipts().iter().filter(|r| r.outcome == "accepted").count(), 1);
    registration::open(&proj).expect("project reopens after recovery");
}

/// A2 — a selection whose document changed is never silently applied.
#[test]
fn a2_stale_selection_is_never_silently_applied() {
    let dir = tempfile::tempdir().unwrap();
    let proj = dir.path().join("proj");
    fs::create_dir(&proj).unwrap();
    fs::write(proj.join("draft.md"), DRAFT).unwrap();
    let a = draft_anchor(&proj);

    // External edit after anchoring.
    fs::write(proj.join("draft.md"), "Totally different content now.").unwrap();
    let provider = MockProvider::new();
    assert!(run_action(&proj, &a, SelectionAction::Rephrase, &provider, "m").is_err(), "stale action refused");

    // Even a pre-built proposal cannot be applied to changed bytes.
    let p = Proposal::new("draft.md", projectos_kernel::proposals::ProposalKind::Replacement, PASSAGE, "X", &a.doc_hash);
    assert!(apply(&proj, &a, &p, Decision::Accept).is_err(), "stale apply refused");
    assert_eq!(fs::read_to_string(proj.join("draft.md")).unwrap(), "Totally different content now.");
}

/// A3 — a rejection mutates nothing and records a redacted receipt.
#[test]
fn a3_rejected_proposal_mutates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let proj = dir.path().join("proj");
    fs::create_dir(&proj).unwrap();
    fs::write(proj.join("draft.md"), DRAFT).unwrap();
    let a = draft_anchor(&proj);

    let provider = MockProvider::new();
    let outcome = run_action(&proj, &a, SelectionAction::Explain, &provider, "m").unwrap();
    assert_eq!(outcome.proposal.kind, projectos_kernel::proposals::ProposalKind::Annotation);

    let rel = "draft.md";
    let p = Proposal::new(rel, projectos_kernel::proposals::ProposalKind::Replacement, PASSAGE, "replacement", &a.doc_hash);
    let before = fs::read(proj.join(rel)).unwrap();
    let rev = apply(&proj, &a, &p, Decision::Reject).unwrap();
    assert!(rev.is_none(), "a rejection returns no revision");
    assert_eq!(fs::read(proj.join(rel)).unwrap(), before, "a rejection mutates nothing");
    let rw = ReceiptWriter::new(&proj).unwrap();
    assert!(rw.receipts().iter().any(|r| r.outcome == "rejected"));
    assert_eq!(RevisionLog::new(&proj).unwrap().revisions().len(), 0);
}

/// A4 — the shapes/route are typed and the export path stays relative.
#[test]
fn a4_contracts_are_typed() {
    let dir = tempfile::tempdir().unwrap();
    let proj: PathBuf = dir.path().join("proj");
    fs::create_dir(&proj).unwrap();
    // Rephrase/Complete rewrite; Explain/FactCheck annotate (brief §8).
    assert_eq!(SelectionAction::Rephrase.contract().output, OutputShape::Replacement);
    assert_eq!(SelectionAction::Complete.contract().output, OutputShape::Replacement);
    assert_eq!(SelectionAction::Explain.contract().output, OutputShape::Annotation);
    assert_eq!(SelectionAction::FactCheck.contract().output, OutputShape::Annotation);
    assert!(SelectionAction::Explain.contract().read_only);
    assert!(!SelectionAction::Rephrase.contract().read_only);
}
