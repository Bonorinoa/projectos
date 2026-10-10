# M2 sprint report — Substack golden-path mechanisms (2026-10-09)

Author: reviewing lead. Method: every claim below was re-run by the lead on the
committed tree; the implementing agent's self-report was treated as a claim.
Acceptance plan: `ACCEPTANCE-M2.md`; scope reconciliation: `docs/M2-PLAN-REVIEW.md`.

## What shipped

| Commit | Contents |
|---|---|
| `d4f1a89` | M2 reviewer adversarial round (9 tests) + the M2 session instruction doc |
| `d1136cb` | M2 slice definition (`docs/M2-PLAN-REVIEW.md`, `kernel/INTERFACES-M2.md`) + parent core (`provider.rs`, `selection.rs`, `proposals.rs`) + child stubs |
| `3b5ec9e` | M2 modules: `actions.rs`, `recipe.rs`, `milestone.rs`, `lens.rs` + their suites + the offline acceptance suite `m2_acceptance.rs` |
| *(roadmap commit)* | `kernel/evidence/status.json` + generator/serve changes + regenerated manifest/snapshot/HTML |

## Verification (lead reruns, pinned Rust 1.97.1)

`cargo test` → **92 passed · 0 failed · 0 warnings**. Per suite:

| Suite | Tests | Kind |
|---|---|---|
| `actions` | 7 | module (child A) |
| `adversarial` | 9 | M1 reviewer |
| `broker` | 6 | M1 contract |
| `graph` | 6 | M1 module |
| `journal` | 8 | M1 module |
| `lens` | 5 | module (lead) |
| `m1_acceptance` | 5 | M1 reviewer exit criteria |
| `m1b_adversarial` | 5 | M1 reviewer round 2 |
| `m2_adversarial` | 9 | **M2 reviewer round** |
| `milestone` | 4 | module (lead) |
| `profile` | 5 | M1 module |
| `receipts` | 5 | M1 module |
| `recipe` | 6 | module (lead) |
| `sidecar_registration` | 8 | M1 contract |
| `m2_acceptance` | 4 | **M2 reviewer exit criteria** |

- `cargo clippy --all-targets` → **unavailable** for toolchain 1.97.1 (recorded;
  not installed, per the frozen build environment).
- The M1 slice was re-verified from a clean tree: the 13 frozen contract tests in
  `kernel/INTERFACES.md` (8 sidecar/registration + 5 broker) all pass.

## The headline proof

`m2_acceptance::a1_substack_golden_path_offline` runs the eight-step Substack path
end to end **offline** (no network, no credentials): select → typed action → diff
→ accept → revision with a **recoverable before-image** → recipe (pause at the
human gate, resume) → named milestone → lens → portable export → recovery, with a
redacted receipt that **discloses the resolved model route** and leaks no absolute
path or content bytes.

## Findings (found by review, not by the implementer)

1. **`RecipeRun` needed one private field** beyond the frozen struct: `advance`/
   `approve` must know whether the current block is an approval gate, which the
   frozen public fields do not carry. Recorded deviation: a private
   `blocks: Vec<RecipeBlock>` was added; behaviour and the public surface match
   the contract. (The M1 contract lets the parent own the frozen surface; this is
   noted rather than silently absorbed.)
2. **Doc-comment typo** in the child-authored `actions.rs` ("refused,never") —
   cosmetic, fixed by the lead.
3. **Roadmap generator was blind to the kernel.** `projectos-roadmap-status.json`
   had carried only `m0.*`, so the dashboard showed every `m1.*` as `planned`
   while the M1 kernel was committed and green. The generator now walks `kernel/`
   into the source snapshot and ingests `kernel/evidence/status.json`, binding
   each M1/M2 claim to exact source hashes (a changed file blocks the claim).
4. **M2 naming collision resolved.** The kernel slice notes used "M2" for Helix
   wiring; the product M2 is the Substack golden path. Documented in
   `docs/M2-PLAN-REVIEW.md` (D-M2-a).

## Model provenance

| Work | Model | Outcome |
|---|---|---|
| `actions.rs` + `tests/actions.rs` | `deepseek/deepseek-v4-flash-0731` | 7/7 predicted tests; clean, contract-faithful; one cosmetic typo |
| `provider.rs`, `selection.rs`, `proposals.rs`, `lib.rs`, `INTERFACES-M2.md`, stubs | lead | parent core (frozen) |
| `recipe.rs`, `milestone.rs`, `lens.rs` + suites | lead | delegation budget capped at 2 children for this run; implemented inline |
| `m2_acceptance.rs`, `m2_adversarial.rs`, `status.json`, generator changes, this report | lead | reviewer |

Driver policy: the M1 review showed the free model shipped a symlink write-escape,
so the paid driver (`deepseek/deepseek-v4-flash-0731`) was selected per the
standing rule and set with `hermes config set delegation.model`.

## Explicit non-goals (stated, not hidden)

Live provider transport (M2.2 — blocked on owner credentials / OS keyring /
live opt-in), the app-side editor and native bridge wiring (M2.1), Scope capture,
the Helix `GraphEngine` adapter, `tauri build` bundling/signing, the Linux matrix,
and `OUTCOME_UNKNOWN` reconciliation policy. M1 remains unreleased until the
owner's human acceptance.

## Reproduction

```sh
export CARGO_HOME=/private/tmp/projectos-gate-a-cargo
export RUSTUP_HOME=/private/tmp/projectos-gate-a-rustup
export RUSTUP_TOOLCHAIN=1.97.1
export PATH=/private/tmp/projectos-gate-a-cargo/bin:$PATH
cd kernel && cargo test                                   # 92 passed / 0 failed / 0 warnings
node scripts/update-roadmap.mjs                           # regenerate the manifest (exit 1 = the pre-existing m0.worker diagnostic)
node scripts/test-roadmap.mjs                             # 10/10
```
