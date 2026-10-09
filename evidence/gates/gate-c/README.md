# Gate C evidence

`report.json` is the decision record. `raw-results.json` preserves the underlying command outputs and protocol transcripts.

Result from the current macOS host: **24 pass, 3 fail, 10 untested; overall Gate C decision: fail; recommended `m0.worker` roadmap status: blocked.**

Implemented improvements: a retained-root-descriptor native `openat`/`O_NOFOLLOW` broker, 50,000 active symlink swaps with zero escape, eight native contract checks, and a locally copied version/platform/SHA256-pinned Node runtime. All worker probes run the bundled copy; the original runtime directory is no longer in the diagnostic sandbox's readable subtree. Source and binary hashes are recorded in the report, including the test runner.

The three hard failures are:

1. The local arm64 bundle is not yet a signed/notarized universal package. Upstream runtime provenance/license notices and clean-machine installation remain unproven.
2. `RLIMIT_AS` is rejected with `EINVAL`, leaving no proven OS-enforced memory ceiling.
3. The working macOS probe uses `sandbox-exec`, which the installed system manual explicitly marks deprecated; it is evidence for policy behavior, not an acceptable selected production boundary.

Linux packaging/confinement and all separately enumerated computer-use checks remain untested rather than being counted as passes.

Additional supported-boundary evidence is in `app-sandbox-report.json`: a native helper packaged in an identified app bundle and signed with only the App Sandbox entitlement actually denied the ambient file and localhost socket with EPERM. Its unsandboxed control opened the same file and received ECONNREFUSED instead of EPERM. However, `/bin/sh` successfully executed inside the App Sandbox. Thus the native supported subset passes, while the production worker confinement criterion remains failed. The current report includes repository-relative `sourceHashes` for automatic stale-evidence invalidation.

Reproduce from the repository root with `node spikes/gate-c/run-tests.mjs` using the exact pin in `spikes/gate-c/runtime-pin.json`. The full diagnostic profile must run on the host; an outer sandbox rejects nested sandbox application. Native race/contract tests run in either context. See `spikes/gate-c/README.md` and `docs/adr-worker-isolation.md` for limitations and production disposition.
