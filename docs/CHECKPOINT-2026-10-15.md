# ProjectOS checkpoint — October 15, 2026

This is a stable local Software foundation checkpoint for the requested month-long pause, not full M3 completion or owner acceptance. No workers, user-repository test execution, canonical patch application, automatic commit, push, PR, or app-control capability was added to ProjectOS. Publishing this development checkpoint to the existing ProjectOS GitHub remote is a separately authorized maintenance action.

## What is verified

Rust 1.97.1 locked suites: **121 kernel tests, 42 native tests, zero failures**. Two native conformance tests remain explicitly opt-in: OS keyring and live public OpenRouter catalog. **36 frontend/roadmap tests** pass. All UI JavaScript syntax checks, `git diff --check`, and native debug build pass. [Exact hashes, commands and logs](../evidence/m3/report.json) bind the evidence to source bytes. The first sandbox-only native run could not bind local HTTP fixtures; the authorized rerun with loopback access passed. No production inference was purchased.

The actual ProjectOS Tauri app—not the Gate A shell—was exercised on macOS arm64: Software registration and reopen, native folder picker, two-repository staging isolation, review/export, stale-content refusal, disabled Apply/test/app-check controls, keyboard focus, Activity refusal filtering, useful missing-folder feedback, unsaved-draft quit cancellation/discard, and writer-lock release. [Native evidence](../evidence/m3/native-walkthrough.json) identifies the walkthrough passes and binary. This is bounded foundation evidence; `m3.ui`, `m3.effects`, and `m3.e2e` remain planned or blocked.

Receipt hardening preserves the managed run identity, relative changed paths, base HEAD and review fingerprint. Export refuses stale bytes, bad extensions and existing destinations. A successful effect followed by receipt failure reports the partial outcome and forbids blind retry. Fault injection covers those command paths. No `software_apply` exists.

## First permanent project

Local folder: `/Users/bonorinoa/Documents/ChatGPT/AfterDarkPhoenix`.

Bootstrap: `6ff1b97f2b2672f739d91ddbedb863fd21c0ba39`, branch `main`, **no remote**. The eight tracked files include the portable project manifest; runtime graph, receipts, locks and temporary files are ignored. The canonical tracked files remain byte-identical to bootstrap. Native operation receipts update ignored sidecar metadata, not canonical content. The app is closed and its writer lock released.

Proposal: `/Users/bonorinoa/Documents/ChatGPT/AfterDarkPhoenix-review.patch`. It contains the 1,305-word “Heat After Sunset” article, outline status, and an explicit research-blocker audit. It was populated only in native-created staging, reviewed and exported by ProjectOS. The patch passes `git apply --check --whitespace=error`; **it was not applied to the canonical checkout**. Review content fingerprints and exported-file SHA256 are different hashes over different representations; the report compares matching review/export receipt fingerprints and separately records file SHA256. Intermediate exports were retained under `AfterDarkPhoenix-review-initial-national.patch` and `AfterDarkPhoenix-review-ledger-initial.patch`, not deleted.

Staging directories are temporary and may disappear after reboot. The exported patch is the durable proposal. Do not reopen a staging checkout as a separate ProjectOS project. Native run IDs are session-owned; create a fresh managed worktree on resume and populate only that staging checkout if reconstruction is necessary. Changed worktree cleanup is not automated in this foundation.

## Research blocker and budget

Treg CLI login and MCP connection work without provider API keys. Successful retained calls cost **$0.092** ($0.002 Phoenix/English SERP, $0.09 national/English keyword batch). The source ledger explicitly distinguishes institutional factual evidence, uncertain claims and pinned GitHub implementation examples.

The keyword batch mistakenly used U.S. geography. The owner approved one $0.09 Phoenix correction, raising the authorized ceiling to **$0.182**. The correction attempt returned Treg HTTP 503 `provider_capacity_unavailable`; **nothing was charged**. [Raw refusal and exact parameters](../evidence/m3/treg-phoenix-correction.json) are retained and included in the proposal. Treg reported expected capacity around October 15, 3:04 p.m. Phoenix time; that is a provider estimate, not a promise. Country-only alternatives do not meet the required Phoenix-city and English inputs. No silent fallback or provider-key request was made.

Phoenix-only keyword research remains incomplete. Before the next paid call, recheck balance, endpoint price and capacity. Preserve the existing refusal; do not replay a known refusal as if its result were unknown. A new retrieval needs a new canonical parameter/request identity, while unknown-result retries reuse their original identity. Keep additional settled spending at or below the approved $0.09. Replace no raw evidence and do not present `null` as zero.

## Design and visual QA

The [System Design DOCX](ProjectOS-M3-First-Project-System-Design.docx) was cloned from the retained template, patched in editable slots, rendered to seven pages and inspected page by page. The task-local [template contract](../evidence/m3/design/artifact.md), package audits and navy/light-blue architecture source are retained. The reference remains SHA256 `13504f6c221a42c1726460a9e865e563355539ff97d702d6c9b2267b4b261d76`. Styles, numbering, fonts, relationship identities, headers, section setup and table geometry are preserved. Documented exceptions are cover spacers/page break for the long title and the existing placeholder hyperlink's target, updated to the public source contract.

Impeccable guided narrow error-state and accessibility refinements: preserve the incumbent palette, improve functional text contrast/size, expose a modal alert, and make refusal filtering include Software outcomes. No product redesign was introduced. The architecture figure uses the template's navy/light-blue system and remains readable in grayscale.

## Known limitations

- Gate C still requires signing/notarization resources, Linux evidence and enforceable worker/computer-use boundaries. No full Software golden path, central offline/egress proof, canonical recovery integration or release matrix is claimed.
- Live OpenRouter and real OS-keyring conformance require owner opt-in. These are not counted as passing tests.
- A temporary unsigned debug `.app` launched through LaunchServices stalled while opening the Documents folder; direct developer-terminal launch worked. The stalled process was terminated and its confirmed-stale lock moved recoverably to a temporary audit folder. No permission protection was disabled. Signed packaged launch remains unverified; use the development command below.
- The repository arrived at published snapshot `e7baf5a` with M2b/M3 history and tracked `.projectos` metadata already combined. Existing published commits were not rewritten, and `.projectos` was neither restaged nor deleted. New fixes and evidence are separate focused commits. Historical M2b evidence retains its original scope.
- Owner manual acceptance remains the final gate. Agent-written roadmap status never sets `accepted`.

## Resume after the month-long pause

No recurring monitor, background worker or future paid research job is scheduled. Start by reading this guide and the source-bound roadmap, then inspect local changes before doing anything.

```sh
git status --short
git log -5 --oneline
rustup run 1.97.1 cargo --version
node scripts/verify-checkpoint.mjs
node scripts/update-roadmap.mjs
```

Activate the pinned toolchain on PATH first; `PROJECTOS_CARGO` can name its cargo executable. For the current machine, cargo is under `/Users/bonorinoa/.rustup/toolchains/1.97.1-aarch64-apple-darwin/bin/`; Node is available in the bundled Codex runtime. Local HTTP fixtures need loopback permission. The roadmap command deliberately returns nonzero if historical evidence is stale; inspect diagnostics instead of weakening the gate or refreshing hashes without checks.

For native development from the repository root:

```sh
cargo +1.97.1 run --locked --manifest-path app/src-tauri/Cargo.toml
```

Reopen After Dark Phoenix as Software, verify bootstrap integrity, inspect the exported proposal, then resolve the Treg capacity blocker within the approved budget. Never auto-apply the proposal. Revisit full M3 only after owner prioritization and Gate C resources. A month away does not itself authorize new capabilities or spending.
