# M2b sprint report — draft editor + live provider integration (2026-10-11)

Author: reviewing lead. Method: M2b was carried by a Codex session that ran out of
budget mid-sprint. Its work was snapshotted verbatim (`8347f90`), then
**independently reviewed** in parallel by two subagents (kernel + app), and every
claim below was re-run by the lead on the committed tree. Subagent reports were
treated as claims, not facts.

Acceptance plan: `ACCEPTANCE-M2b.md`. Kernel review: `kernel/REVIEW-m2b-kernel.md`.
App conformance audit: `docs/REVIEW-m2b-app-conformance.md`.
Contract: `kernel/INTERFACES-M2b.md` + `docs/EDITOR-PROVIDER-IPC.md`.

Milestone note: this slice was first labelled "M3" (commit `8347f90`). Per brief
§18 the canonical **M3 is the Software vertical** (repository adapter, worktree
workflow, patch review) — it has **no implementation** and remains unstarted. The
label was corrected to **M2b** = M2.1 + M2.2; the `m3.*` task ids injected into
the roadmap were renamed to `m2.*`, and the `m3.ui` id (which collided with the
roadmap's own `m3.ui`, "constrained computer-use checks") was removed.

## What shipped

| Commit | Contents |
|---|---|
| `8347f90` | M2b work-in-progress snapshot (Codex handoff): `document.rs`, revised `proposals.rs`, the whole `app/src-tauri/src/{provider_transport,credentials,workspace,main}.rs` layer, the native UI bridge/editor, vendored fonts, `docs/EDITOR-PROVIDER-IPC.md` |
| `5265d0e` | fix: refuse the `objects/` subtree in metadata paths (review Finding 1) + three clippy lints in `document.rs` |
| `8052b94` | reviewer suite `kernel/tests/m2b_adversarial.rs`, `REVIEW-m2b-kernel.md`, `docs/REVIEW-m2b-app-conformance.md`, `docs/adr-kernel-native-fs-syscalls.md` |
| *(this commit)* | `INTERFACES-M2b.md`, this report, `ACCEPTANCE-M2b.md`, ADR ratified Accepted, `kernel/evidence/status.json` (m2 hashes refreshed + `m2.*` added), regenerated roadmap manifest |

## Verification (lead reruns, pinned Rust 1.97.1)

`cargo test --no-fail-fast` → **121 passed · 0 failed**. Per suite:

| Suite | Tests | Kind |
|---|---|---|
| `actions` | 7 | M2 module |
| `adversarial` | 9 | M1 reviewer |
| `broker` | 6 | M1 contract |
| `document` | 6 | **M2b module (new)** |
| `graph` | 6 | M1 module |
| `journal` | 8 | M1 module |
| `lens` | 5 | M2 module |
| `m1_acceptance` | 5 | M1 reviewer exit criteria |
| `m1b_adversarial` | 5 | M1 reviewer round 2 |
| `m2_acceptance` | 4 | M2 reviewer exit criteria |
| `m2_adversarial` | 9 | M2 reviewer round |
| `m2b_adversarial` | 18 | **M2b reviewer round (new)** |
| `milestone` | 4 | M2 module |
| `profile` | 5 | M1 module |
| `proposal_editor` | 5 | **M2b module (new)** |
| `receipts` | 5 | M1 module |
| `recipe` | 6 | M2 module |
| `sidecar_registration` | 8 | M1 contract |

App crate `cargo test` → **19 passed · 2 ignored** (owner-opt-in: OS-keyring,
network). UI `node --test` → **9 passed**.

`cargo clippy` was previously recorded "unavailable"; it is installable under the
pinned toolchain (`rustup component add clippy`). After the fix commit, **M2b files
are clippy-clean**; **11 pre-existing** warnings remain in M1/M2 files
(`worker.rs`, `selection.rs`, `registration.rs`, `journal.rs`, three test files) —
recorded, not part of M2b.

## The headline proof

The app-side golden path is implemented end to end and covered by the app crate's
inline integration tests: open a workspace → open/create a draft → anchor a byte
range → `action_prepare` (frozen price + budget, exact model) → execute over the
fixed endpoint (loopback fixture in tests) → stream redacted text → review the
diff → `action_decide(accept)` → the draft bytes change, a revision and a
**redacted, model-disclosing receipt** are recorded, and an **annotation** decision
leaves the draft bytes intact. The kernel reviewer suite independently re-proves
the file-store properties: symlinked root/file/parent refused, root-swap and
directory-swap race fail closed with the outside target untouched, lexical escapes
and size/UTF-8 bounds refused, stale-write refused with bytes untouched, and the
before-image retained content-addressed.

## Findings (found by review, not by the implementer)

1. **Metadata `objects/` scope leak — real, contained, fixed.** `metadata_path`
   admitted any `.toml` name, so `write_metadata("objects/probe.toml")` reached the
   reserved before-image store. No escape (still inside `.projectos`, blobs are
   16-hex with no `.toml` extension, `read_revision_blob` re-verifies the hash), but
   it violated the stated contract. Fixed in `5265d0e`; the reviewer's failing test
   now passes.
2. **`libc` added, crossing recorded decision D-M2-c.** The unsafe syscall code
   (`openat/mkdirat/renameat/unlinkat/fdopendir/readdir/closedir` in
   `document.rs`) was reviewed and judged **sound** (`O_CLOEXEC|O_NOFOLLOW`
   everywhere, `CString` NUL discipline, explicit `fdopendir` failure handling,
   `O_EXCL` temp + `renameat`). Carried to
   `docs/adr-kernel-native-fs-syscalls.md`; **owner-approved 2026-10-11**.
3. **The `m2_acceptance.rs` assertion change is a correction, not a weakening.**
   `proposal.after_hash` is the hash of the replacement snippet; `revision.after_hash`
   is the hash of the **whole** rebuilt document, so the original assertion could
   never have held. The change asserts the documented property; the reviewer suite
   re-proves it independently.
4. **Observation (untested).** `Proposal::new` derives `id` without `kind`, so two
   proposals differing only in `kind` share an `id`. Not exploitable (proposed and
   original are fully validated); noted for downstream consumers.

## Model provenance

| Work | Model | Outcome |
|---|---|---|
| M2b implementation (`document.rs`, `proposals.rs` revision, whole app layer, UI) | Codex session | Substantial and largely correct; ran out of budget before the contract/report; introduced the `objects/` leak + `libc` deviation |
| Kernel adversarial review + `REVIEW-m2b-kernel.md` | `deepseek/deepseek-v4-flash-0731` (subagent) | 18 tests; found Finding 1; correctly judged F3; 1 real bug |
| App conformance audit + `REVIEW-m2b-app-conformance.md` | `deepseek/deepseek-v4-flash-0731` (subagent) | 9/9 checks PASS; all 17 commands PASS; 2 unproven areas named |
| Fix, contract, report, acceptance plan, status.json, ADR, roadmap | lead | parent-owned |

Driver policy: per the standing rule, the paid driver (`deepseek/deepseek-v4-flash-0731`)
was selected for M2b (security-adjacent, multi-crate) and set with
`hermes config set delegation.model`; a 1-token live call verified it before
dispatch.

## Explicit non-goals (stated, not hidden)

Live OpenRouter conformance against a real account (priced/billed cost reporting),
the OS-keyring round-trip under `PROJECTOS_TEST_KEYRING=1`, `tauri build`
bundling/signing, the Linux matrix, the Helix `GraphEngine` adapter, Scope capture,
and M0 Gate C (`m0.worker`) remain open or owner-gated. M2b is **not** human-accepted.

## Reproduction

```sh
export CARGO_HOME=/private/tmp/projectos-gate-a-cargo
export RUSTUP_HOME=/private/tmp/projectos-gate-a-rustup
export RUSTUP_TOOLCHAIN=1.97.1
export PATH=/private/tmp/projectos-gate-a-cargo/bin:$PATH
cd kernel && cargo test --no-fail-fast            # 121 passed / 0 failed
cd ../app/src-tauri && cargo test                  # 19 passed / 2 ignored
cd ../ui && node --test                            # 9 passed
cd ../.. && node scripts/update-roadmap.mjs        # regenerate the manifest (exit 1 = the pre-existing m0.worker diagnostic)
node scripts/test-roadmap.mjs                      # see the test-roadmap output
```
