//! Tests for the inspectable, bounded recipe engine (M2 §B).

use projectos_kernel::recipe::{RecipeBlock, RecipeDef, RecipeRun, RunState};

fn def_of(ids: &[(&str, bool)]) -> RecipeDef {
    RecipeDef {
        id: "t".to_string(),
        version: "0".to_string(),
        blocks: ids
            .iter()
            .map(|(id, gate)| RecipeBlock {
                id: id.to_string(),
                action: format!("a.{id}"),
                optional: false,
                approval_gate: *gate,
            })
            .collect(),
    }
}

#[test]
fn inspect_lists_blocks_in_order() {
    let def = RecipeDef::publish_readiness();
    let blocks = def.inspect().unwrap();
    let ids: Vec<&str> = blocks.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["structure", "claims", "citations", "voice", "human_gate", "outputs"]
    );
}

#[test]
fn inspect_rejects_empty_and_duplicate() {
    let empty = RecipeDef {
        id: "e".to_string(),
        version: "0".to_string(),
        blocks: vec![],
    };
    assert!(empty.inspect().is_err());

    let dup = def_of(&[("x", false), ("x", false)]);
    assert!(dup.inspect().is_err());
}

#[test]
fn publish_readiness_has_a_human_approval_gate() {
    let def = RecipeDef::publish_readiness();
    assert!(
        def.blocks.iter().any(|b| b.approval_gate),
        "publish-readiness must carry a human approval gate"
    );
}

#[test]
fn full_run_completes_blocks_in_order() {
    let def = def_of(&[("a", false), ("b", false), ("c", false)]);
    let mut run = RecipeRun::start(&def);
    assert_eq!(run.state, RunState::Running);
    assert_eq!(run.current.as_deref(), Some("a"));

    assert_eq!(run.advance(), RunState::Running); // completes a
    assert_eq!(run.advance(), RunState::Running); // completes b
    assert_eq!(run.advance(), RunState::Completed); // completes c
    assert_eq!(run.completed, vec!["a", "b", "c"]);
    assert_eq!(run.current, None);
}

#[test]
fn run_pauses_at_gate_and_approve_resumes() {
    // a, gate, c
    let def = def_of(&[("a", false), ("g", true), ("c", false)]);
    let mut run = RecipeRun::start(&def);

    assert_eq!(run.advance(), RunState::Running); // completes a, now at g
    assert_eq!(run.current.as_deref(), Some("g"));
    assert_eq!(run.advance(), RunState::AwaitingApproval); // pauses at the gate
    assert_eq!(run.advance(), RunState::AwaitingApproval); // still paused (no-op)
    assert!(!run.completed.contains(&"g".to_string()), "gated block not completed yet");

    assert_eq!(run.approve(), RunState::Running); // gate passed, now at c
    assert!(run.completed.contains(&"g".to_string()));
    assert_eq!(run.current.as_deref(), Some("c"));
    assert_eq!(run.advance(), RunState::Completed);
    assert_eq!(run.completed, vec!["a", "g", "c"]);
}

#[test]
fn cancel_is_terminal_and_idempotent() {
    let def = def_of(&[("a", false), ("b", false)]);
    let mut run = RecipeRun::start(&def);
    run.advance();
    assert_eq!(run.cancel(), RunState::Cancelled);
    assert_eq!(run.cancel(), RunState::Cancelled);
    assert_eq!(run.advance(), RunState::Cancelled, "advance after cancel is a no-op");
    assert_eq!(run.approve(), RunState::Cancelled, "approve after cancel is a no-op");
}
