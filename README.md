# ProjectOS

> **Frozen 2026-10-15** at tag [`checkpoint-2026-10-15`](https://github.com/Bonorinoa/projectos/tree/checkpoint-2026-10-15); the repository is archived (read-only) while the owner works on other priorities. State: [`docs/CHECKPOINT-2026-10-15.md`](docs/CHECKPOINT-2026-10-15.md) · Resume on a clean machine: [`docs/RESUME-FROM-CLONE.md`](docs/RESUME-FROM-CLONE.md).

A local-first desktop operating environment for long-lived creative and technical projects — a Tauri app with a trusted Rust kernel that adds structure, provenance, and approval-gated AI assistance to ordinary folders, without replacing the files themselves as the authority.

## What problem does it solve?

Most agentic tools treat a project as a chat transcript. ProjectOS treats a project as an **ordinary folder** carrying a portable `.projectos/` sidecar, and puts **artifacts** (documents, code, diffs, evidence maps, receipts) at the center of collaboration instead of prompts. It exists to make AI-assisted work on real projects:

- **Visible and accountable** — every AI action is a typed capability request with a diff, an approval gate, and a durable redacted receipt; nothing is inserted silently.
- **Local and owner-controlled** — offline by default, no required backend or account, files remain ordinary user-owned bytes, secrets stay in the OS keyring.
- **Safe by architecture** — a Rust kernel brokers every file, Git, network, credential, and external-app operation through bounded capability grants; nothing runs with ambient authority.

It targets four project types (Academic, Software, Substack, Learning), with Substack as the first deep MVP vertical and Software second.

## Status

**In-development.** The Rust kernel and the offline Substack golden path are implemented and verified by tests; the Software (M3) vertical and production packaging are in progress. `app` version is 0.0.1; this is not a released or packaged product yet.

Recorded milestone progress (see [`projectos-roadmap-status.json`](projectos-roadmap-status.json), updated 2026-10-15):

| Area | Status |
|---|---|
| M0 proofs (shell / graph / recovery / provider) | Implemented (worker spike blocked) |
| M1 kernel (project registry, `.projectos` sidecar, capability broker, mutation journal, graph/profile split) | Verified by tests |
| M2 Substack golden path (editor, selection actions, diff/revision loop, recipes, receipts) | Implemented/verified **offline on macOS** |
| M2.1/M2.2 (native DocumentStore, OpenRouter transport, OS-keyring credentials) | Implemented; live provider + keyring conformance is owner-opt-in and **unproven** |
| Live provider calls (priced/billed) | **Blocked/not claimed** (owner opt-in) |
| M3 Software vertical (repo, worktree, patch-review) | Local foundation verified on macOS; full UI/effects/e2e remain open |
| Gate C worker isolation | **On hold** — needs a signing/notarization identity and a Linux environment |

**Gate C is on hold.** The worker-isolation core (hard memory ceiling, macOS production confinement, packaging) is parked pending owner resources, and the constrained-computer-use criteria remain part of Gate C (see [`docs/DECISION-gate-c-scope.md`](docs/DECISION-gate-c-scope.md)). No code work is in flight on that slice by decision.

## Build & run

The app requires the **Tauri v2** toolchain with a pinned Rust compiler (1.97.1) and a Node runtime for the UI.

```bash
# all authorized offline checks, source hashes, logs, and debug build
# activate Rust 1.97.1 and Node first; fixture tests need loopback access
node scripts/verify-checkpoint.mjs

# individual locked suites, from the repository root
cargo +1.97.1 test --locked --manifest-path kernel/Cargo.toml
cargo +1.97.1 test --locked --manifest-path app/src-tauri/Cargo.toml
node --test app/ui/tests/*.test.cjs scripts/test-roadmap.mjs

# run the desktop app in dev mode (no installer is built yet: bundle is inactive)
cd app/src-tauri && cargo tauri dev
```

There is **no packaged installer** at this point (`bundle.active` is false) — the dev shell is the runnable surface.

## How do I know it works?

The October 15 checkpoint records 121 kernel, 42 native and 36 frontend/roadmap tests passing, plus syntax, whitespace and debug-build checks. Two native live/keyring tests are explicitly opt-in and ignored. The actual native app was exercised with two repositories, stale review refusal, patch export, keyboard focus and guarded quit. See the [checkpoint and resume guide](docs/CHECKPOINT-2026-10-15.md) for limitations and evidence. This is not signed-release or full M3 acceptance.

After Dark Phoenix is the first permanent Software project. Its sourced article remains an exported proposal, not a canonical edit. The approved Phoenix-only keyword correction is externally capacity-blocked; national metrics are labeled national. No remote was created for that project.

## Deeper documentation

| Doc | What |
|---|---|
| [`docs/projectos-architecture-brief.md`](docs/projectos-architecture-brief.md) | Product thesis, six surfaces, four project types, locked decisions & ADR rules, feasibility gates A–E (**start here**) |
| [`docs/DECISION-gate-c-scope.md`](docs/DECISION-gate-c-scope.md) | Gate C scope and current hold |
| [`docs/M3-ACCEPTANCE.md`](docs/M3-ACCEPTANCE.md) | Software-vertical acceptance criteria (foundation slice) |
| [`docs/CHECKPOINT-2026-10-15.md`](docs/CHECKPOINT-2026-10-15.md) | Verified checkpoint, research blocker and month-pause resume instructions |
| [`docs/ProjectOS-M3-First-Project-System-Design.docx`](docs/ProjectOS-M3-First-Project-System-Design.docx) | Template-preserving System Design proposal |
| `kernel/` | Interfaces and acceptance notes per milestone (`INTERFACES*.md`, `ACCEPTANCE-M1/M2/M2b.md`, `SPRINT-*.md`, `REVIEW-*.md`) |
| [`design/projectos-live-mvp-roadmap.html`](design/projectos-live-mvp-roadmap.html) | Live milestone roadmap (M0→M4) |
| [`projectos-roadmap-status.json`](projectos-roadmap-status.json) · `projectos-source-snapshot.json` | Machine-readable roadmap/source state |
| `evidence/gates/` | Reproducible gate evidence and logs |
