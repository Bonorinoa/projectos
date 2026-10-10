# Gate B license screen — helix-metrics (AGPL-3.0) in the HelixDB dependency graph

**Date:** 2026-10-09
**Scope:** Static source analysis of pinned HelixDB rev `c72e64f0c7e480f3d125f071659d5e3ad7b8da15` (checkout at `/private/tmp/projectos-helix-c72e64f`) and ProjectOS manifest `spikes/gate-b/rust-tauri/Cargo.toml`.
**Verdict at a glance:** `helix-metrics` is the **only** license-incompatible crate in the graph (explicit `AGPL-3.0`), it is a **mandatory (non-optional) dependency** of the `db` crate with **no feature toggle**, and it is compiled into the embedded product path unconditionally. It **cannot** be excluded by any build flag — removal requires a fork/patch or an upstream change. The rest of the HelixDB tree at this rev is Apache-2.0 (repo LICENSE is Apache-2.0, not the historical source-available license).

---

## 1. Can helix-metrics be excluded at build level? — **No.**

### 1.1 The dependency is non-optional

`crates/db/Cargo.toml` (package `db`, v0.1.0) — line 14 under `[dependencies]`, **no `optional = true`, no feature gate**:

```toml
14| helix-metrics = { path = "../metrics" }
```

The `[features]` table (lines 77–87: `default`, `index-lifecycle-testing`, `test-util`, `future-vector-codecs`, `production-coverage`, `production-scale`, `fuzzing`, `async-index-benchmark`, `force-vector-scalar-kernel`) contains **no metrics reference**; `default = []` (line 78), so `default-features = false` changes nothing. Cargo has no "exclude dependency X" mechanism — every feature combination builds `helix-metrics`.

### 1.2 Both ProjectOS pins pull db → helix-metrics

`spikes/gate-b/rust-tauri/Cargo.toml` (lines 9–10):

```toml
9| helix-db = { git = "https://github.com/HelixDB/helix-db.git", rev = "c72e64f0c7e480f3d125f071659d5e3ad7b8da15", features = ["embedded"] }
10| helix-core-db = { package = "db", git = "https://github.com/HelixDB/helix-db.git", rev = "c72e64f0c7e480f3d125f071659d5e3ad7b8da15" }
```

- `helix-db` (sdks/rust) → `embedded = ["dep:db"]` (`sdks/rust/Cargo.toml:40`) → `db`; `helix-core-db` **is** `db`.
- `db` → `helix-metrics` (mandatory, §1.1). The embedded product path therefore always compiles it.
- Same non-optional pattern elsewhere (not in ProjectOS's graph): `crates/server/Cargo.toml:14`, `crates/cli/Cargo.toml:7` — both `helix-metrics = { path = ... }` with no optional/feature gating.

### 1.3 Usage surface in db (what a removal patch must touch)

`helix_metrics` appears in `crates/db/src` in exactly two non-test files plus one test:

- `crates/db/src/lib.rs:88-89` — `use helix_metrics::query::transport::OssQueryMetrics;` / `use helix_metrics::telemetry;`
- `crates/db/src/lib.rs:2516-2537` — `async fn with_embedded_query_metrics(self)` calls `helix_metrics::query::transport::start_oss_from_env(telemetry::Source::Embedded)` (embedded path; skipped under `cfg!(test)` or `QueryTelemetry::Disabled` at lib.rs:2517-2518)
- `crates/db/src/lib.rs:2539+` — `fn embedded_query_metrics(&self) -> Option<OssQueryMetrics>`
- `crates/db/src/query_service.rs:12` — `use helix_metrics::{query, query::transport::OssQueryMetrics};`
- `crates/db/src/lib.rs:4355` — test `embedded_query_entrypoints_emit_exactly_once`

(db-internal `mod metrics` items — `pull::metrics::WorkCounters`, `index_lifecycle/secondary.rs` `metrics::` — are crate-internal and unrelated.)

**Telemetry is runtime opt-in, so the problem is distribution-time copyleft, not default data exfiltration:** `crates/metrics/src/query/transport.rs:121-130` — `start_oss_from_env` returns `Ok(None)` when no persisted privacy settings exist (`cli::load_query_metrics_settings()` → `None`); `HELIX_CLUSTER_ID` env only supplies a cluster id. Still, shipping an AGPL telemetry transport at all is a separate local-first/privacy tension (cf. D-14) even if it never fires.

### 1.4 Conclusion for Q1

No feature flag and no `default-features` combination removes `helix-metrics`. Exclusion is possible only by **forking/patching HelixDB at rev c72e64f** (delete `crates/db/Cargo.toml:14` + neutralize the ~5 call sites above) or an **upstream feature** (none exists). Patch surface is small (~2 source files + 1 manifest line + 1 test), but the fork must be maintained across rev bumps.

## 2. What licenses do the Helix crates actually declare?

### 2.1 Repo license at this rev: Apache-2.0 — not a custom source-available license

- `/private/tmp/projectos-helix-c72e64f/LICENSE` — full Apache License 2.0 text (verified: "Apache License / Version 2.0, January 2004").
- `/private/tmp/projectos-helix-c72e64f/README.md:321` — "HelixDB is licensed under the Apache License 2.0" (badge at :17).

The historical source-available-license assumption does **not** hold for this checkout. The `null`-license crates in `evidence/gates/gate-b/dependency-license-inventory.json` (`db`, `helix-ast`, `helix-ast-arena-derive`, `helix-cypher`, `helix-planner`, `helix-value-semantics`) are workspace members (`Cargo.toml` members lines 2–18) covered by the repo's Apache-2.0 LICENSE — the nulls are a **metadata gap, not a license gap**.

Crate-by-crate (each crate's `Cargo.toml`):

| Crate | Declared `license =` | Effective license |
|---|---|---|
| `helix-db` (sdks/rust) | `Apache-2.0` (line 7) | Apache-2.0 |
| `helix-dsl-macros` | `Apache-2.0` (line 6) | Apache-2.0 |
| `helix-graph-algorithms` | `Apache-2.0` (line 7) | Apache-2.0 |
| `db`, `helix-ast`, `helix-ast-arena-derive`, `helix-cypher`, `helix-planner`, `helix-value-semantics` | (none) | Apache-2.0 via repo LICENSE |
| **`helix-metrics`** | **`AGPL-3.0`** (`crates/metrics/Cargo.toml:7`) | **AGPL-3.0 — sole incompatibility** |

### 2.2 What the AGPL means for the product

`helix-metrics` explicitly declares `license = "AGPL-3.0"` (`crates/metrics/Cargo.toml:7`); the repo's Apache-2.0 LICENSE does not override the crate's manifest for consumers. Statically linked into a Rust binary, AGPL-3.0 §13 treats the combined work as a derivative — incompatible with an Apache-2.0 product (Gate B hard fail, `docs/projectos-architecture-brief.md` §17 Gate B; D-22 "Apache-2.0 — Locked product decision, ADR and legal review required", decisions table). The other Helix crates raise no Gate B license issue.

## 3. Practical options, ranked

### (a) Fork/patch HelixDB at rev c72e64f to strip helix-metrics — **rank 1**
- Diff: delete `crates/db/Cargo.toml:14`; replace `helix_metrics` call sites in `crates/db/src/lib.rs` (88–89, 2516–2537, 2539+) and `query_service.rs:12` with no-ops/local stubs; drop the telemetry test (lib.rs:4355). Optionally add `license = "Apache-2.0"` to the six metadata-less crates in the same fork to close the audit gap. Pin the fork via `[patch."https://github.com/HelixDB/helix-db"]` at the same rev.
- **Tradeoffs:** keeps the Helix engine (Gate B runtime evidence intact — Ladybug vs Helix per `evidence/gates/gate-b/REPORT.md`, Helix smaller/faster on the arm64 run); removes the telemetry transport along with the license conflict; rest of the tree is already Apache-2.0. Costs: fork maintenance across upstream rev bumps; small, well-defined code surgery; file an upstream feature request ("make `metrics` optional") in parallel.
- **Proof:** see §4.

### (b) LadybugDB via the Rust crate `lbug` (MIT) — **rank 2 fallback**
- Evidence: `evidence/gates/gate-b/sources/ladybug-rust-cargo-main.toml` — `lbug` 0.18.0, `license = "MIT"` (line 9), built by `cmake` + `cxx-build` in `build.rs` from the vendored C++ core (`lbug-src`), `vcpkg = "0.2"` for native deps, `arrow` optional. LadybugDB passed the Gate B hard-fail screen (`evidence/gates/gate-b/report.json` → `hard_fail_screen.ladybug.status = "pass"`).
- **What a Rust-first path needs:** pin exact `lbug`/`lbug-src` pairing (Gate B already mandates pinning/checksumming extensions), and solve the old arm64 blocker — the tested PyPI artifact was thin-arm64 and linked Homebrew OpenSSL (`/opt/homebrew/opt/openssl@3/...libssl.3.dylib`, per gate-b `report.json`/REPORT.md) — by a **vendored/static TLS build with no Homebrew `pkg-config` dependency**, then re-run Gate B/D evidence on the new engine (smaller feature set: no native vector search/BM25), including symbol/signing checks. Higher cost than (a); choose only if fork maintenance is unacceptable.

### (c) ADR to change D-22 or dual-license a component — **rank 3, last resort**
- D-22 is a **Locked product decision**: changing it requires ADR + legal review (decisions table). Dual-licensing ProjectOS's *own* code does **not** cure the conflict — the AGPL obligation attaches to the **combined work** containing the statically linked crate, so any artifact linking `helix-metrics` ships under AGPL terms. Only viable if legal review accepts AGPL for the product, which contradicts D-22's purpose; a commercial upstream license for helix-metrics is not evidenced.

## 4. Recommendation

**Owner decision: option (a)** — fork HelixDB @ `c72e64f`, strip `helix-metrics`, pin the fork. It is the only path that keeps the Helix engine (all existing Gate B evidence intact) and the locked D-22 Apache-2.0 product, with a small, well-understood patch surface and a local-first/privacy bonus. Fall back to (b) LadybugDB `lbug` (MIT) only if fork maintenance is unacceptable; do not change D-22 without legal review.

**Gatekeeper evidence** (exact rev; isolated toolchain per Gate A: `CARGO_HOME=/private/tmp/projectos-gate-a-cargo`, `RUSTUP_HOME=/private/tmp/projectos-gate-a-rustup`, cargo at `/private/tmp/projectos-gate-a-rustup/toolchains/1.97.1-aarch64-apple-darwin/bin/cargo`):

```bash
# BEFORE (proves the conflict sits on the product path):
cargo tree -i helix-metrics
#   helix-core-db v0.1.0 (...) └── helix-metrics v0.1.1 (AGPL-3.0)

# AFTER (fork pinned via [patch]):
cargo tree -i helix-metrics   # → error: package ID specification `helix-metrics` did not match any packages
grep -rn "helix_metrics" <fork>/crates/db  # → 0 hits
cargo tree -p helix-core-db   # → no helix-metrics node anywhere
# then regenerate dependency-license-inventory.json → no AGPL entries
```

Static-analysis anchors: `crates/db/Cargo.toml:14`, `crates/metrics/Cargo.toml:7`, `/private/tmp/projectos-helix-c72e64f/LICENSE`, `README.md:321`, `sdks/rust/Cargo.toml:40`, `spikes/gate-b/rust-tauri/Cargo.toml:9-10`, `crates/metrics/src/query/transport.rs:121-130`.

---

## 5. EXECUTED — vendored fork strip verified (2026-10-09)

Option (a) was executed and re-verified end to end.

**Artifact.** `vendor/helix-db` = `git archive` of upstream rev `c72e64f`
(same revision), with: `crates/metrics` deleted and removed from `members`;
non-product members pruned (`crates/server`, `crates/cli`, `crates/ast-bench`,
`bindings/uniffi`, `tools/opencypher-tck`, docs/assets/docker/scripts trees);
`crates/db` dependency on `helix-metrics` removed; telemetry call sites and the
`QueryObservation` payload builder removed; inert stand-ins added in
`crates/db/src/embedded_metrics.rs`; `license = "Apache-2.0"` added to the six
crates that lacked metadata. Every delta is enumerated in
`vendor/helix-db/VENDOR.md`.

**Repoint.** `spikes/gate-b/rust-tauri/Cargo.toml` now depends on the fork by
path (same rev), so the upstream git source — and with it `helix-metrics` — is
absent from the product dependency graph.

**Gatekeeper evidence (this run, Rust 1.97.1, isolated toolchain):**

| Check | Result |
|---|---|
| `cargo build` against the fork | **pass** (~3m30s; zero warnings after cleanup) |
| Gate B suite | **6/6 pass** — desktop-build, rust-tests, desktop-bundle, contract, dynamic-libraries, dependency-inventory |
| Contract receipt | unchanged: 6 operations, `queryTelemetry: explicitly-disabled`, `sourceRevision c72e64f`, export rows `[a,b]` |
| `dependency-license-inventory.json` | **497 packages (was 505); 0 AGPL entries; 0 `helix-metrics`**; only null-license entry is the spike crate itself (`projectos-graph-embedding`, `publish = false`) |
| `grep -ri agpl vendor/helix-db` | only explanatory strip comments (VENDOR.md + inline notes) — no license text |
| `grep -rl helix_metrics vendor/helix-db` | only the stand-in doc comments in `embedded_metrics.rs` — no code references |

**Unchanged:** D-22 (Apache-2.0 product) — no ADR required, option (a) satisfies
the locked decision. **Residual:** legal review of the vendored fork remains an
owner action; the fork is a pin and must be re-stripped on upstream rev bumps;
an upstream "make metrics optional" request is still the long-term fix.
