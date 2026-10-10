# M1 slice 2 — interface contract (frozen for this pass)

Slice 1 is committed (`1f05d20`, 23/23 tests, review: `REVIEW-m1-slice.md`).
This contract freezes slice 2: the graph engine, the profile split, the recovery
journal, the receipt writer, and the app shell (canvas + a11y list). Owner
decisions in force: **in-memory `GraphEngine`** (Helix fork in M2),
**Synodos Field** visual language, **a11y list-equivalent + keyboard path in this
pass**, subagent driver = `deepseek/deepseek-v4-flash-0731`.

## Parent-applied edits (do not re-do, do not undo)

- `registration.rs`: the lock body now records `pid`/`started_at` (TOML);
  `registration::lock_owner(folder) -> Result<Option<LockOwner>>` is public;
  `registration::release_lock(folder)` is **`pub(crate)`** for the recovery path.
- `sidecar.rs`: `pub use crate::error::SCHEMA_VERSION;` re-export.
- `lib.rs` declares `graph`, `graph_memory`, `journal`, `profile`, `receipts`.
- **Alias semantics decided (was an open question in the slice-1 review):**
  `register()` is *first registration only*. Identity is the canonical path; any
  **different spelling** of a registered project (symlink, case variant,
  canonical-vs-lexical) is an `AliasPath` error naming the registered path.
  Callers doing real work use `open()`, which is spelling-agnostic. This is
  intended behavior — documented in the code, not a bug to fix.

## Ownership (strict — one writer per file)

| Workstream | Owns | May NOT touch |
|---|---|---|
| **G** graph | `src/graph.rs`, `src/graph_memory.rs`, `tests/graph.rs` | anything else |
| **P** profile | `src/profile.rs`, `tests/profile.rs` | anything else |
| **J** journal | `src/journal.rs`, `tests/journal.rs` | anything else |
| **R** receipts | `src/receipts.rs`, `tests/receipts.rs` | anything else |
| **U** app shell | `app/**` (new) | anything in `kernel/`, `design/`, `docs/` |

Parents own `Cargo.toml`, `src/lib.rs`, this file, and the existing modules.
No `git` commands. No new crate dependencies — use `serde`, `toml`, `uuid`,
`thiserror` (dev: `tempfile`). Persisted files are **TOML** (no new deps).

## Build/test environment (exact)

```sh
export CARGO_HOME=/private/tmp/projectos-gate-a-cargo
export RUSTUP_HOME=/private/tmp/projectos-gate-a-rustup
export RUSTUP_TOOLCHAIN=1.97.1     # default here is 1.90.0 and this crate requires 1.97.1
export PATH=/private/tmp/projectos-gate-a-cargo/bin:$PATH
cd /Users/bonorinoa/Documents/ChatGPT/ProjectOS/kernel
cargo test                      # your suite + everyone else's must stay green
```

## G — `graph.rs` + `graph_memory.rs` (public API frozen)

```rust
pub type ObjectId = String;                      // opaque id string (e.g. uuid)
pub enum Relation { Supports, Cites, GovernedBy, Executed, Produced, RevisionOf, AppliedTo, LinksTo }
pub struct Object { pub id: ObjectId, pub kind: String, pub label: String,
                    pub fields: std::collections::BTreeMap<String, String> }
pub struct Edge { pub from: ObjectId, pub to: ObjectId, pub rel: Relation }

pub trait GraphEngine {
    fn upsert_object(&mut self, obj: Object) -> crate::error::Result<()>;
    fn delete_object(&mut self, id: &ObjectId) -> crate::error::Result<bool>;
    fn object(&self, id: &ObjectId) -> crate::error::Result<Option<Object>>;
    fn objects(&self) -> crate::error::Result<Vec<Object>>;          // deterministic order
    fn add_edge(&mut self, e: Edge) -> crate::error::Result<()>;
    fn edges(&self) -> crate::error::Result<Vec<Edge>>;              // deterministic order
    fn lineage(&self, id: &ObjectId) -> crate::error::Result<Vec<ObjectId>>; // transitive closure, cycle-safe
    fn save_snapshot(&self, file: &std::path::Path) -> crate::error::Result<()>; // TOML, atomic (tmp+rename)
}

pub struct InMemoryGraphEngine { /* BTreeMap-based */ }
impl InMemoryGraphEngine {
    pub fn new() -> Self;
    pub fn load_snapshot(file: &std::path::Path) -> crate::error::Result<Self>;
}
impl GraphEngine for InMemoryGraphEngine { /* ... */ }
```
Rules: deterministic iteration (sorted) so snapshots are byte-stable; `lineage`
must terminate on cycles; `delete_object` also removes incident edges;
`add_edge` to a missing object is an `InvalidSidecar`-style error (pick the
closest `KernelError` variant and say which in your report).

**Required tests** (`tests/graph.rs`): upsert/get/delete; delete removes incident
edges; edge to a missing object errors; `lineage` is transitive AND cycle-safe
(build a cycle, assert termination + set equality); snapshot round-trip is
byte-identical across two saves of the same graph; deterministic ordering.

## P — `profile.rs` (public API frozen)

The profile graph is **installation-specific** and must **never override a
project UUID** (brief §15). It lives in an app-data directory the caller passes.

```rust
pub struct ProfileLink { pub label: String, pub target_object: String }
pub struct ProfileEntry {
    pub project_uuid: uuid::Uuid,
    pub path_binding: std::path::PathBuf,   // machine-local; never identity
    pub last_seen_checkpoint: Option<String>,
    pub links: Vec<ProfileLink>,
}
pub struct RestoreReport { pub entries: usize, pub rebound: usize, pub missing: Vec<uuid::Uuid> }
pub struct ProfileRegistry { /* loaded from <app_data>/profile.toml */ }
impl ProfileRegistry {
    pub fn open(app_data_dir: &std::path::Path) -> crate::error::Result<Self>;
    pub fn upsert(&mut self, entry: ProfileEntry) -> crate::error::Result<()>;
    pub fn get(&self, uuid: &uuid::Uuid) -> Option<&ProfileEntry>;
    pub fn entries(&self) -> Vec<&ProfileEntry>;                    // deterministic order
    pub fn save(&self) -> crate::error::Result<()>;                 // atomic TOML write
    pub fn export(&self, to: &std::path::Path) -> crate::error::Result<()>;
    pub fn restore(&mut self, from: &std::path::Path,
                   project_root: &std::path::Path) -> crate::error::Result<RestoreReport>;
}
```
Rules: `restore` re-binds export entries into a **clean** app-data dir,
re-attaching each project's `path_binding` under `project_root` and verifying
each folder's sidecar UUID **matches the entry; on mismatch the SIDECAR UUID
WINS** and the entry is re-bound to the on-disk project (never the reverse).
Report `missing` for entries whose folder is not present.

**Required tests** (`tests/profile.rs`): save/open round-trip; export → restore
into a clean dir reproduces entries and rebinds paths; **UUID authority**: an
entry whose path holds a different sidecar UUID is corrected to the sidecar's
UUID (assert the sidecar's uuid wins); missing folders land in `restore.missing`;
deterministic ordering.

## J — `journal.rs` (public API frozen)

```rust
pub enum Record {
    Opened { at: u64 },
    Mutated { object: String, at: u64 },
    Checkpoint { id: String, at: u64 },
    Recovery { policy: String, at: u64 },
}
pub struct RecoveryReport { pub replayed: usize, pub lock_taken_over: bool, pub checkpoint: Option<String> }
pub enum RecoverPolicy { Replay, TakeoverStaleLock }
pub struct Journal { /* <project>/.projectos/journal.toml, append-only */ }
impl Journal {
    pub fn open(folder: &std::path::Path) -> crate::error::Result<Self>;
    pub fn append(&mut self, r: Record) -> crate::error::Result<()>;
    pub fn records(&self) -> &[Record];
    pub fn checkpoint(&mut self, folder: &std::path::Path,
                      engine: &dyn crate::graph::GraphEngine) -> crate::error::Result<String>;
    pub fn recover(folder: &std::path::Path, policy: RecoverPolicy)
        -> crate::error::Result<RecoveryReport>;
}
```
Rules: records are append-only (rewrite the whole file atomically each append;
keep it simple and auditable); `checkpoint` snapshots the engine to
`.projectos/graph/snapshot.toml` (create the dir) and appends a `Checkpoint`
record, returning the checkpoint id; `recover(Replay)` reads the latest
checkpoint, loads it via `InMemoryGraphEngine::load_snapshot`, counts the records
after that checkpoint as `replayed`, and appends a `Recovery` record;
`recover(TakeoverStaleLock)` does the same **and** calls
`crate::registration::release_lock(folder)` (it is `pub(crate)`), setting
`lock_taken_over = true`. `recover` on a project with no journal is an error.

**Required tests** (`tests/journal.rs`): append/read round-trip; checkpoint
writes a snapshot + record and returns an id; Replay after a checkpoint reports
the right `replayed` count; TakeoverStaleLock actually releases a lock file
(create one with `registration::open` held by a dropped handle or write the file
directly, then assert a later `registration::open` succeeds); a `Recovery` record
is appended; journal persists across `Journal::open` calls.

## R — `receipts.rs` (public API frozen)

Durable **redacted** receipt facts (brief §15: the project graph holds redacted
receipts; the sensitive trace vault stays out of scope this pass).

```rust
pub struct RedactedReceipt {
    pub id: String,               // deterministic: sha256-less — derive from (at, op, rel_path) via a simple stable hash you document
    pub at: u64,
    pub op: String,
    pub rel_path: String,         // redacted: never absolute
    pub outcome: String,
    pub content_hash: Option<String>,
    pub route: Option<String>,
    pub cost: Option<String>,
}
pub struct ReceiptWriter { /* <project>/.projectos/receipts.toml */ }
impl ReceiptWriter {
    pub fn new(folder: &std::path::Path) -> crate::error::Result<Self>;
    pub fn append(&mut self, r: RedactedReceipt) -> crate::error::Result<()>;
    pub fn receipts(&self) -> &[RedactedReceipt];
    pub fn ingest_broker_receipts(folder: &std::path::Path,
        receipts: &[crate::broker::Receipt], at: u64) -> crate::error::Result<usize>;
    pub fn redact_rel_path(rel: &str) -> String;   // same rule as the broker
}
```
Rules: **no file bytes and no absolute paths ever reach disk**; `append` rewrites
atomically; `ingest_broker_receipts` turns broker denials into durable facts and
returns how many were written; ids are deterministic and documented.

**Required tests** (`tests/receipts.rs`): append/read round-trip; the persisted
file contains no absolute path and no test secret bytes (read the file back and
assert); `ingest_broker_receipts` from a broker that denied an absolute attempt
and a `../` attempt writes two redacted facts; ids deterministic across two runs
with identical inputs; atomic (no `.tmp` left behind).

## U — app shell (`app/**`, no Rust workspace root)

Layout to create:

```
app/
├── assets/fonts/…            (ALREADY VENDORED — do not modify)
├── ui/
│   ├── index.html            # the real artifact canvas (Synodos Field)
│   ├── app.js                # canvas engine + a11y list view, driven by a mock kernel bridge
│   ├── tokens.css            # copy of design/tokens/synodos-field.css (add a header comment)
│   └── bridge.js             # `window.projectos` mock: listProjects/objects/edges/receipts + register/open stubs
└── src-tauri/
    ├── Cargo.toml            # standalone crate `projectos-app`; dep: projectos-kernel = { path = "../../kernel" }, tauri = "=2.12.0"
    ├── build.rs              # tauri_build::build()
    ├── tauri.conf.json       # frontendDist: "../ui", devUrl omitted; identifier com.projectos.app
    └── src/main.rs           # Tauri v2 app exposing 3 commands: project_status(folder), project_register(folder,declared_type), project_open(folder) wired to the kernel
```

Requirements:
1. **Canvas** (Synodos Field): typed artifact nodes with the 12×12 pixel sprites
   from `design/sketches/007-canvas-synodos-field/index.html` (reuse the sprite
   rect data), dashed typed edges, port-to-port linking, lens filters
   (All/Lineage/Evidence/Workflow), zoom, drag, inspector with typed fields.
   Use the **vendored** fonts via relative paths (`../assets/fonts/...`) — no CDN.
2. **A11y list-equivalent**: the same data as a semantic `<table>`/list view,
   toggled by a button AND by a keyboard shortcut; every artifact reachable by
   keyboard (Tab order, Arrow keys to move, Enter to open the inspector, Escape to
   close); the canvas and list expose the same records. Reduced-motion respected.
3. **No provider calls, no network** except none at all (fonts local).
4. Rust: the three commands call `projectos_kernel::registration::{register,open}`
   and `sidecar::SidecarEnvelope::read`; they must **compile** (`cargo build`)
   under the pinned toolchain. A full `tauri build` bundle is NOT required.

**Verification (do this yourself and report it):**
- `cd app/ui && python3 -m http.server` is NOT needed — instead render with a
  headless browser if available, else assert the page loads by opening it with
  `file://` and reporting the DOM checks you performed; at minimum verify the
  HTML parses (no unclosed tags), `bridge.js` and `app.js` load without errors,
  and the a11y table is present in the initial DOM.
- `cd app/src-tauri && cargo build` must succeed (pinned toolchain) — report the
  exact command + result. If Tauri's build needs assets you cannot provide,
  report precisely what failed instead of faking it.

## Reporting (all children)

Files written; exact commands; raw pass/fail counts; assumptions; anything you
could not do and why. Do not commit. Do not claim more than the runs show.
