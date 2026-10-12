# Decision: Gate C scope and hold (2026-10-11)

Owner decision recorded after the M2b close-out and a Gate C baseline rerun.

## 1. Gate C keeps its computer-use criteria

Gate C's exit criteria include eight `C-CU-*` (constrained computer-use) checks.
The engineering lead proposed splitting those out to the roadmap's M3 `m3.ui`;
the owner **declined the split**. `C-CU-*` remain part of Gate C.

Consequence (accepted): Gate C cannot exit until the constrained computer-use
surface is built — i.e. **closing Gate C necessarily pulls in the roadmap's M3
`m3.ui` work.** Gate C and that slice are coupled by decision, not by oversight.

## 2. Current Gate C status (baseline rerun 2026-10-11)

Reran `~/.hermes/node/bin/node spikes/gate-c/run-tests.mjs` (pinned runtime):
**24 pass / 3 fail / 10 untested**, decision `fail` → `blocked`.

- FAIL: `C-PACKAGE-MACOS`, `C-MEMORY-CEILING`, `C-MACOS-PRODUCTION-CONFINEMENT`
- UNTESTED: `C-PACKAGE-LINUX`, `C-LINUX-CONFINEMENT`, and the eight `C-CU-*`.

## 3. Hold

The infrastructure work that would close the worker-isolation core — shipping the
decided **javy-QuickJS-under-wasmtime** topology as producing enforcement (hard
memory ceiling, macOS production confinement, packaging) — is **HELD**, pending
owner resources:

- a **signing / notarization identity** (`C-PACKAGE-MACOS`);
- a **Linux environment** for the Linux matrix (`C-PACKAGE-LINUX`,
  `C-LINUX-CONFINEMENT`).

No code work is in flight by decision. The next concrete step is the constrained
computer-use surface (`m3.ui`), sequenced after the owner sorts the resources
above.

Evidence: `evidence/gates/gate-c/report.json` (rerun), `spikes/gate-c2/decision-matrix.md`.
