//! Inspectable, bounded recipes (M2 child B) — SCAFFOLD STUB.
//!
//! Frozen contract: `kernel/INTERFACES-M2.md` §B. The owner of this file
//! replaces every `unimplemented!()` body with the real behaviour + tests in
//! `tests/recipe.rs`. Do not change the public signatures.

use crate::error::Result;

/// One ordered block of a recipe.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RecipeBlock {
    pub id: String,
    pub action: String,
    pub optional: bool,
    pub approval_gate: bool,
}

/// A typed, versioned recipe definition (a bounded DAG in the MVP).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RecipeDef {
    pub id: String,
    pub version: String,
    pub blocks: Vec<RecipeBlock>,
}

/// The lifecycle state of a recipe run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Pending,
    Running,
    AwaitingApproval,
    Completed,
    Cancelled,
}

/// A bounded, cancellable run of a recipe.
#[derive(Debug, Clone, PartialEq)]
pub struct RecipeRun {
    pub recipe_id: String,
    pub version: String,
    pub state: RunState,
    pub completed: Vec<String>,
    pub current: Option<String>,
}

impl RecipeDef {
    /// The first-party publish-readiness recipe (structure, claims, citations,
    /// voice, a human approval gate, declared outputs).
    pub fn publish_readiness() -> Self {
        unimplemented!()
    }

    /// Validate + list the blocks in order (inspection before running).
    pub fn inspect(&self) -> Result<Vec<&RecipeBlock>> {
        unimplemented!()
    }
}

impl RecipeRun {
    pub fn start(def: &RecipeDef) -> Self {
        let _ = def;
        unimplemented!()
    }

    pub fn advance(&mut self) -> RunState {
        unimplemented!()
    }

    pub fn approve(&mut self) -> RunState {
        unimplemented!()
    }

    pub fn cancel(&mut self) -> RunState {
        unimplemented!()
    }
}
