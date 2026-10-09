# Gate C: worker packaging and isolation spike

This spike tests the ProjectOS architecture brief's Gate C without claiming that a process boundary is an authorization boundary. It uses a versioned NDJSON protocol, an opaque capability handle, a TypeScript worker, a JavaScript supervisor, and a small native guardian.

## What this demonstrates

- Protocol v1 negotiation and rejection of an unsupported version.
- A worker environment rebuilt from an allowlist rather than inherited from the parent.
- Brokered file reads through a random opaque capability handle.
- Native descriptor-relative `openat` reads with `O_NOFOLLOW` on every component, a retained root descriptor, device/inode verification, regular-file and byte limits. The worker never receives a descriptor.
- Active directory/symlink swaps during 50,000 reads, plus eight native positive/negative contract cases.
- A SHA256-pinned local Node arm64 bundle, launched from `build/runtime/node`; changed runtimes fail closed rather than being silently re-pinned.
- Current-host denial probes for ambient files, DNS, direct IPv4/IPv6, IPv4/IPv6 localhost, redirect initiation, and child-process creation.
- Parent-crash cleanup with a `kqueue` parent-exit watch and process-group kill.
- Full process-tree cancellation using a controlled test-only profile that permits only `/bin/sleep` as a descendant.
- Wall-clock, CPU, frame-size, and cumulative-output ceilings.

## What this does not prove

The gate is **failed/blocked**, not passed. The local runtime bundle now has a reviewed version/platform/hash pin and the native broker passes the active race adversary. The bundle still needs upstream provenance/license verification, signing/notarization, universal and clean-machine distribution checks. The current macOS probe uses `sandbox-exec`, whose installed manual marks it deprecated and directs developers to App Sandbox. macOS rejects the attempted `RLIMIT_AS` memory ceiling, so no OS-enforced memory bound was proven. Linux packaging/confinement and the separate computer-use matrix were not run on this host.

The remaining production decision is recorded in `docs/adr-worker-isolation.md`: prove a minimal App Sandbox/XPC service topology or a runtime alternative that meets all locked boundaries, select an enforceable macOS memory mechanism, and run the frozen package/OS matrix before changing `m0.worker` to verified.

## Reproduce

Requirements on the tested host:

- macOS with `/usr/bin/cc`, `/usr/bin/sandbox-exec`, and `/usr/bin/man`
- The exact Node.js 22.22.3 macOS arm64 binary specified by `runtime-pin.json`, with built-in TypeScript type stripping. A reviewed re-pin and separate evidence are required for another target.

From the repository root:

```sh
node spikes/gate-c/run-tests.mjs
```

When run inside another sandbox, the outer sandbox may reject nested `sandbox_apply`. Run the command directly on the host in that case. The suite only writes beneath `spikes/gate-c/build/` and `evidence/gates/gate-c/`.

Primary outputs:

- `evidence/gates/gate-c/report.json` — machine-readable decision matrix.
- `evidence/gates/gate-c/raw-results.json` — raw commands, protocol transcripts, lifecycle observations, and stderr.

The test-only controlled-child profile is not the production profile. The production profile denies process forking and execution; the controlled profile exists only to prove that the guardian kills an entire process group if descendants ever exist.

The retained descriptor is passed only to the trusted native broker. Hard links planted inside a grant and concurrent writes to a readable file are not prevented by this path lookup policy; no immutable snapshot or write broker is claimed. The race fixture is disposable and contains sentinels, never credentials.

## Supported App Sandbox incremental probe

Run `node spikes/gate-c/run-app-sandbox.mjs` on the host. It builds and ad-hoc signs a minimal native `.app` helper with only the App Sandbox entitlement, then compares its file/network/child behavior against an unsandboxed control. Evidence is in `evidence/gates/gate-c/app-sandbox-report.json`. The native helper's ambient file and localhost connection are denied with EPERM, but `/bin/sh` still executes. This proves a supported confinement subset, **not** the locked worker boundary. Bare CLI signing was insufficient on this host; the identified app bundle launches successfully. No secret, external network request or new entitlement is used.
