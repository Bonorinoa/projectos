# M1 acceptance plan — owner-visible exit criteria

Author of the acceptance tests: the reviewing lead (NOT a subagent — acceptance
tests written by the implementing agents would repeat the correlated-failure
trap the slice-1 review exposed).

Source of truth: `docs/projectos-architecture-brief.md` §18 (M1 exit criteria:
**open → move → recover a project and restore the profile graph; no provider
required**) plus the acceptance items named in the M1 plan and the slice-1 review.

File: `kernel/tests/m1_acceptance.rs`. Every test must be reproducible with
`cargo test --test m1_acceptance` under the pinned toolchain (Rust 1.97.1).

## A1 — headline: open → move → recover restores the project graph

1. tempdir; create `proj/`; `registration::register(proj, "writing", vec![])`.
2. `open(proj)` (writer session) — assert the lock file exists and `lock_owner`
   reports this pid.
3. Build an in-memory graph: ≥4 objects (Draft/Evidence/Receipt/Revision) and ≥3
   typed edges; `journal::checkpoint(...)` → checkpoint id; append ≥1 `Mutated`
   record after the checkpoint (so replay has work to do).
4. Write ≥1 redacted receipt (including one ingested from a real broker denial).
5. Drop the handle (lock released). `move_project(proj, proj_moved)` → UUID
   preserved.
6. `journal::recover(proj_moved, RecoverPolicy::Replay)` → assert:
   `replayed` equals the records written after the checkpoint;
   `lock_taken_over == false`; the snapshot loads back with the **same objects
   and edges** (set equality) and the same UUID; receipts survive the move
   (`receipts()` unchanged, still redacted).
7. `open(proj_moved)` succeeds (no provider, no network, no lock leak).

## A2 — profile graph restore into a clean app-data dir

1. Register two projects; mutate + checkpoint each; record profile entries via
   `ProfileRegistry::open(app_data_a)` (path bindings + last-seen checkpoints).
2. `export(...)` → `restore(from, project_root)` into a **fresh** app-data dir
   `app_data_b` with `project_root` = the parent of the moved folders.
3. Assert: entries count matches; path bindings are re-attached under
   `project_root`; `missing` is empty; each entry's UUID equals the on-disk
   sidecar UUID.
4. **UUID authority**: tamper one project's sidecar UUID on disk, restore again,
   assert the entry is corrected to the sidecar's UUID (sidecar wins — never the
   profile).

## A3 — alias rejection at the registration boundary

1. `register(proj)`; a symlink `link → proj` and (on a case-insensitive volume)
   a case variant must both fail with `AliasPath` naming the registered path.
2. `open()` by canonical path **succeeds** (spelling-agnostic use), asserting the
   documented decision rather than leaving it implicit.

## A4 — second-writer block and explicit takeover

1. `open(proj)` held → a second `open(proj)` fails `ProjectLocked`.
2. Drop the first handle → `open` succeeds (lock released on drop).
3. Simulate a crashed writer (write `.projectos/lock` directly with a stale
   `pid`) → `open` still fails `ProjectLocked` (no automatic recovery) **and**
   `journal::recover(proj, TakeoverStaleLock)` releases it
   (`lock_taken_over == true`), appends a `Recovery` record, after which `open`
   succeeds.

## A5 — OUTCOME_UNKNOWN is recorded and never silently retried

The brief requires the acceptance suite to carry an `OUTCOME_UNKNOWN`
reconciliation **stub**. Scope this pass: lock in the *invariant*, not the
policy.

1. Record a redacted receipt fact whose `outcome` marks an unknown external
   result (`"outcome-unknown"`), plus a journal `Mutated` record for the object
   it concerns.
2. Reopen the project and recover (`Replay`); assert the fact is still present,
   exactly once (no duplicate, no silent retry), and that the same object is not
   re-mutated.
3. Documented TODO in the test: reconciliation *policy* (resolve or abandon)
   lands with the external-effects slice; this test only forbids unsafe retries.

## Verification wrapper (sprint report, not a test)

- `cargo test` → all suites (8+6+9+graph+profile+journal+receipts+acceptance)
  green on the lead's rerun.
- `cargo tree` shows **no provider/network crate** on the kernel's dependency
  path — the "no provider required" criterion, evidenced rather than asserted.
- Lead-authored adversarial tests for the new modules (journal replay/takeover
  edge cases, profile UUID authority, receipt redaction) pass — separate file
  `kernel/tests/m1b_adversarial.rs`.
- App shell: `cargo build` in `app/src-tauri` observed by the lead; UI rendered
  and screenshotted; a11y list view + keyboard path present in the DOM.

## Recorded non-goals for this pass (stated, not hidden)

- Helix fork engine integration (M2), encrypted trace vault, provider transport,
  `tauri build` bundling/signing, Linux matrix, layout persistence in the canvas.
