# VENDORED FORK — HelixDB @ c72e64f (ProjectOS)

**Upstream:** https://github.com/HelixDB/helix-db
**Revision:** `c72e64f0c7e480f3d125f071659d5e3ad7b8da15`
**Vendored:** 2026-10-09, via `git archive HEAD` of the pinned rev (checkout `/private/tmp/projectos-helix-c72e64f`).
**License:** Apache-2.0 (upstream `LICENSE` retained).

## Why this fork exists

Gate B's hard-fail screen flagged one license-incompatible crate in the dependency
graph: `helix-metrics` (declared `AGPL-3.0` in `crates/metrics/Cargo.toml:7`), a
**mandatory, non-optional** dependency of the `db` crate (upstream
`crates/db/Cargo.toml:14`) with no feature toggle — it cannot be excluded by any
build flag. Analysis: `evidence/gates/gate-b/license-screen-helix-metrics.md`.
The rest of the upstream tree at this rev is Apache-2.0; six workspace crates had
missing `license` metadata (a metadata gap, not a license gap).

## What changed vs upstream

1. **`crates/metrics` deleted** and removed from workspace `members`. It is an
   unused-by-default telemetry transport (runtime opt-in; `start_oss_from_env`
   returns `Ok(None)` without persisted privacy settings — see upstream
   `crates/metrics/src/query/transport.rs`). Product behaviour with the
   transport present-but-disabled is identical.
2. **`crates/db/Cargo.toml`**: dropped the `helix-metrics` dependency line.
3. **`crates/db/src/embedded_metrics.rs` added**: inert stand-ins
   (`OssQueryMetrics`, `Runtime`, `TenantId`, `QueryErrorType`) preserving the
   signatures that referenced upstream metrics types.
4. **`crates/db/src/lib.rs`**: metrics imports removed; `with_embedded_query_metrics`
   is now a no-op returning `self`; the metrics field/shutdown slot is emptied
   without an upstream runtime; the metrics-specific unit test was removed.
5. **`crates/db/src/query_service.rs`**: the `QueryObservation` telemetry
   payload builder (which reimplemented upstream canonical/redaction logic) and
   its call sites were removed along with the three telemetry-only tests. The
   `From<QueryFailureClass> for QueryErrorType` mapping and its tests are kept
   against the local `QueryErrorType`.
6. **`crates/db/tests/production_index_operation_queue.rs`**: imports the local
   `QueryErrorType`.
7. **`license = "Apache-2.0"`** added to the six crates whose manifests lacked
   `license` metadata (`db`, `helix-ast`, `helix-ast-arena-derive`,
   `helix-cypher`, `helix-planner`, `helix-value-semantics`) — they are covered
   by the repo `LICENSE`; this closes the inventory metadata gap.
8. **Non-product workspace members pruned** (deleted, not shipped): `crates/server`,
   `crates/cli`, `crates/ast-bench`, `bindings/uniffi`, `tools/opencypher-tck`,
   plus docs/assets/docker/scripts trees. Only the product dependency closure
   (`crates/{ast,ast-arena-derive,graph-algorithms,planner,cypher,db,db-testkit,value-semantics}`,
   `sdks/rust`) is kept. (Both pruned bins also depended on `helix-metrics`.)
9. **Pins outside the fork are unchanged**: `slatedb` git rev,
   `rust-toolchain.toml`, dependency versions.

## Gatekeeper evidence (rerun to verify)

```sh
# from spikes/gate-b/rust-tauri, with the isolated toolchain env:
cargo tree -i helix-metrics        # expect: package ID did not match any packages
grep -ri helix_metrics vendor/helix-db   # expect: 0 hits (docs/VENDOR aside)
grep -ri 'agpl' vendor/helix-db    # expect: 0 hits
node evidence.mjs                  # Gate B suite: 6/6 checks
```

Receipts: `spikes/gate-b/rust-tauri/evidence/` and
`evidence/gates/gate-b/dependency-license-inventory.json`.

## Maintenance

This is a **fork pin**: upstream rev bumps require reapplying the strip (small,
well-defined surface: one manifest line + the `embedded_metrics` stand-ins +
`query_service.rs` telemetry removal). An upstream feature request to make
`metrics` optional is the long-term fix. Re-vendoring must never reintroduce
`helix-metrics` (AGPL-3.0) into the product graph.
