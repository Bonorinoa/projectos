# M2 kernel slice — interface contract (frozen for this pass)

M2 is the **Substack golden path** (brief §18). This contract freezes the
*kernel mechanisms* (slice M2.0, see `docs/M2-PLAN-REVIEW.md`): everything the
eight-step path needs **except** live provider transport (M2.2, blocked on owner
resources) and the app-side editor (M2.1, deferred). Mechanisms are proven
offline against a deterministic mock provider.

Read first: `docs/projectos-architecture-brief.md` (§6 model, §8 interaction
grammar, §9 trace contract, §13 Substack versioning, §14 golden path) and
`docs/M2-PLAN-REVIEW.md` (why M2 = Substack path, not Helix wiring).

## Crate layout (this slice)

```
kernel/src/
├── lib.rs          # module declarations (parent-owned, frozen)
├── selection.rs    # byte-range document anchors + freshness   [core]
├── provider.rs     # Provider trait + deterministic MockProvider (offline) [core]
├── proposals.rs    # Proposal / Diff / Decision / Revision / before-images [core]
├── actions.rs      # typed selection actions + contracts        [child A]
├── recipe.rs       # inspectable bounded recipe + run state machine [child B]
├── milestone.rs    # named milestone + portable export           [child C]
└── lens.rs         # evidence / lineage lens rows (table-equivalent) [child D]
```

**Ownership (strict, one writer per file).** `selection.rs`, `provider.rs`,
`proposals.rs`, `lib.rs`, `Cargo.toml` are **parent-written in the scaffold and
frozen** — children read them, never edit them. Child A owns `actions.rs` +
`tests/actions.rs`; B owns `recipe.rs` + `tests/recipe.rs`; C owns `milestone.rs`
+ `tests/milestone.rs`; D owns `lens.rs` + `tests/lens.rs`. No `git`. No new
crate dependencies (only `serde`, `toml`, `uuid`, `thiserror`; dev `tempfile`).
Persisted files are **TOML**. Hashing is **FNV-1a-64 → 16 lowercase hex**
(documented in `receipts.rs`); the scaffold exposes `crate::provider::stable_hash`.

## Build/test environment (exact)

```sh
export CARGO_HOME=/private/tmp/projectos-gate-a-cargo
export RUSTUP_HOME=/private/tmp/projectos-gate-a-rustup
export RUSTUP_TOOLCHAIN=1.97.1          # default here is 1.90.0; crate requires 1.97.1
export PATH=/private/tmp/projectos-gate-a-cargo/bin:$PATH
cd <repo>/kernel && cargo test
```

## Frozen core (parent)

### `selection.rs`
```rust
pub struct DocAnchor { pub rel_path: String, pub byte_start: usize, pub byte_end: usize,
                       pub text_hash: String, pub doc_hash: String }   // Serialize+Deserialize+Clone+PartialEq+Debug
pub enum AnchorState { Fresh, Stale(String) }
pub fn anchor(folder: &Path, rel_path: &str, byte_start: usize, byte_end: usize) -> Result<DocAnchor>;
pub fn resolve(folder: &Path, a: &DocAnchor) -> Result<AnchorState>;
```
`rel_path` must be relative and non-escaping (`..`/absolute → `InvalidSidecar`);
the range must be in-bounds and on char boundaries. `resolve` re-hashes the
document: `Fresh` iff `doc_hash` matches; missing file or changed bytes →
`Stale(reason)` (never an error — a stale selection fails safely downstream).

### `provider.rs`
```rust
pub struct ProviderRequest { pub model: String, pub prompt: String }
pub struct ProviderResponse { pub text: String, pub resolved_model: String, pub usage_tokens: Option<u64> }
pub trait Provider { fn complete(&self, req: &ProviderRequest) -> Result<ProviderResponse>; }
pub struct MockProvider;   // deterministic, offline, no network
impl MockProvider { pub fn new() -> Self; }
pub fn stable_hash(bytes: &[u8]) -> String;   // FNV-1a-64, 16 hex
```
`MockProvider::complete` returns a **deterministic** function of the request
(document the exact rule). No network, no credentials — this is the offline
stand-in; the live adapter is M2.2.

### `proposals.rs`
```rust
pub enum ProposalKind { Replacement, Annotation }
pub struct Proposal { pub id: String, pub rel_path: String, pub kind: ProposalKind,
                      pub original: String, pub proposed: String,
                      pub before_hash: String, pub after_hash: String }
pub struct Diff { pub removed: Vec<String>, pub added: Vec<String> }
pub enum Decision { Accept, Reject }
pub struct Revision { pub id: String, pub proposal_id: String, pub rel_path: String,
                      pub before_hash: String, pub after_hash: String, pub at: u64 }
pub struct RevisionLog { /* .projectos/revisions.toml */ }
impl Proposal {
    pub fn new(rel_path: &str, kind: ProposalKind, original: &str, proposed: &str, before_hash: &str) -> Self;
    pub fn diff(&self) -> Diff;
}
impl RevisionLog { pub fn new(folder: &Path) -> Result<Self>; pub fn revisions(&self) -> &[Revision]; }
pub fn apply(folder: &Path, anchor: &crate::selection::DocAnchor, p: &Proposal,
             d: Decision) -> Result<Option<Revision>>;
```
Rules: `apply(Accept)` **re-resolves the anchor** and errors
(`InvalidSidecar("stale selection")`) if not `Fresh` — no silent application of a
stale proposal. It stores the **before-image** as its full previous bytes under
`.projectos/objects/<before_hash>` (content-addressed; no-op if present),
replaces the anchored byte range atomically (tmp + rename), appends a journal
`Mutated` record and a **redacted receipt** (`op = "apply"`, no bytes/absolute
paths) via the existing `receipts`/`journal` modules, appends the `Revision`, and
returns it. `apply(Reject)` mutates nothing and records a `rejected` receipt,
returning `None`.

## Child modules

### A — `actions.rs` (+ `tests/actions.rs`)
```rust
pub enum SelectionAction { Rephrase, Explain, Complete, FactCheck }
pub enum OutputShape { Replacement, Annotation }
pub struct ActionContract { pub id: &'static str, pub output: OutputShape, pub read_only: bool }
impl SelectionAction { pub fn contract(self) -> ActionContract; }
pub struct ActionOutcome { pub action: SelectionAction, pub proposal: crate::proposals::Proposal,
                           pub route: String, pub resolved_model: String, pub usage_tokens: Option<u64> }
pub fn run(folder: &Path, a: &crate::selection::DocAnchor, action: SelectionAction,
           provider: &dyn crate::provider::Provider, model: &str) -> Result<ActionOutcome>;
```
Contracts: `Rephrase`/`Complete` → `Replacement` (writes back to the artifact);
`Explain`/`FactCheck` → `Annotation` (read-only). `run` resolves the anchor first
(`Stale` → `InvalidSidecar("stale selection: …")`), reads the selected text, calls
the provider, and builds the `Proposal` (`kind` from the contract,
`original` = selected text, `proposed` = provider text, `before_hash` =
`anchor.doc_hash`). Required tests: each action's contract; a stale anchor is
refused; `Rephrase` yields a `Replacement` proposal whose `original` is the
selection; an `Annotation` action yields `Annotation`; the route/model are
recorded in the outcome.

### B — `recipe.rs` (+ `tests/recipe.rs`)
```rust
pub struct RecipeBlock { pub id: String, pub action: String, pub optional: bool, pub approval_gate: bool }
pub struct RecipeDef { pub id: String, pub version: String, pub blocks: Vec<RecipeBlock> }
pub enum RunState { Pending, Running, AwaitingApproval, Completed, Cancelled }
pub struct RecipeRun { pub recipe_id: String, pub version: String, pub state: RunState,
                       pub completed: Vec<String>, pub current: Option<String> }
impl RecipeDef { pub fn publish_readiness() -> Self; pub fn inspect(&self) -> Result<Vec<&RecipeBlock>>; }
impl RecipeRun { pub fn start(def: &RecipeDef) -> Self; pub fn advance(&mut self) -> RunState;
                 pub fn approve(&mut self) -> RunState; pub fn cancel(&mut self) -> RunState; }
```
The recipe is a **typed DAG inspectable before running**; runs are **bounded and
cancellable**. `publish_readiness()` must include structure / claims / citations /
voice / a human approval gate / declared outputs, with no unbounded loops.
`inspect()` validates non-empty blocks and unique ids. `advance` walks blocks,
pausing at an `approval_gate` block (`AwaitingApproval`); `approve` resumes;
completion → `Completed`; `cancel` → `Cancelled` (terminal; further calls are
no-ops). Required tests: `inspect` lists every block in order and rejects an
empty/duplicate-id def; a run pauses at the approval gate and resumes; `cancel`
is terminal and idempotent; a full run completes in order.

### C — `milestone.rs` (+ `tests/milestone.rs`)
```rust
pub struct Milestone { pub name: String, pub checkpoint: String, pub at: u64 }
pub struct MilestoneStore { /* .projectos/milestones.toml */ }
impl MilestoneStore { pub fn new(folder: &Path) -> Result<Self>; pub fn milestones(&self) -> &[Milestone];
    pub fn create(&mut self, folder: &Path, name: &str, engine: &dyn crate::graph::GraphEngine) -> Result<Milestone>; }
pub struct ExportReport { pub files: usize, pub graph_snapshot: bool, pub receipts: usize }
pub fn export(folder: &Path, to: &Path) -> Result<ExportReport>;
```
`create` quiesces the engine via `journal::Journal::checkpoint` and records a
named milestone (empty name → `InvalidSidecar`). `export` writes a **portable**
copy into `to`: `project.toml`, the graph snapshot, `receipts.toml`,
`revisions.toml`, `milestones.toml` — **never** credentials, `.tmp`, caches, or
the raw trace vault (none exist yet). Required tests: create→persist→reopen
round-trip; empty name rejected; export produces the expected files and
`ExportReport` counts and contains **no** absolute path or `.tmp`.

### D — `lens.rs` (+ `tests/lens.rs`)
```rust
pub struct LensRow { pub object: String, pub kind: String, pub relation: String }
pub struct Lens { pub center: String, pub rows: Vec<LensRow> }
pub fn one_hop(engine: &dyn crate::graph::GraphEngine, id: &crate::graph::ObjectId) -> Result<Lens>;
pub fn lineage_lens(engine: &dyn crate::graph::GraphEngine, id: &crate::graph::ObjectId) -> Result<Lens>;
pub fn evidence_lens(engine: &dyn crate::graph::GraphEngine, id: &crate::graph::ObjectId) -> Result<Lens>;
```
Every lens is a **table-equivalent** (deterministically ordered rows), satisfying
brief §8 "every lens has an equivalent list/table". `one_hop` = objects within
one hop; `lineage_lens` = the transitive `lineage` closure; `evidence_lens` =
neighbours joined by `Supports`/`Cites`/`GovernedBy`. Required tests: one-hop
finds both directions; lineage includes the transitive closure and terminates on
a cycle; rows are deterministically ordered; an unknown id yields empty rows (not
an error).

## Reporting (all children)

Files written; exact commands; raw pass/fail counts; assumptions; anything you
could not do and why. Do not commit. Do not claim more than the runs show.
