# Gate D recovery contract spike

This is an offline executable model of the architecture's recovery protocol. It
uses deterministic JSON files and POSIX advisory locks on the local host, **not
Helix/Ladybug graph transactions or their WAL**. Passing this suite is evidence
for protocol fixtures, not verification of the production recovery gate.

From the repository root:

```sh
python3 spikes/gate-d/test_recovery.py --report evidence/gates/gate-d/report.json
```

Python 3.11+ under an unprivileged account on macOS/Linux is required. No packages, network, credentials, API
charges, or changes to user projects are needed. Each case owns a temporary
directory and cleans it up. Eight cases end a child process with `os._exit(91)`
at a named persistence boundary and then reopen its files in the parent.
Those cases prove abrupt-process recovery, not survival of power loss or lost
filesystem/device write buffers.

The report contains every test's outcome, elapsed time and traceback, host
identity, source SHA-256 hashes, and an explicit list of untested requirements.
Regenerate it after source changes. `gate_status` stays `partial_unverified`.

## What changed

The interrupted harness could not hydrate file bytes and its three-field journal
could not replay graph state. It also appended onto torn JSON tails, overwrote a
corrupt live store without quarantine, trusted snapshots without integrity
validation, changed the live schema before migration validation, permitted
concurrent writers, and reported nonexistent raw vault records as available.

The model now retains before/after objects, checks graph invariants and byte
hashes, emits sequenced full-state journal records, replays newer records after a
validated snapshot, quarantines corrupt stores and torn tails, restores full
clones to new destinations, and creates truthful unavailable-blob tombstones for
metadata-only exports. Revision restoration defaults to a new copy. UUID
mismatches and divergent lineage require explicit decisions. Vault loss and
source revocation emit durable model checkpoints so corrupt-store recovery does
not resurrect revoked chunks or unavailable trace claims.

Graph/schema and namespaced pack migration stage a copy, validate it, preserve a
restore point, swap atomically, and emit a post-commit recovery checkpoint before
returning success. Pack transforms receive only their namespace's data. Snapshot
checksums cover sequence/version metadata as well as graph bytes. The independent operation ledger fixture
uses documented idempotency keys and blocks unknown non-idempotent effects until
reconciliation or renewed approval; it is intentionally not a durable ledger.

## Limits that must stay visible

- The real Rust graph adapter must execute these fixtures against actual
  transactions, exports and WAL. The JSON model has no nodes/edges query engine.
- Exception injection models ENOSPC and permission errors at specified calls;
  a real permission-denied case is included. Actual disk exhaustion is untested.
- Project I/O uses a retained root descriptor, `O_DIRECTORY|O_NOFOLLOW` ancestor
  traversal, `O_NOFOLLOW` regular-file reads, and descriptor-relative replacement.
  Hard-linked files are rejected before truncation; nonblocking opens reject
  FIFOs. Tests include an active concurrent symlink adversary and swaps after the
  final validation, parent/root rename and replacement, sidecar/lock symlinks,
  and orphan-stage cleanup. Root movement preserves authority over the originally
  opened directory; a new directory at the previous pathname is untouched.
- Python casefold checks reject the fixture's case aliases. Complete Unicode and
  case equivalence rules across filesystems are untested. Atomic replacement
  never follows a destination symlink, but POSIX rename is not an atomic content
  compare-and-swap: an unrelated regular-file edit after the final hash check can
  still race apply. The production apply protocol must resolve that limitation.
- The manifest is valid TOML but covers only identity/schema for this fixture,
  not the complete project envelope or approved content roots. Envelope and
  graph schema-version migration are not yet coordinated/validated as a pair.
- Restore/export currently model one named artifact plus its revision history.
  Full multi-artifact milestones, deletion/rename semantics, selected retention
  windows, complete recipe/receipt transport, and user-facing repair are untested.
- Profile export/restoration preserves links, preferences, approved patterns and
  unknown fields while dropping local path bindings. Real profile graph adapter
  and its snapshot/journal replay are not implemented here.
- Checksums detect accidental corruption; they are not signatures/authentication
  against an attacker who can rewrite data and recompute hashes.
- Cloud/network filesystem behavior, Linux interoperability, packaging, device
  flush guarantees, production external effects and vault cryptography remain
  separate gates. A missing revision blob produces a tombstone, not a successful
  promise to recover bytes that are no longer present.

`evidence/gates/gate-d/coverage.json` maps architecture requirements to the
actual fixture coverage and remaining work.
