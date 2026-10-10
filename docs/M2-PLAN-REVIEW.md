# M2 plan review — reconciling the brief, the roadmap, and the kernel

**Date:** 2026-10-09 · **Author:** reviewing lead (M2 session)
**Scope:** task 2 of `docs/M2-SESSION-INSTRUCTION.md` — "review M2 against the
brief and the roadmap, and update the plan where reality differs."

## 1. The divergence this review resolves

Two documents call different things "M2":

| Source | What it calls M2 | Status |
|---|---|---|
| `docs/projectos-architecture-brief.md` §18 (product contract) | **Substack golden path** — editor, selected-text actions, draft review recipe, revisions, milestones; complete the eight-step path with a reproduction-oriented receipt | the milestone |
| `design/projectos-live-mvp-roadmap.html` (`m2.*`) | editor · actions · diff/approval/revision · publish-readiness recipe · evidence/lineage lenses · named milestone + export · end-to-end receipt | same milestone, decomposed into 7 checks |
| `kernel/INTERFACES.md` / `INTERFACES-M1b.md` (kernel slice notes) | "in-memory `GraphEngine` (Helix fork is wired in M2)" | **a prerequisite, not the milestone** |

The kernel notes reused "M2" to mean *wire the vendored Helix fork behind the
`GraphEngine` trait*. That is a Gate-B-gated implementation task; it is **not**
the product M2. Recorded decision: the product milestone is the brief's Substack
golden path. Helix-engine wiring stays an independent, Gate-B-gated task and does
not block the M2 product work (the `GraphEngine` trait already isolates it).

## 2. What M2 actually requires, and what blocks it

The eight-step Substack path (brief §14) needs, in order:

1. register / open a project — **done (M1)**;
2. capture Scope — kernel model, not built;
3. open a Markdown draft and select text — `m2.editor`;
4. run a typed selected-text action and review its diff — `m2.actions`, `m2.diff`;
5. run a whole-draft review recipe returning an issue table + evidence map — `m2.recipe`, `m2.context`;
6. accept changes; inspect lineage and the receipt — `m2.diff`, `m1.trace`;
7. create a named publish-candidate milestone — `m2.milestone`;
8. export publishable content — `m2.milestone`.

**Hard dependency:** steps 4 and 5 call a model. **Live provider transport
(Gate E native: credential isolation, OS keyring, live opt-in conformance) is
blocked on owner resources** (credentials, keyring, Linux vault, signing) and is
*not* in this session's controllable set. Therefore M2 is split so the
*kernel-side mechanism* is provable **offline against a deterministic mock
provider**, and the *live* path stays honestly blocked.

## 3. M2 decomposition (slices) and evidence states

| Slice | Contents | Evidence state after this session |
|---|---|---|
| **M2.0 — kernel mechanisms (this session)** | selection anchors; typed selection actions with contracts; proposal → diff → approval → revision loop with before-images and receipts; inspectable bounded recipe + run/cancel; evidence/lineage lens rows; named milestone + portable export; offline end-to-end golden-path test | `implemented` / `verified` (cargo test) per module; e2e as `verified` (offline) |
| **M2.1 — app surface** | wire `app/ui/bridge.js` to the real Tauri commands; artifact editor surface; recipe board; lens/list UI | `planned` |
| **M2.2 — live provider transport** | Gate E native: credential isolation, OS keyring, PKCE, live opt-in, cost ceilings | `blocked` on owner resources |
| **Helix engine wiring** | `HelixGraphEngine` implementing `GraphEngine` against `vendor/helix-db` | `planned` (Gate-B gated; not M2) |
| **Scope capture (step 2)** | typed `Scope` object + premise/audience/promise | `planned` |

## 4. M1 status correction (dashboard was stale)

`projectos-roadmap-status.json` carried only `m0.*` tasks, so the dashboard
showed all `m1.*` as `planned` — **false**: the M1 kernel is committed and
re-runs green (see the M2 sprint report). The roadmap generator
(`scripts/update-roadmap.mjs`) only walked `spikes/` and `scripts/` and only
emitted `m0.*`. Fix applied in this session: the generator now also ingests a
committed `kernel/evidence/status.json` (with per-task source hashes, so a kernel
source change still invalidates the claim) and walks `kernel/` into the source
snapshot.

## 5. Recorded decisions

- **D-M2-a:** product M2 = Substack golden path (brief §18). Kernel "M2" naming
  in the slice notes is reinterpreted as the Helix-wiring task.
- **D-M2-b:** M2.0 proves the mechanisms **offline** with a mock provider; no
  claim of live provider behaviour is made (Gate E native stays blocked).
- **D-M2-c:** no new crate dependencies; persisted artifacts stay TOML;
  hashing stays FNV-1a-64 (documented, non-cryptographic), matching `receipts.rs`.
- **D-M2-d:** `app/` UI wiring (M2.1) is deferred; the mock bridge stays a mock
  and is labelled as such — no silent "native" claim.

## 6. Non-goals restated for this session

Live provider calls, credential storage, `tauri build` bundling, Linux matrix,
Scope capture, the app-side editor, and Helix-engine wiring. Each is named here
rather than absorbed into a "done" claim.
