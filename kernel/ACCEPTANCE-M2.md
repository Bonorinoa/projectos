# M2 acceptance plan — owner-visible criteria (Substack golden-path mechanisms)

Author of the acceptance tests: the reviewing lead (NOT the implementing agent —
correlated-failure trap, see `REVIEW-m1-slice.md`).

Source of truth: `docs/projectos-architecture-brief.md` §18 (**M2 = Substack
golden path**) and §8/§9/§13/§14, plus `docs/M2-PLAN-REVIEW.md` (why M2 is the
Substack path and why M2.0 proves the mechanisms offline).

File: `kernel/tests/m2_acceptance.rs`. Reproduce with
`cargo test --test m2_acceptance` under the pinned toolchain (Rust 1.97.1).

## Scope of this pass (M2.0)

Kernel mechanisms only, **offline** against the deterministic `MockProvider`.
Live provider transport (M2.2) and the app-side editor (M2.1) are out of scope
and are named, not claimed.

## A1 — headline: the offline Substack golden path

register a writing project → open → select a passage → run `Rephrase` through the
mock provider → inspect the proposal + diff → record a redacted action receipt →
ingest a real broker denial as a durable fact → **accept** (atomic replace,
recoverable content-addressed before-image, journal `Mutated`, redacted receipt,
revision) → run the publish-readiness recipe (inspect, pause at the human gate,
resume, complete) → build a graph + checkpoint → create a **named milestone** →
one-hop lens over the graph → **portable export** → recover. Assertions: the
artifact holds the proposal; the before-image restores the original bytes; the
receipt discloses the resolved route and leaks **no** absolute path and **no**
content bytes; the accepted fact survives recovery exactly once.

## A2 — a stale selection is never silently applied

Anchor, then edit the document externally. Both running the action **and**
applying a pre-built proposal must error; the file is unchanged.

## A3 — a rejection mutates nothing

`apply(Reject)` returns no revision, changes no bytes, records a `rejected`
receipt, and appends no revision.

## A4 — the action contracts are typed

`Rephrase`/`Complete` → `Replacement` (read-only false); `Explain`/`FactCheck` →
`Annotation` (read-only true).

## Verification wrapper (sprint report, not tests)

- `cargo test` → all suites green on the lead's rerun (see `SPRINT-M2-REPORT.md`).
- `cargo clippy --all-targets` is **unavailable** for the pinned toolchain — said
  so, not installed.

## Recorded non-goals for this pass (stated, not hidden)

Live provider transport (M2.2, blocked on owner resources), app-side editor +
native bridge wiring (M2.1), Scope capture (golden-path step 2), the Helix
`GraphEngine` adapter, `tauri build` bundling/signing, the Linux matrix, and
`OUTCOME_UNKNOWN` reconciliation *policy*.
