# M1 thin-slice interface contract (frozen for this pass)

Owner decisions (2026-10-09): **trait boundary first** (broker + `WorkerHost`
trait, in-process adapter; WASM guest is the second adapter), **thin vertical
slice** (registration + `.projectos` sidecar + capability broker), **in-memory
`GraphEngine`** (Helix fork is wired in M2), **fonts vendored + a11y list path
in this pass**. Repo: `/Users/bonorinoa/Documents/ChatGPT/ProjectOS`.

Read first: `docs/projectos-architecture-brief.md` (§15 stores/sidecar layout,
§18 M1 exit criteria) and `docs/adr-worker-isolation.md` (broker pattern, and
the rule that product features must not be built on the unproven Gate-C
boundary — which is why this slice has a *trait*, not a confinement claim).

## Crate layout (this slice)

```
kernel/                      # standalone crate (no workspace root yet — app/ adds one with excludes)
├── Cargo.toml
├── INTERFACES.md            # this file
├── src/
│   ├── lib.rs               # module declarations (parent-owned, frozen)
│   ├── error.rs             # KernelError                     [child A]
│   ├── sidecar.rs           # .projectos/project.toml envelope [child A]
│   ├── registration.rs      # register/open/move + locking      [child A]
│   ├── broker.rs            # capability broker + receipts      [child B]
│   └── worker.rs            # WorkerHost trait + in-process adapter [child B]
└── tests/
    ├── sidecar_registration.rs   [child A]
    └── broker.rs                 [child B]
```

**Ownership is strict.** Child A writes `error.rs`, `sidecar.rs`,
`registration.rs`, `tests/sidecar_registration.rs`. Child B writes `broker.rs`,
`worker.rs`, `tests/broker.rs`. **Nobody edits `Cargo.toml` or `lib.rs`** — if
something is missing there, report it instead. No `git` commands; the parent
commits.

## Build/test environment (exact)

```sh
export CARGO_HOME=/private/tmp/projectos-gate-a-cargo
export RUSTUP_HOME=/private/tmp/projectos-gate-a-rustup
export RUSTUP_TOOLCHAIN=1.97.1     # REQUIRED: the rustup default here is 1.90.0, which this crate rejects (rust-version = 1.97.1)
export PATH=/private/tmp/projectos-gate-a-cargo/bin:$PATH
cd <repo>/kernel
cargo test                          # or: cargo +1.97.1 test
cargo clippy --all-targets          # if clippy is unavailable in the toolchain, say so — do not install
```

## Frozen public API

### `error.rs`
```rust
#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    #[error("io: {0}")] Io(#[from] std::io::Error),
    #[error("invalid sidecar: {0}")] InvalidSidecar(String),
    #[error("unsupported schema: found {found}, expected {expected}")]
        UnsupportedSchema { found: u32, expected: u32 },
    #[error("project is locked by another writer: {0}")] ProjectLocked(String),
    #[error("folder is an alias/duplicate of a registered project: {0}")] AliasPath(String),
    #[error("no project at {0}")] NotRegistered(String),
}
pub type Result<T> = std::result::Result<T, KernelError>;
```

### `sidecar.rs`
```rust
pub const SIDECAR_DIR: &str = ".projectos";
pub const MANIFEST_FILE: &str = "project.toml";
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SidecarEnvelope {
    pub project_uuid: uuid::Uuid,      // identity, excluding envelope? per brief §15: envelope owns UUID
    pub declared_type: String,         // e.g. "writing", "code"
    pub content_roots: Vec<String>,    // relative paths inside the project folder
    pub schema_version: u32,
    pub engine_version: String,
    pub export_version: String,
    pub checkpoint: Option<String>,    // last quiesced checkpoint id, if any
}
impl SidecarEnvelope {
    pub fn read(folder: &std::path::Path) -> Result<Option<Self>>;   // None when no sidecar
    pub fn write(folder: &std::path::Path, env: &Self) -> Result<()>; // atomic: tmp + rename
    pub fn validate(&self) -> Result<()>;                            // schema version, roots are relative, type non-empty
}
```
Rules: `content_roots` are **relative** and must not escape the folder (`..`,
absolute, symlink-resolved escape → `InvalidSidecar`). Writing is atomic
(`project.toml.tmp` then rename). Reading a **newer** `schema_version` is an
`UnsupportedSchema` error, never a silent downgrade.

### `registration.rs`
```rust
pub struct RegisteredProject { pub folder: std::path::PathBuf, pub envelope: SidecarEnvelope }
pub fn register(folder: &Path, declared_type: &str, content_roots: Vec<String>) -> Result<RegisteredProject>;
pub fn open(folder: &Path) -> Result<RegisteredProject>;          // NotRegistered if no sidecar
pub fn move_project(from: &Path, to: &Path) -> Result<RegisteredProject>; // move folder, keep UUID
pub fn is_alias_of_registered(folder: &Path) -> Result<Option<PathBuf>>; // same realpath / case-insensitive alias
```
Rules: identity is the **canonicalized realpath**; registering a folder that is an
alias (symlink, or case-insensitive duplicate) of an already-registered project
must fail with `AliasPath` naming the existing path. **Second-writer block**: a
writer session creates `.projectos/lock` with `create_new(true)` (no extra
crates); a second `open` while the lock is held fails with `ProjectLocked`.
`move_project` must preserve the UUID and update nothing inside `project.toml`
(it travels with the folder).

### `broker.rs`
```rust
#[derive(Debug, Clone, PartialEq)] pub enum Capability { ReadFile(String), WriteFile(String) } // relative paths, granted up front
#[derive(Debug, Clone)] pub struct CapabilitySet(Vec<Capability>);
#[derive(Debug)] pub enum BrokerRequest { ReadFile(String), WriteFile { path: String, bytes: Vec<u8> } }
#[derive(Debug, PartialEq)] pub enum BrokerOutcome { Read(Vec<u8>), Written, Denied(String) }
#[derive(Debug, Clone, PartialEq, serde::Serialize)] pub struct Receipt { pub op: String, pub rel_path: String, pub outcome: String } // redacted: no bytes, no absolute paths

pub struct Broker { /* root + grants + receipts */ }
impl Broker {
    pub fn new(root: PathBuf, grants: CapabilitySet) -> Self;
    pub fn request(&mut self, req: BrokerRequest) -> Result<BrokerOutcome>; // deny => Ok(Denied(_)) + Receipt
    pub fn receipts(&self) -> &[Receipt];
}
```
Rules (**port of the gate-c descriptor-relative pattern**): every path is
resolved **relative to the root**, component by component; reject absolute paths,
`..` components, and any path whose realpath escapes the root (symlink escape) →
`Denied`. A denied request appends a redacted `Receipt` and returns `Denied`; a
missing grant is a denial, never an error. No ambient authority: the module must
not read anything outside the root even when asked. Do not claim OS-level
confinement — this is a *broker*, and confinement is the worker adapter's
(unproven) job.

### `worker.rs`
```rust
pub trait WorkerHost {
    fn start(&mut self, broker: &mut Broker) -> Result<()>;
    fn invoke(&mut self, op: WorkerOp) -> Result<BrokerOutcome>;
    fn cancel(&mut self) -> Result<()>;
    fn receipts(&self) -> Vec<Receipt>;
}
pub struct WorkerOp { pub request: BrokerRequest }
pub struct InProcessWorker { /* holds a cloned broker handle */ }
impl WorkerHost for InProcessWorker { /* every op goes through the broker */ }
```
Rules: the in-process adapter is the **first** implementation of the trait (it
proves the interface, nothing about confinement). Include a doc comment stating
exactly that, and that the WASM guest adapter (wasmtime/javy, Gate C2 evidence)
is the second implementation, deferred until Gate C packaging closes. `cancel`
must be a no-op success in this adapter (documented).

## Acceptance tests (must exist and pass)

Child A — `tests/sidecar_registration.rs`:
1. register → sidecar exists, UUID stable across reopen;
2. re-registering the same folder is idempotent (same UUID, no error);
3. symlink alias of a registered folder → `AliasPath`;
4. `open` on a folder without a sidecar → `NotRegistered`;
5. `move_project` preserves UUID; old path no longer opens;
6. second `open` while locked → `ProjectLocked`;
7. sidecar with `..` in a content root → `InvalidSidecar`;
8. sidecar with a future `schema_version` → `UnsupportedSchema`.

Child B — `tests/broker.rs`:
1. granted read returns bytes; ungranted read → `Denied` + receipt;
2. `../escape`, absolute path, and symlink-escape are all `Denied` (and the file outside the root is not read);
3. write is atomic-ish (bytes land), ungranted write denied;
4. receipts contain no absolute paths and no file bytes;
5. `InProcessWorker` routes every op through the broker (denial behaves identically).

## Reporting

Report: files written, exact commands run, raw pass/fail counts, anything you
could not do and why. Do not claim more than the test run shows. Do not commit.
