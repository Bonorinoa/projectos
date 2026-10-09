# M0 Gate B — embedded graph feasibility spike

Date: 2026-10-06  
Host: macOS 26.6.2, arm64, Python 3.14.4  
Decision: **do not select an engine yet**

## Outcome

LadybugDB 0.21.1 and HelixDB SDK 0.3.4 with embedded runtime 0.3.3 both pass the same narrow, in-process contract on this host. Neither process opened a listening TCP socket. Both produced the same 2,057-byte deterministic export with SHA-256 `881b5e537b044c94fbb7b40357c84949bec6f65de8fded685f075a1fd2a60ff2`.

Helix must **not** be eliminated at pre-screen. Its current official documentation now describes an embedded client that executes without HTTP against memory or disk. That is a material change from the server/SDK-only hypothesis recorded in the architecture brief.

Update 2026-10-08: a pinned Helix Git source now builds with its upstream Rust 1.97.1 toolchain in a Tauri 2.12.0 macOS arm64 debug app. Six local Rust operations pass both from the CLI and through the app's actual IPC button: one-hop, two-hop, update, delete, disk reopen and read-only write rejection. See [rust-integration.json](rust-integration.json) and [host-ui-observations.json](host-ui-observations.json). The Python contract was rerun for both candidates.

The gate is not fully verified: the full shared Rust adapter contract, clean universal/Linux release packages, signing, accepted memory corpus and license closure remain open. The published helix-db 3.0.0 crate lacks the embedded feature, so the probe pins upstream Git revision `c72e64f0c7e480f3d125f071659d5e3ad7b8da15` rather than silently patching the crate. Its dependency inventory includes AGPL-3.0 metrics and upstream crates with missing license metadata; these are review blockers, not a license compatibility conclusion.

Independent review found `Client::open` starts upstream telemetry by default. The initial native/CLI observations preceded this finding; they may have attempted telemetry, but delivery was not measured. The probe must explicitly configure `QueryTelemetry::Disabled` for every open and rerun before current-source proof is claimed. A local embedded database does not automatically mean no network-capable behavior.

Recommended roadmap status for `m0.graph`: **implemented**, with verification still open.

## Shared executable results

| Criterion | Ladybug | Helix | Evidence boundary |
|---|---:|---:|---|
| In-process open; no listening TCP port | Pass | Pass | `lsof` against a live embedded process |
| Transactional node/edge insert and update | Pass | Pass | One engine transaction/write batch over the shared fixture |
| Node/edge deletion | Pass | Pass | Counts and stable IDs checked after deletion |
| One/two-hop lineage and reverse impact | Pass | Pass | Exact expected stable-ID sets |
| FTS companion mapping | Pass | Pass | Disposable SQLite FTS5 index returns graph stable IDs only |
| Four readers plus one controlled writer | Pass | Pass | 20 upserts; final value present once |
| Abrupt exit and reopen | Pass | Pass | Child exits with `os._exit(79)` after committed operations |
| ProjectOS journal replay | Pass | Pass | Three fsynced records replay identically twice |
| Deterministic export/import | Pass | Pass | Byte-identical JSONL before and after import |
| Copy-on-write migration | Pass | Pass | Separate candidate, validation, atomic swap, reopen |
| Small multi-project memory probe | Measured | Measured | 603 nodes and 600 edges; not yet a production corpus |

Memory probe results:

| Engine | Peak RSS | Store bytes | Ingest/export time |
|---|---:|---:|---:|
| Ladybug | 94,765,056 B | 3,162,112 B | 1.984 s |
| Helix | 92,815,360 B | 849,928 B | 0.389 s |

These numbers are feasibility observations, not selection benchmarks. The architecture requires correctness, recovery, licensing, and packaging to pass before performance can decide anything.

## Packaging and licensing findings

### LadybugDB

- The official project and pinned package metadata declare MIT licensing.
- Current official docs describe an embedded database, serializable ACID transactions, one read-write database object with multiple connections, and macOS/Linux platform support.
- The installed PyPI native library is thin arm64, ad-hoc signed, and links `/opt/homebrew/opt/openssl@3/lib/libssl.3.dylib` and `libcrypto.3.dylib`. That binding is not a clean universal product artifact.
- The intended Rust path remains untested. The Rust wrapper/core version pairing and final extension vendoring/checksums must be proven in a clean product build.

### HelixDB

- Current official docs explicitly describe embedded requests without HTTP, disk/in-memory sources, writer and read-only handles, and bounded cache profiles.
- The installed embedded runtime is universal2 and links only system libraries. It is ad-hoc signed, not a signed/notarized product artifact.
- PyPI publishes universal2 macOS plus manylinux x86_64/aarch64 runtime wheels.
- The current Python SDK is 0.3.4 while the embedded runtime is 0.3.3 even though the docs say to install a matching runtime.
- The repository license is Apache-2.0, SDK package metadata says MIT, and the embedded runtime package declares no license. This must be reconciled before selection.

## Recommendation

Keep both candidates alive. Give Helix the first slot in the next packaging bakeoff because its current embedded runtime is universal2, self-contained on this host, and passed the shared contract. This is not an engine selection.

The next decisive test is a pinned Rust implementation inside a minimal Tauri shell on clean targets:

1. Build and sign a macOS universal app with no Homebrew or runtime-download dependency.
2. Build and install Linux x86_64 and aarch64 packages on clean supported distributions.
3. Record exact core/wrapper/runtime pins, checksums, licenses, symbols, and dynamic dependencies.
4. Repeat the shared contract and crash/migration tests through the Rust adapter.
5. Freeze a representative multi-project corpus and memory ceiling, then rerun the bounded-memory probe.

If neither candidate clears those steps, return to collaborative design as required. Do not substitute a different database silently.

## Reproduction

```sh
python3 -m venv spikes/gate-b/.venv
spikes/gate-b/.venv/bin/python -m pip install -r spikes/gate-b/requirements.lock
PYTHONPATH=spikes/gate-b/src \
  spikes/gate-b/.venv/bin/python spikes/gate-b/src/run_gate_b.py
```

The harness deletes and recreates only `spikes/gate-b/work/`. Its result is [runtime-results.json](runtime-results.json). The criterion-level decision is [report.json](report.json), packaging inspection is [packaging-inspection.json](packaging-inspection.json), and downloaded primary-source snapshots are checksummed in [source-manifest.sha256](source-manifest.sha256).

## Primary sources

- LadybugDB pinned repository: <https://github.com/LadybugDB/ladybug/tree/v0.21.1>
- LadybugDB concurrency: <https://docs.ladybugdb.com/concurrency/>
- LadybugDB system requirements: <https://docs.ladybugdb.com/system-requirements/>
- LadybugDB 0.21.1 package metadata: <https://pypi.org/pypi/ladybug/0.21.1/json>
- HelixDB embedded database: <https://docs.helix-db.com/database/helix-db/start-here/local-development/embedded-database>
- HelixDB repository: <https://github.com/HelixDB/helix-db>
- HelixDB SDK package metadata: <https://pypi.org/pypi/helix-db/json>
- HelixDB embedded runtime metadata: <https://pypi.org/pypi/helix-db-embedded/json>

## Explicitly untested

- Full shared Rust adapter contract beyond the six executed operations.
- Clean universal release embedding, product codesigning/notarization, and installer behavior.
- Linux runtime behavior on x86_64 or aarch64.
- A frozen production-representative corpus and accepted memory ceiling.
- Ladybug FTS/vector dynamic extensions; the spike intentionally used the allowed clean companion-index mapping.
- Gate D's exhaustive recovery matrix: disk-full, torn journal, corrupt database, permission loss, N-1/N-2 migrations, cloud/network filesystems, and cross-platform sidecar transfer.
