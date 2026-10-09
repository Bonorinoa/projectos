# M0 implementation and verification handoff

Window: October 8, 2026. All five focused issues were worked on, with independent subagent review. M0 is **not fully verified** and manual acceptance stays locked. No database or worker architecture was silently selected. No paid model calls, commits, pushes or remote creation were performed.

## What changed in plain language

| Workstream | Demonstrated result | Still required |
| --- | --- | --- |
| A: desktop shell | Native macOS arm64 app builds and launches. Local file preview and cancel were exercised. Duplicate request cancellation registration is fixed; three Rust tests cover registration, job limits and navigation. | Clean universal/Linux install, release signing and full accessibility/IME/PDF checks. |
| B: local graph | Both Python candidate contracts were rerun. A pinned Helix Rust engine builds in a native Tauri app; six graph operations passed through actual IPC before independent review found upstream telemetry enabled by default. Explicit disablement and fresh evidence are required. Dependency metadata is recorded. | Full shared Rust contract, representative memory corpus, clean packages and license closure. AGPL-labelled metrics and missing upstream license metadata prevent treating the inventory as approval. Earlier runs may have attempted telemetry; delivery was not measured. No engine selected. |
| C: worker containment | Native file broker retains the root directory identity and walks paths without following symlinks. 50,000 concurrent swap reads produced zero escapes. Supported Apple App Sandbox denies ambient file/network access in a native subset probe. | App Sandbox still permits spawning a child process; no hard OS memory ceiling was proven. Production TypeScript confinement, release runtime provenance and Linux remain blocked. |
| D: recovery | 113 fault-injection tests pass, independently reviewed. Journal replay, snapshots, migrations, identity/credential recovery and descriptor-anchored writes are covered in the model. | Actual selected graph-engine integration, power-loss/platform durability, Unicode aliases and final regular-file edit races. This remains a model, not a production recovery engine. |
| E: provider contract | 31 offline/mock tests pass. Request-lifetime secret redaction, retryable revoke state and atomic vault size limits were fixed after independent review. PKCE callback and encrypted backup round trips were exercised. | Native credential/egress enforcement, OS keyring, opt-in real-provider conformance and Linux. No live provider claims. |

## Visual progress

Run `node scripts/serve-roadmap.mjs`, then open <http://127.0.0.1:4317/design/projectos-live-mvp-roadmap.html>.

The interactive-roadmap skill guided exact-source snapshots, explicit detected/implemented/verified distinctions and the human-only final gate. The page refreshes status every five seconds while served locally. The server regenerates source/evidence snapshots when files change; it does **not** rerun tests or invent verification. Changed source invalidates old evidence and clears revision-bound acceptance. Ten regression tests cover these rules, imported-content escaping and live regeneration.

## Reproduction

```sh
# Roadmap behavior
node scripts/test-roadmap.mjs
node scripts/update-roadmap.mjs

# Recovery model
python3 spikes/gate-d/test_recovery.py --report evidence/gates/gate-d/report.json

# Mock provider, local PKCE and encrypted vault
node spikes/gate-e/evidence.mjs

# Native shell and graph evidence using the isolated installed toolchains
PROJECTOS_CARGO_HOME=/private/tmp/projectos-gate-a-cargo PROJECTOS_RUST_TOOLCHAIN_HOME=/private/tmp/projectos-gate-a-rustup node spikes/gate-a/tests/native-evidence.mjs
PROJECTOS_CARGO_HOME=/private/tmp/projectos-gate-a-cargo PROJECTOS_RUST_TOOLCHAIN_HOME=/private/tmp/projectos-gate-a-rustup node spikes/gate-b/rust-tauri/evidence.mjs
```

Some local socket/native sandbox probes require normal host execution rather than a restricted execution sandbox. Gate C reproduction and platform limitations are in `spikes/gate-c/README.md`; its expected failing criteria must not be converted to passes.

Evidence lives under `evidence/gates/gate-a` through `gate-e`, including source hashes, logs and independent reviews. Graph GUI observations are in `evidence/gates/gate-b/host-ui-observations.json`. Roadmap review is `evidence/roadmap-review.json`.

## Decisions needed before closing M0

1. Approve a supported worker confinement design that covers child processes and memory, using `docs/adr-worker-isolation.md` as the decision input.
2. Resolve database licensing and complete the candidate-specific Rust/recovery corpus before selecting an engine.
3. Supply/approve clean target hosts and the frozen Linux package matrix, release signing and accepted production-scale corpus.

These are remaining release gates, not reasons to mark the completed narrow tests as failures. Conversely, passing spike tests is not a usable MVP or permission to bypass your manual test gate.
