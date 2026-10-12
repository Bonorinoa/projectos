# M2b kernel slice — interface contract (frozen for this pass)

M2b is the **app-side completion of M2** — draft editor + live provider
integration (app contract: `docs/EDITOR-PROVIDER-IPC.md`). It builds the two
pieces M2 named but deferred in `docs/M2-PLAN-REVIEW.md`: the app-side editor
surface (**M2.1**) and the live OpenRouter path (**M2.2**).
Ordinary UTF-8 Markdown/text files remain canonical; native commands hold the
project writer session; renderer calls never supply API endpoints or receive
stored credentials.

This file is **recorded from the shipped implementation** — the M2b sprint was
carried by a Codex session that ran out of budget before freezing the contract.
It documents the kernel surface M2b introduced; the app/UI surface is specified by
`docs/EDITOR-PROVIDER-IPC.md`.

Read first: `docs/EDITOR-PROVIDER-IPC.md`, `docs/adr-kernel-native-fs-syscalls.md`
(the accepted `libc` decision), and the M2b review `kernel/REVIEW-m2b-kernel.md`.

## Crate layout (this slice)

```
kernel/src/
├── document.rs     # native DocumentStore: pinned root, O_NOFOLLOW openat, atomic replace  [core, new]
└── proposals.rs    # Proposal/Decision/Revision routed through DocumentStore               [core, revised]
app/src-tauri/src/
├── provider_transport.rs  # fixed-endpoint OpenRouter transport                            [app]
├── credentials.rs         # OS-keyring credential boundary                                 [app]
├── workspace.rs           # single native writer session                                   [app]
└── main.rs                # ~17 Tauri commands + inline integration tests                  [app]
app/ui/
├── bridge.js              # native-only renderer bridge (no mock)                          [ui]
└── editor.js, editor-state.js, editor.css, index.html                                      [ui]
```

Ownership: `document.rs` + `proposals.rs` are the parent-owned frozen core.
`libc` is the one added dependency and is confined to `document.rs`
(`docs/adr-kernel-native-fs-syscalls.md`, Accepted 2026-10-11). Hashing stays
**FNV-1a-64 → 16 lowercase hex** (`crate::provider::stable_hash`); persisted
metadata is **TOML** under `.projectos/`.

## Build/test environment (exact)

```sh
export CARGO_HOME=/private/tmp/projectos-gate-a-cargo
export RUSTUP_HOME=/private/tmp/projectos-gate-a-rustup
export RUSTUP_TOOLCHAIN=1.97.1          # default here is 1.90.0; the crate requires 1.97.1
export PATH=/private/tmp/projectos-gate-a-cargo/bin:$PATH
cd kernel && cargo test --no-fail-fast              # 121 passed / 0 failed
cd app/src-tauri && cargo test                       # 19 passed / 2 ignored
cd app/ui && node --test                             # 9 passed
```

`cargo test` is fail-fast across targets — use `--no-fail-fast` for the full
picture. `clippy` is not in the pinned toolchain until `rustup component add
clippy` is run.

## Frozen kernel core

### `document.rs` — `DocumentStore`

```rust
pub const MAX_DOCUMENT_BYTES: usize = 1024 * 1024;
pub struct DocumentStore { /* holds a pinned root directory descriptor */ }
impl DocumentStore {
    pub fn open(root: &Path) -> Result<Self>;
    pub fn read(&self, rel: &str) -> Result<String>;
    pub fn create(&self, rel: &str) -> Result<()>;
    pub fn save(&self, rel: &str, expected_hash: &str, text: &str) -> Result<()>;
    pub fn read_metadata(&self, rel: &str) -> Result<Option<String>>;
    pub fn write_metadata(&self, rel: &str, text: &str) -> Result<()>;
    pub fn read_revision_blob(&self, hash: &str) -> Result<Vec<u8>>;
    pub fn list_text_files(&self) -> Result<Vec<String>>;
}
```

Rules:

- Every descendant is opened `openat(dirfd, name, O_NOFOLLOW | O_CLOEXEC)`,
  walking from the **pinned root**. A replaced root, a swapped subdirectory, or
  any symlinked component fails closed and never touches the outside target.
- Draft paths are relative, non-escaping, extension-restricted
  (`.md` / `.markdown` / `.txt`). `create` uses `O_EXCL` (never overwrites).
- Metadata paths are `.projectos`-relative, must end `.toml`, and must **not**
  contain an `objects` component (that subtree is reserved for content-addressed
  before-images — M2b review Finding 1).
- `save` carries a stale-write guard: it writes a sibling temp `O_EXCL`, verifies
  the current bytes against `expected_hash` in the compare window, then `renameat`s
  atomically; the previous bytes are retained content-addressed under
  `.projectos/objects/<hash>`. Documented caveat: the final compare/rename window
  is **not** a lock against an uncooperative concurrent writer.

### `proposals.rs` (revised)

```rust
pub enum ProposalKind { Replacement, Annotation }
pub struct Proposal { pub id, rel_path, kind, original, proposed, before_hash, after_hash }
pub enum Decision { Accept, Reject }
pub struct Revision { pub id, proposal_id, rel_path, before_hash, after_hash, at }
pub struct RevisionLog { /* backed by DocumentStore; .projectos/revisions.toml */ }
pub fn apply(folder: &Path, anchor: &DocAnchor, p: &Proposal, d: Decision) -> Result<Option<Revision>>;
```

Change from M2: `apply` and `RevisionLog` route **all** byte/metadata I/O through
`DocumentStore` (no ad-hoc `std::fs`).

- `apply(Accept)` on a **Replacement** splices the anchored byte range and returns
  a `Revision` whose `after_hash == stable_hash(whole new document)`.
- `apply(Accept)` on an **Annotation** stores a linked artifact and leaves the
  draft bytes intact, returning `None`.
- `apply(Reject)` mutates nothing and records a `rejected` receipt.

Consequence: the M2 acceptance assertion in `m2_acceptance.rs` was corrected to
compare `rev.after_hash` against `stable_hash(new_text)` — the documented
property — which is a correction, not a weakening (`REVIEW-m2b-kernel.md` F3).

## App surface

Specified by `docs/EDITOR-PROVIDER-IPC.md`: 17 renderer commands with snake_case
result fields / camelCase invoke args, the `Document` / `Workspace` / `Object` /
`Edge` / `Receipt` / `ProposalResult` shapes, and the BYOK + offline-by-default +
frozen-budget guarantees. Implementation lives in
`app/src-tauri/src/{main.rs, workspace.rs, provider_transport.rs, credentials.rs}`;
the renderer bridge (`app/ui/bridge.js`) is native-only and refuses to impersonate
a project in a browser preview.

## Reporting (as built)

Files written; exact commands; raw pass/fail counts; assumptions; anything not
done and why. Every `verified`/`implemented` claim in
`kernel/evidence/status.json` is bound to exact source hashes; a changed file
blocks the claim.
