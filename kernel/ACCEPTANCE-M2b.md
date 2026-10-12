# M2b acceptance plan — owner-visible criteria (draft editor + live provider)

Author of the acceptance criteria: the reviewing lead (NOT the implementing
agent — correlated-failure trap, see `REVIEW-m1-slice.md`).

Source of truth: `docs/EDITOR-PROVIDER-IPC.md` (the app contract) and
`docs/projectos-architecture-brief.md` §14/§18. Interface: `kernel/INTERFACES-M2b.md`.
Review evidence: `kernel/REVIEW-m2b-kernel.md`, `docs/REVIEW-m2b-app-conformance.md`.

Files: `kernel/tests/document.rs`, `kernel/tests/proposal_editor.rs`,
`kernel/tests/m2b_adversarial.rs`; `app/src-tauri/src/main.rs` (inline
`mod integration_tests`); `app/ui/tests/editor-state.test.cjs`. Reproduce under
the pinned toolchain — see `SPRINT-M2b-REPORT.md`.

## Scope of this pass

Kernel-native file store + the app-side editor/provider layer. Live OpenRouter
conformance and the OS-keyring round-trip are **owner-opt-in** and are named, not
claimed.

## A1 — headline: the app-side editing golden path (offline / loopback)

open a workspace → open a draft → select a byte range → `action_prepare`
(frozen model + price + budget ceiling) → execute over the fixed endpoint (loopback
fixture) with streamed, redacted text → review the diff → `action_decide(accept)`
→ the draft bytes change and a revision + redacted, model-disclosing receipt are
recorded. An **annotation** decision leaves the draft bytes intact.

## A2 — the native file store never follows a symlink

`DocumentStore` refuses symlinked roots, files and parents; a root swapped or a
subdirectory swapped for a symlink fails closed and never touches the outside
target; lexical escapes (`..`, absolute, `.projectos/x`, `a//b`, `C:/x`) are
refused; drafts are extension-restricted and size/UTF-8-bounded; no partial write
and no leftover temp sibling on failure.

## A3 — stale writes are refused, before-images are recoverable

`save` with a wrong `expected_hash` is refused and leaves the bytes untouched; the
previous bytes are retained content-addressed under `.projectos/objects/<hash>`;
a corrupt before-image at the exact hash fails **before** any mutation.

## A4 — proposal semantics

`apply(Accept)` on a Replacement splices the anchored bytes and returns a revision
whose `after_hash == stable_hash(whole new document)`; `apply(Accept)` on an
Annotation records an `accepted-annotation` receipt, appends no revision, and
leaves the draft bytes unchanged; `apply(Reject)` mutates nothing and records a
`rejected` receipt; a stale anchor is refused before mutation.

## A5 — offline-by-default and no silent retry

Online is false at startup. `provider_set_online(false)` cancels in-flight work and
never queues or replays a request; the transport disables retries and model
fallback; `resolved_model` must be in the model frozen at approval;
`provider_reconcile` acknowledges an unknown outcome **without** repeating the
request.

## A6 — secret hygiene

The API key never appears in a returned value, error string, receipt, or the
`provider-progress` event; streamed text is redacted even when the secret is split
across SSE events or UTF-8 chunk boundaries. Credentials are written to the OS
keyring only (no plaintext/in-memory fallback).

## A7 — budget

`maxTokens` and `ceilingUsd` are enforced before any request; the price/model are
frozen at `action_prepare` and not refetched at execute; `estimated_upper_usd` is a
conservative upper bound (not a billed claim); billed cost is `null` when unknown.

## Verification wrapper (sprint report, not tests)

- `cargo test --no-fail-fast` (kernel) → 121 passed / 0 failed on the lead's rerun.
- `cargo test` (app) → 19 passed / 2 ignored; `node --test` (UI) → 9 passed.
- M2b files clippy-clean; 11 pre-existing M1/M2 warnings recorded.

## Recorded non-goals for this pass (stated, not hidden)

Live OpenRouter conformance against a real account (priced/billed cost reporting),
the OS-keyring round-trip (`PROJECTOS_TEST_KEYRING=1`), `tauri build`
bundling/signing, the Linux matrix, the Helix `GraphEngine` adapter, Scope capture,
and M0 Gate C (`m0.worker`). M2b is not human-accepted.
