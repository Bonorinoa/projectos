//! Inspectable, bounded recipes (M2, `m2.recipe`).
//!
//! A recipe is a typed, versioned list of ordered blocks that can be **inspected
//! in full before it runs** and is **bounded and cancellable** (brief §8: no
//! arbitrary topology, no unbounded loops, no hidden retries). A block may carry
//! a **human approval gate**; a run pauses there until [`RecipeRun::approve`].
//!
//! ## Transition rule (documented)
//!
//! `start` → `Running` with `current` = the first block. `advance` completes the
//! current block and moves to the next, **except** that a block with
//! `approval_gate = true` moves the run to `AwaitingApproval` *without*
//! completing it; `approve` completes a gated block and resumes. Reaching the
//! end → `Completed` (`current = None`). `cancel` → `Cancelled`. `Completed` and
//! `Cancelled` are terminal: `advance`/`approve`/`cancel` are no-ops returning
//! the current state.

use crate::error::{KernelError, Result};

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
///
/// `blocks` is a private implementation detail: `advance`/`approve` need the
/// ordered definition to know whether the current block is an approval gate.
#[derive(Debug, Clone, PartialEq)]
pub struct RecipeRun {
    pub recipe_id: String,
    pub version: String,
    pub state: RunState,
    pub completed: Vec<String>,
    pub current: Option<String>,
    blocks: Vec<RecipeBlock>,
}

fn block(id: &str, action: &str, optional: bool, approval_gate: bool) -> RecipeBlock {
    RecipeBlock {
        id: id.to_string(),
        action: action.to_string(),
        optional,
        approval_gate,
    }
}

impl RecipeDef {
    /// The first-party publish-readiness recipe: structure, claims, citations,
    /// voice, a **human approval gate**, and declared outputs.
    pub fn publish_readiness() -> Self {
        RecipeDef {
            id: "substack.publish-readiness".to_string(),
            version: "1.0.0".to_string(),
            blocks: vec![
                block("structure", "review.structure", false, false),
                block("claims", "review.claims", false, false),
                block("citations", "validate.citations", false, false),
                block("voice", "review.voice", true, false),
                block("human_gate", "approve.publish", false, true),
                block("outputs", "emit.outputs", false, false),
            ],
        }
    }

    /// Validate + list the blocks in order (inspection before running).
    ///
    /// Errors on an empty recipe or a duplicate block id.
    pub fn inspect(&self) -> Result<Vec<&RecipeBlock>> {
        if self.blocks.is_empty() {
            return Err(KernelError::InvalidSidecar("recipe has no blocks".to_string()));
        }
        let mut seen = std::collections::BTreeSet::new();
        for b in &self.blocks {
            if !seen.insert(b.id.as_str()) {
                return Err(KernelError::InvalidSidecar(format!(
                    "duplicate recipe block id: {}",
                    b.id
                )));
            }
        }
        Ok(self.blocks.iter().collect())
    }
}

impl RecipeRun {
    pub fn start(def: &RecipeDef) -> Self {
        RecipeRun {
            recipe_id: def.id.clone(),
            version: def.version.clone(),
            state: RunState::Running,
            completed: Vec::new(),
            current: def.blocks.first().map(|b| b.id.clone()),
            blocks: def.blocks.clone(),
        }
    }

    fn index_of(&self, id: &str) -> Option<usize> {
        self.blocks.iter().position(|b| b.id == id)
    }

    fn complete_current(&mut self, idx: usize) {
        let id = self.blocks[idx].id.clone();
        if !self.completed.contains(&id) {
            self.completed.push(id);
        }
        if idx + 1 < self.blocks.len() {
            self.current = Some(self.blocks[idx + 1].id.clone());
            self.state = RunState::Running;
        } else {
            self.current = None;
            self.state = RunState::Completed;
        }
    }

    pub fn advance(&mut self) -> RunState {
        match self.state {
            RunState::Completed | RunState::Cancelled | RunState::AwaitingApproval => {
                return self.state
            }
            RunState::Pending | RunState::Running => {}
        }
        let idx = match self.current.as_deref().and_then(|id| self.index_of(id)) {
            Some(i) => i,
            None => {
                self.current = None;
                self.state = RunState::Completed;
                return self.state;
            }
        };
        if self.blocks[idx].approval_gate {
            self.state = RunState::AwaitingApproval;
            return self.state;
        }
        self.complete_current(idx);
        self.state
    }

    pub fn approve(&mut self) -> RunState {
        if self.state != RunState::AwaitingApproval {
            return self.state;
        }
        match self.current.as_deref().and_then(|id| self.index_of(id)) {
            Some(idx) => self.complete_current(idx),
            None => {
                self.current = None;
                self.state = RunState::Completed;
            }
        }
        self.state
    }

    pub fn cancel(&mut self) -> RunState {
        if matches!(self.state, RunState::Completed | RunState::Cancelled) {
            return self.state;
        }
        self.state = RunState::Cancelled;
        self.state
    }
}
