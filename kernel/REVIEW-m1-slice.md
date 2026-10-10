# M1 thin slice — lead review (2026-10-09)

Reviewer: parent session (engineering lead). Method: independent reruns + a
reviewer-authored adversarial suite; child self-reports were treated as claims.

## What shipped

`kernel/` — the M1 thin vertical slice per `INTERFACES.md`:

| File | Contents |
|---|---|
| `src/error.rs` | `KernelError` (Io, InvalidSidecar, UnsupportedSchema, ProjectLocked, AliasPath, NotRegistered), `Result`, `SCHEMA_VERSION` |
| `src/sidecar.rs` | `.projectos/project.toml` envelope: read / atomic write (tmp+rename) / validate; rejects future schema versions and escaping content roots |
| `src/registration.rs` | canonical-path identity, idempotent re-register, alias rejection, `open()` writer lock (`.projectos/lock`, `create_new`), `move_project` (UUID preserved) |
| `src/broker.rs` | capability broker: component-wise relative resolution, grants, realpath containment, redacted receipts, symlink write guard |
| `src/worker.rs` | `WorkerHost` trait + `InProcessWorker` (first adapter; proves the interface, makes no confinement claim) |
| `tests/sidecar_registration.rs` | 8 contract acceptance tests |
| `tests/broker.rs` | 6 contract acceptance tests |
| `tests/adversarial.rs` | 9 reviewer-authored property tests (escape/alias/lock/redaction) |

**Verification (parent rerun, pinned Rust 1.97.1): `cargo test` → 23/23 pass, 0 failures, no warnings.**
Ownership respected: `Cargo.toml`, `lib.rs`, `INTERFACES.md` were never edited by a subagent.

## Model provenance

| Work | Model | Outcome |
|---|---|---|
| error.rs, sidecar.rs, registration.rs, tests/sidecar_registration.rs | `poolside/laguna-s-2.1:free` | 8/8 contract tests; clean, well-documented code; two contract nits; disclosed its own limitation honestly |
| broker.rs, worker.rs, tests/broker.rs | `poolside/laguna-s-2.1:free` | 6/6 contract tests **but shipped a real symlink write-escape** (see below) |
| broker.rs fix | `deepseek/deepseek-v4-flash-0731` | 9/9 adversarial, 23/23 total, 46 s |
| app/assets/fonts/** | `poolside/laguna-s-2.1:free` | verified byte-for-byte, committed `a85f43b` |
| tests/adversarial.rs; `sidecar::SCHEMA_VERSION` re-export | reviewer | — |

## The finding (why the driver was upgraded)

**Escape:** a brokered WRITE whose granted path resolved through a dangling (or
chained) symlink at the final component **created or overwrote a file outside
the root**. Root cause: `resolve()`'s not-yet-existing-file fallback
canonicalizes the parent and re-attaches the final *name*, so an in-root-looking
path passed the containment check while `std::fs::write` followed the link. This
is a capability-boundary violation, not merely an IO bug.

**Fix (paid driver):** writes now refuse a final component that is a symlink
(`symlink_metadata`), denying with a redacted receipt; intermediate components
remain confined by canonicalize + `starts_with`. Trade-off stated by the fixer
and accepted: a legitimate symlink that stays inside the root is also refused as
a *write target* (a dangling link's ultimate target cannot be proven in-root
without following it).

**Coverage added by the reviewer** (`tests/adversarial.rs`), all passing:
dangling-symlink write, symlink-chain write, symlink-to-existing-outside-file
overwrite, symlinked-directory write, relative/absolute traversal, case-
insensitive alias, canonical-path open, lock release on drop, receipt redaction.

## Contract deviations recorded (not silently decided)

1. **`SCHEMA_VERSION` placement** — defined in `error.rs`; the contract put it in
   `sidecar.rs`. Reviewer added a `pub use` re-export so the contracted path
   resolves. Cosmetic; done.
2. **`register()` alias semantics** — the lexical vs canonical spelling of the
   *same* folder is treated as an alias (false `AliasPath` for callers that
   canonicalize first). `open()` is unaffected. This is an identity-semantics
   question for the next slice; deliberately not patched by the reviewer.
3. **Stale writer lock** — a lock file left by a crashed writer is never
   recovered (no staleness detection). Documented by
   `stale_lock_file_is_not_recovered_yet`; owned by the recovery slice
   (gate-d model integration).

## Decision recorded

Per the owner's rule, the subagent driver was upgraded to
`deepseek/deepseek-v4-flash-0731` once review showed the free model's output
required a security fix. Standing policy: free models for mechanical/scaffolded
work; the paid driver for security-adjacent or integration-heavy work, with
reviewer-authored adversarial tests on both paths.

## Reproduction

```sh
export CARGO_HOME=/private/tmp/projectos-gate-a-cargo
export RUSTUP_HOME=/private/tmp/projectos-gate-a-rustup
export RUSTUP_TOOLCHAIN=1.97.1     # rustup default here is 1.90.0; the crate requires 1.97.1
export PATH=/private/tmp/projectos-gate-a-cargo/bin:$PATH
cd kernel && cargo test
```
