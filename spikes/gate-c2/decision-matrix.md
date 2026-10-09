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

## Gate C2 probe results (2026-10-09, macOS arm64)

Executed `probe-wasm` per its README. Guest = javy 9.1.0 quickjs-provider module under wasmtime 49.0.2.
Raw evidence: `spikes/gate-c2/probe-wasm/evidence/*` (worker-hello.json, worker-broker-request.json,
worker-broker-denied.json, worker-unknown-type.json, worker-version-mismatch.json, env-probe.json,
imports-clean.txt, oom-trap.txt, fuel-exhausted.txt, work-wasm-sha256.txt, strip-types-delta.txt).

| # | Criterion | WASM host verdict | Evidence |
|---|---|---|---|
| C1 | Language-neutral brokered workers, TS first | **PASS** — NDJSON protocol core runs in guest; hello/ack, broker.request grant+deny, unknown-type, version-mismatch all correct | worker-*.json |
| C2 | No ambient file authority | **PASS by construction** — guest imports only stdio/env/clock (9 wasi_snapshot_preview1 fns); no fs above fd_*; host broker (gate-c) composes; fd handoff integration open (M1) | imports-clean.txt |
| C3 | No ambient network | **PASS by construction** — zero socket/DNS imports; env-probe: process/spawn/fetch/require all undefined | env-probe.json |
| C4 | No child-process spawn | **PASS by construction** — no process/spawn imports; guest cannot fork or exec (no such wasm imports) | env-probe.json + imports-clean.txt |
| C5 | Hard memory ceiling | **PASS** — `-W max-memory-size=8MiB` → guest traps ("Error: out of memory"); 128MiB also traps | oom-trap.txt |
| C6 | Parent-crash orphan cleanup | host-side supervisor (gate-c kqueue+pgkill composes); not re-proven here | open at M1 |
| C7 | Full process-tree cancellation | **PASS (fuel)** — `-W fuel=10000` traps; 1M fuel control passes hello | fuel-exhausted.txt |
| C8 | Output backpressure | host-side concern; not probed | open at M1 |
| C9 | Signed universal distribution + clean install | **OPEN** — single macOS arm64 host run; no Linux, no signing, no universal | — |
| C10 | Runtime provenance & licensing | **FAVORABLE** — javy 9.1.0 (Apache-2.0, sha-pinned), wasmtime 49.0.2 (Apache-2.0), QuickJS MIT-style (inside provider); vendor provider for product provenance | runtime-pin.json |
| C11 | TS/JS language support | **PASS (with delta)** — protocol core ported and executes; delta = gate-c worker.ts Node builtins (fs/child_process/net/dns/readline) do not exist in guest and are intentionally not portable; denial-probe code is obsolete in this topology | strip-types-delta.txt |
| C12 | Receipts/observability of denials | guest-side denials are normal replies (worker-broker-denied.json); host receipts open at M1 | — |

**Decision (evidence-backed, ADR input):** WASM-hosted JS runtime is the only candidate satisfying
C4+C5 on macOS. XPC+App Sandbox and Node SEA controls fail on documented+probed grounds (see above).
m0.worker remains `blocked` until C9 packaging/signing + Linux reruns exist — this closes the
*topology* question only.

## Evidence slots (fill on completion)

- [x] spikes/gate-c2/probe-wasm/run.md + raw logs → spikes/gate-c2/probe-wasm/evidence/*.json
- [ ] probe control rerun → spikes/gate-c2/evidence/control-*.json (deferred: Node control already fails C4/C5 in existing gate-c evidence)
- [x] ADR update: docs/adr-worker-isolation.md §Production confinement decision
- [ ] roadmap manifest: m0.worker → decision + next step