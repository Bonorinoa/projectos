# ADR: native filesystem syscalls in the kernel document store

**Status:** **Accepted** — owner-approved 2026-10-11
**Date:** 2026-10-11
**Amends:** the recorded decision **D-M2-c** ("no new crate dependencies"),
for the kernel's document layer only.

## Context

M3 introduced `kernel/src/document.rs`, a native `DocumentStore` that opens,
reads, atomically replaces, and enumerates files under a **pinned project root**
without ever following a symlink — including per-descent
(`openat(..., O_NOFOLLOW)`), with atomic `renameat`/`unlinkat` relative to a held
directory descriptor. This is the enforcement mechanism for the invariant
"ordinary project files remain authoritative for their bytes", and for the tested
refusal of symlinked roots / directories / parents and the directory-swap race
(`kernel/tests/document.rs`, `kernel/tests/m3_adversarial.rs`).

`std::fs` cannot express this. It has no per-descent `O_NOFOLLOW`; its
`canonicalize`-then-open is TOCTOU-prone (exactly the case the swap-race test
exercises); it has no `renameat`/`unlinkat` relative to a held fd; and it cannot
pin a directory by descriptor.

## Decision (proposed)

Allow the `libc` crate (v0.2) as a kernel dependency, used **only** inside
`kernel/src/document.rs`, to call `openat` / `mkdirat` / `renameat` / `unlinkat`
/ `fdopendir` / `readdir` / `closedir`. All `unsafe` is confined to that module.
This amends D-M2-c for the document layer; "no new crate dependencies" continues
to hold everywhere else in the kernel.

## Alternatives considered

- **`std::fs` + `canonicalize`** — rejected: TOCTOU; the directory-swap-race
  tests fail closed against it.
- **`openat2` with `RESOLVE_NO_SYMLINKS`** — rejected: Linux-only; no macOS
  equivalent.
- **`cap-std` / `rustix`** — rejected: still a new dependency, heavier, with no
  smaller safety surface than the direct, localized calls.

## Consequences

- The kernel dependency set grows by one foundational, ubiquitous crate.
- The `unsafe` surface is localized to one module and was independently
  code-reviewed as sound (see `kernel/REVIEW-m3-kernel.md`, Finding 2):
  `O_CLOEXEC | O_NOFOLLOW` on every open, `CString` NUL discipline, explicit
  `fdopendir`-failure handling (`close` on NULL, RAII `closedir` on success),
  `O_EXCL` temp sibling swapped with `renameat`, temp unlinked on error only when
  created.
- Residual: an audit gate must record this approval; no fork-maintenance burden
  (`libc` is stable core).

## Acceptance

Owner-approved on 2026-10-11. D-M2-c is amended for the kernel document layer
only: `libc` is an accepted kernel dependency, confined to
`kernel/src/document.rs`. M3's kernel evidence for the symlink / race properties
is **verified** (tests); the dependency itself is now **approved**, recorded here
and noted in the roadmap (`kernel/evidence/status.json`, task `m3.document`).
