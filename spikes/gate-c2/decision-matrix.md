# Gate C2 — worker confinement decision matrix (bakeoff contract)

Status: open. Candidates: XPC+App Sandbox, WASM-hosted JS runtime, pinned Node SEA (control).
All verdicts below must be filled from executed probes with retained raw evidence; "documented
elimination" is allowed only when the absence of a mechanism is Apple/upstream documented AND
backed by a probe we ran. No candidate passes from documentation alone.

## Locked acceptance criteria (from docs/projectos-architecture-brief.md, Gate C + D-20)

| # | Criterion | Requirement | XPC+Sandbox | WASM host | Node SEA control |
|---|---|---|---|---|---|
| C1 | Language-neutral brokered workers, first worker TS | Protocol v1 NDJSON, opaque capability handles | ? | ? | ✓ (existing gate-c) |
| C2 | No ambient file authority | openat descriptor-relative broker; EPERM outside grant | ? | ? | ✓ (gate-c evidence) |
| C3 | No ambient network | DNS/IPv4/IPv6/localhost denial | partial (probe EPERM localhost) | ? | partial (probe) |
| C4 | No child-process spawn | fork/exec denied or impossible | **FAILS — App Sandbox has no deny-fork entitlement (documented); existing app-sandbox-probe forked /bin/sh (evidence/gates/gate-c/app-sandbox-report.json). XPC service processes inherit sandbox rules; no entitlement narrows this. Eliminated unless a service-level mechanism is demonstrated.** | by construction (no fork/exec in WASM guest) | FAILS (forked /bin/sh, controlled exit 42) |
| C5 | Hard memory ceiling (both OSes) | OS-enforced, not sampled watchdog | FAILS — no App Sandbox memory-limit entitlement; RLIMIT_AS rejected on Darwin (existing evidence) | by construction (wasmtime/instance memory limit) | FAILS (RLIMIT_AS rejected) |
| C6 | Parent-crash orphan cleanup | process-group kill on parent exit | ? | ? (single in-process guest; supervisor kills runtime) | ✓ (kqueue + pgkill) |
| C7 | Full process-tree cancellation | wall/CPU ceilings + tree kill | ? | fuel/timeout limits | ✓ (existing) |
| C8 | Output backpressure | bounded frames/bytes | ? | ? | ✓ (existing) |
| C9 | Signed universal distribution + clean install, macOS+Linux | notarized .app / debs-rpms; no dev tools | ? (signed XPC service possible) | ? (runtime libs; WASI) | partial (ad-hoc, arm64 only recorded) |
| C10 | Runtime provenance & licensing | pinned, checksummed, license-disclosed | ? | wasmtime (Apache-2.0) + quickjs (MIT-ish) favorable | Node 22.22.3 pin recorded; upstream bundle licenses outstanding |
| C11 | TS/JS language support + cancellation for real worker logic | first-party TS worker runs under candidate | ✓ (Node runtime) | ? — TS→JS transpile + guest API surface | ✓ |
| C12 | Receipts/observability of denials | denial is a trace event, not hidden | ? | ? | ✓ |

## Decision rules

1. Any FAIL on C4 or C5 eliminates the candidate on macOS (both are hard-gate criteria).
2. Licensing (AGPL screen): any candidate that forces an AGPL runtime dependency into the
   Apache-2.0 product fails D-22 unless owner ADRs it. (Helix-metrics finding applies to
   Gate B engine, not Gate C; re-check runtime deps per candidate.)
3. If exactly one candidate passes C4+C5, it wins subject to C9–C12 evidence; if none, return
   to design (worker topology decision reopens, per brief §Gate C fallback).
4. Understandings from gate-c remain usable: descriptor broker, runtime pin, supervisor,
   kqueue orphaning, backpressure — these compose with whichever host wins.

## Probe plan

- Probe 1 (decider): WASM host — Rust crate embedding wasmtime (+ quickjs-wasm or similar
  guest JS engine), TS worker transpiled to JS at build time (esbuild, pinned), memory limit
  set at wasmtime config, fuel-based cancellation, broker fd passing via host, protocol.schema.json
  reused. Expected: passes C4+C5 by construction; risks: C11 guest API surface, C9 packaging.
- Probe 2 (control): rerun of gate-c Node SEA under a strict local sandbox profile — expected
  FAIL C4/C5, retained as the control that justifies the runtime change.
- Probe 3 (optional, if user wants belt-and-braces): minimal XPC service shell proving the
  fork behavior at service level — expected to confirm C4 elimination empirically.

## Evidence slots (fill on completion)

- [ ] spikes/gate-c2/probe-wasm/run.md + raw logs → spikes/gate-c2/evidence/wasm-*.json
- [ ] probe control rerun → spikes/gate-c2/evidence/control-*.json
- [ ] ADR update: docs/adr-worker-isolation.md §Production confinement decision
- [ ] roadmap manifest: m0.worker → decision + next step