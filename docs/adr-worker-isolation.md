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
