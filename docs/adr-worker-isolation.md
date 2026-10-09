# Worker isolation disposition — Gate C

Date: 2026-10-08. Status: partial implementation; production topology remains blocked.

## Implemented boundary improvement

The spike's JavaScript check-then-open broker has been removed. At capability grant the supervisor opens the root directory, captures its device/inode with `fstat`, and retains its descriptor. Only the native broker receives that descriptor as fd 3; the worker receives an opaque random handle. The broker duplicates the descriptor, verifies identity, and traverses each relative component with `openat(O_NOFOLLOW)` plus `O_DIRECTORY` for intermediate components. Parent traversal and absolute/NUL paths are denied. The final descriptor must be a regular file; nonblocking opens avoid FIFO blocking and reads stop at 16 KiB. Identity is recorded before/after reads. Renaming the granted directory does not retarget the capability.

This guarantees descriptor-relative lookup under a symlink swap. It does not freeze file contents, prevent an authorized writer from editing a file already within the grant, or deny hard links planted before grant; those need separate policy if desired. No write capability is implemented here. The root grant path is supplied by the trusted supervisor, not worker input.

The native adversary continuously renames an intermediate directory and places an outside symlink at its name while 50,000 descriptor-relative reads execute. Every successful read must return the inside sentinel. Denials must also occur, establishing that the attacker actually overlaps the reader. Native contract cases cover final/intermediate symlinks, traversal, wrong root inode, FIFO, oversized reads, and a valid read. Raw results and source/binary hashes live in the Gate C evidence folder.

## Runtime packaging

The runner verifies the exact version/platform and SHA256 in `spikes/gate-c/runtime-pin.json`, copies the binary into `build/runtime/node`, and launches that copy. It records `otool -L`. This is a reproducible local arm64 bundle, not a signed universal distribution. The runtime came from the existing local Hermes installation; verifying upstream provenance and collecting the distribution license notices are outstanding. A different host/runtime must fail rather than silently re-pin. Node's [official SEA documentation](https://nodejs.org/download/release/v22.22.3/docs/api/single-executable-applications.html) describes another packaging route, but compilation alone grants no confinement.

## Production confinement decision

Do not promote the diagnostic `sandbox-exec` profile to a shipping boundary. Its installed manual marks it deprecated. A supported macOS design should prototype a separately sandboxed XPC service with minimal file/network entitlements and scoped broker IPC. [Apple documents App Sandbox](https://developer.apple.com/documentation/security/protecting-user-data-with-app-sandbox) and [child sandbox inheritance](https://developer.apple.com/library/archive/documentation/Miscellaneous/Reference/EntitlementKeyReference/Chapters/EnablingAppSandbox.html). Inheritance carries the main app's static entitlements, so attaching an inherited child to a broadly privileged kernel is not proof of the intended narrower worker boundary. User-selected dynamic access also cannot simply be assumed inherited.

XPC + App Sandbox is a candidate requiring a signed service build and adversarial test; it has not been implemented or verified in this spike. App Sandbox by itself does not demonstrate the locked prohibition on arbitrary child processes, and Node exposes child-process/network/filesystem primitives. One alternative to evaluate is a minimal JS engine or WebAssembly-hosted JS runtime exposing only broker operations, paired with the separately sandboxed service. This changes runtime compatibility and requires explicit proof for language support, cancellation and memory limits. Do not claim any candidate meets Gate C from documentation alone.

### Executed supported App Sandbox subset

`node spikes/gate-c/run-app-sandbox.mjs` builds a native `.app` helper, signs it ad hoc with only `com.apple.security.app-sandbox=true`, displays the signed entitlements and launches it directly. A bare signed CLI executable initially terminated with `SIGTRAP`; its local crash diagnostic reported failure to create code identity. Packaging it in an identified app bundle resolved startup. The generated fixture bundle's Finder metadata is cleared before re-signing; this touches only generated packaging output.

The app-bundle helper actually ran: ambient fixture-file access and IPv4 localhost connection both returned `EPERM`. An unsandboxed control could open the same fixture and received connection refusal rather than policy denial for localhost. DNS lookup for an `.invalid` name failed, but that alone is not proof of DNS confinement. Critically, the sandboxed helper successfully forked and executed `/bin/sh` (controlled exit 42). This demonstrates supported file/network restrictions and also demonstrates that these minimal App Sandbox entitlements do **not** satisfy the locked child-process constraint. `evidence/gates/gate-c/app-sandbox-report.json` retains compilation, signing, entitlements, execution/control transcripts and source hashes. No TypeScript worker, XPC broker integration, OS memory ceiling, universal or distribution proof is claimed from this native probe.

## Unresolved hard gates

- macOS OS memory ceiling: Darwin rejects the tested `RLIMIT_AS`; V8 heap flags and sampled RSS watchdogs do not prove a hard OS ceiling.
- macOS supported production confinement, child-process denial, signed universal/notarized clean install.
- Frozen Linux distribution/package matrix with namespaces/seccomp/cgroups or a demonstrably equivalent boundary.
- Separate computer-use target identity, consent/emergency stop, scoped/redacted capture, permission revocation, credential exclusion, X11/Wayland matrix.

Until these pass, `m0.worker` remains blocked. The native broker improvement is usable evidence for the next topology prototype, not authorization to build product features on an unproven worker boundary.

## Gate C2 probe result — WASM-hosted JS runtime (2026-10-09)

Executed `spikes/gate-c2/probe-wasm` (matrix: `spikes/gate-c2/decision-matrix.md`, evidence:
`spikes/gate-c2/probe-wasm/evidence/`). A javy 9.1.0 QuickJS guest running under wasmtime 49.0.2
satisfies the hard Gate C criteria that eliminated every other candidate:

- C4 (no child process): the guest imports only 9 wasi_snapshot_preview1 stdio/env/clock functions;
  there is no process/spawn import and no ability to fork/exec — by construction.
- C5 (hard memory ceiling): `-W max-memory-size` traps the guest cleanly ("Error: out of memory") at
  both 8 MiB and 128 MiB; a fuel budget (`-W fuel`) traps as a second cancellation mechanism.
- C3 (no ambient network): zero socket/DNS imports; env probe confirms process/spawn/fetch/require
  are all undefined inside the guest.
- C1/C11 (protocol + TS/JS): the NDJSON broker protocol core from `worker.ts` was ported without Node
  builtins and passes hello/negotiation, broker.request grant + deny, unknown-type, and
  version-mismatch cases. Delta recorded: Node builtins (fs/child_process/net/dns/readline) do not
  exist in the guest — the old topology's ambient-denial probe code is obsolete, not portable.

**Topology decision (ADR input, not a claim):** WASM-hosted JS is the only candidate meeting the
locked C4/C5 boundaries on macOS. XPC + App Sandbox cannot express child-process denial or a memory
limit (no such entitlements; probe forked `/bin/sh`); `sandbox-exec` is deprecated; the Node SEA
control fails C4/C5 (existing gate-c evidence). Remaining, recorded, and NOT waived: C9 (signed
universal distribution + clean install, Linux matrix), vendor the QuickJS provider for provenance,
fd-handoff integration with the descriptor broker, host receipts, and a Linux rerun of this probe.
`m0.worker` stays blocked until those have evidence.
