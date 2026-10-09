# Gate C2 — WASM-hosted worker probe

Decider probe from `decision-matrix.md`: can the ProjectOS worker (TS-first, D-20)
run inside a WASM guest with **hard memory ceiling** (C5) and **no child/network
capability by construction** (C3/C4), under wasmtime?

Host: wasmtime 49.0.2 (brew). Toolchain: javy 9.1.0 (sha-verified arm64 macos).
See `runtime-pin.json` for pins and license notes (javy/wasmtime Apache-2.0,
QuickJS MIT-style). Linux/universal reruns remain outstanding.

## Repro (verified commands)

```sh
JAVY=/private/tmp/projectos-gate-c2-javy/javy
WASMTIME=wasmtime

# Build guests (javy auto-fetches the quickjs provider; pin its cache SHA in evidence)
$JAVY build guest/worker-protocol.js -o work/worker-protocol.wasm
$JAVY build guest/oom.js                -o work/oom.wasm
$JAVY build guest/env-probe.js          -o work/env-probe.wasm

# C11 — protocol functionality (one NDJSON frame per run)
echo '{"protocol":"projectos.worker","version":1,"type":"hello","id":"1"}' \
  | $WASMTIME run work/worker-protocol.wasm

# C5 — hard memory ceiling (8 MiB => expect clean trap death)
/usr/bin/time -l $WASMTIME run -W max-memory-size=8388608 work/oom.wasm

# C3/C4 — import surface = entire ambient capability (expect wasi stdio only)
python3 tools/wasm-imports.py work/worker-protocol.wasm

# C3/C4 — env probe (expect no process/spawn/fetch/require/net)
$WASMTIME run work/env-probe.wasm
```

## Evidence slots

- [x] `evidence/worker-hello.json` — negotiation + broker.request reply (hello.ack PASS)
- [x] `evidence/oom-trap.txt` — trap message "Error: out of memory" + peak RSS (8MiB and 128MiB caps both trap)
- [x] `evidence/imports-clean.txt` — import surface: exactly 9 wasi_snapshot_preview1 fns (stdio/env/clock)
- [x] `evidence/env-probe.json` — ambient surface: process/spawn/fetch/require all undefined
- [x] `evidence/work-wasm-sha256.txt` — artifact pins for the three built guests
- [x] `evidence/strip-types-delta.txt` — gate-c worker.ts Node-builtin imports (5) unavailable in guest
- [x] `evidence/fuel-exhausted.txt` — 10k fuel traps; 1M fuel control passes

## Interpretation rules (from decision-matrix.md)

- Guest passes C4 (no child) and C5 (memory) **by construction**; the probe proves
  the mechanism executes and traps as configured.
- C11 delta = the exact list of worker.ts features the guest cannot have (Node
  builtins). The protocol core must survive; denial-probe code does not (it tests
  the old topology).
- One-shot-per-frame I/O is a functional limitation vs the persistent gate-c
  worker — recorded for the ADR as a topology consequence (persistent loop or
  broker-hosted sessions are M1 design choices, not Gate-C claims).
- No claim of `verified` for m0.worker: packaging (C9) and Linux reruns remain
  open. This probe only decides the confinement topology question.