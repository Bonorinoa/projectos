# M1 sprint report — completed and verified (2026-10-09)

Author: reviewing lead. Method: every claim below was re-run by the lead on the
committed tree; subagent self-reports were treated as claims, not evidence.
Acceptance plan: `ACCEPTANCE-M1.md`. Slice-1 review: `REVIEW-m1-slice.md`.

## Delivered

| Commit | Contents |
|---|---|
| `1f05d20` | slice 1 — error, sidecar, registration, broker, `WorkerHost` (+ reviewer adversarial suite) |
| `a85f43b` | Geist + Space Mono vendored (OFL-1.1, hash manifest) |
| `8b851e2` | slice-2 scaffold + frozen `INTERFACES-M1b.md` + lock-owner metadata |
| `5b1162f` | lead-authored `ACCEPTANCE-M1.md` |
| *(this commit)* | graph · profile · journal · receipts · app shell + acceptance suite + adversarial round 2 + this report |

Modules: `graph.rs`/`graph_memory.rs` (GraphEngine trait + in-memory adapter,
cycle-safe lineage, byte-stable TOML snapshots), `profile.rs` (installation
registry, export/restore, sidecar-UUID authority), `journal.rs` (append-only
records, quiesced checkpoints, replay, explicit stale-lock takeover),
`receipts.rs` (durable redacted facts, broker-denial ingestion),
`app/` (Synodos Field canvas + a11y list-equivalent + standalone Tauri v2 crate).

## Verification (lead reruns, pinned Rust 1.97.1)

| Suite | Tests | Result |
|---|---|---|
| `adversarial` (slice 1, reviewer) | 9 | pass |
| `broker` (contract) | 6 | pass |
| `graph` | 6 | pass |
| `journal` | 8 | pass |
| `m1_acceptance` (**reviewer, exit criteria**) | 5 | pass |
| `m1b_adversarial` (reviewer, round 2) | 5 | pass |
| `profile` | 5 | pass |
| `receipts` | 5 | pass |
| `sidecar_registration` | 8 | pass |
| **total** | **57** | **57 pass · 0 fail · 0 warnings** |

- **M1 exit criteria (brief §18)** proven by `m1_acceptance.rs`: A1 open → move →
  recover with the checkpointed graph restored **byte-identically** and receipts
  intact; A2 profile restore into a **clean** app-data dir with sidecar-UUID
  authority; A3 alias rejection + spelling-agnostic `open()`; A4 second-writer
  block, release-on-drop, and recorded stale-lock takeover; A5 `OUTCOME_UNKNOWN`
  recorded exactly once with no silent retry.
- **"No provider required"** evidenced, not asserted: `cargo tree` shows the
  kernel depends only on `serde`, `thiserror`, `toml`, `uuid` (dev: `tempfile`).
- **App shell**: `cargo build` in `app/src-tauri` → `Finished dev profile`, and
  the standalone crate links the real kernel by path.
- **UI**: rendered headlessly by the lead to `app/ui/preview/canvas.png`; DOM audit
  confirms the a11y list table with its rows, the canvas nodes, the inspector,
  the list toggle, and **zero remote/CDN references** (fonts local).

## Findings (found by review, not by the implementers)

1. **False audit fact in recovery** — `recover(TakeoverStaleLock)` reported
   `lock_taken_over = true` even when no lock file existed. Caught by
   `m1b_adversarial::adv_recovery_does_not_claim_takeover_without_a_lock`;
   fixed by the lead (report a takeover only when a lock was actually present).
2. **UI child's `timeout` trap** — macOS has no `timeout`, so its first build
   attempt never ran while the pipeline still exited 0. Caught and steered; the
   re-run (and the lead's own build) is the evidence that counted.
3. **Documented behaviour, asserted rather than hidden**: receipt ids are
   content-addressed by `(at, op, rel_path)`, so two identical denials in the
   same instant share an id while both facts are stored
   (`adv_identical_receipts_share_an_id_but_both_are_stored`).
4. **Documented assumptions**: profile `restore` re-binds by basename and counts
   identity *corrections* in `rebound`; a present-but-unreadable sidecar is
   treated as missing (identity cannot be verified).
5. **App-shell deviations, recorded**: the UI agent generated RGBA icons because
   `tauri::generate_context!` requires one (benign, excluded from bundling);
   `innerHTML` lint hits are justified only because every interpolated record is
   static mock data — dynamic data must move to `textContent` when the canvas is
   wired to the real kernel.

## Explicit non-goals for this pass (stated, not hidden)

Helix vendored-fork engine integration (M2), encrypted trace vault, provider
transport, `tauri build` bundling/signing/notarization, Linux matrix, canvas
layout persistence, and reconciliation *policy* for `OUTCOME_UNKNOWN` (only the
no-silent-retry invariant is locked in).

## Model provenance

All five slice-2 workstreams ran on `deepseek/deepseek-v4-flash-0731` (the driver
upgraded after the slice-1 escape); slice 1's modules were written by
`poolside/laguna-s-2.1:free` except the broker fix and all reviewer tests/fixes.
Every suite above was re-run by the lead; no result is a subagent self-report.
