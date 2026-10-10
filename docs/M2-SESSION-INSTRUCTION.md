# ProjectOS — M2 session instruction (kickoff)

You are the ProjectOS M2 session, started 2026-10-09 by the previous engineering
session. Work in this repo (`/Users/bonorinoa/Documents/ChatGPT/ProjectOS`).
Local-first: local commits are expected, pushes and remotes are not (the owner
decides GitHub owner/name/visibility).

## Load first, in this order

1. Skill `projectos-engineering-lead` (`skill_view`) — the operating rules.
2. `kernel/INTERFACES.md` — the frozen M1 slice contract (API, ownership, 13
   acceptance tests).
3. `docs/projectos-architecture-brief.md` — the product contract: locked
   decisions D-01..D-22, §15 stores/sidecar layout, §18 M1 exit criteria, and
   the M2 definition in the roadmap sections.
4. `~/.hermes/plans/2026-10-09-113000-projectos-m0-m1-takeover.md` — current plan.
5. `design/DECISION-canvas-visual-language.md` + `design/tokens/synodos-field.css`
   — locked UI decisions (open canvas of typed artifacts; Synodos Field).
6. `evidence/gates/` — gate state A–E and the licensing screen
   (`license-screen-helix-metrics.md` §5: the AGPL crate is stripped in the
   vendored fork `vendor/helix-db`).

## Task

1. **Review the M1 thin slice first.** Three parallel agents were implementing
   it at handoff; their outputs may be uncommitted in the working tree
   (`kernel/src/{error,sidecar,registration,broker,worker}.rs`,
   `kernel/tests/*.rs`; `app/assets/fonts/` is already committed as `a85f43b`).
   Verify with the pinned toolchain below, run the 13 acceptance tests from
   `kernel/INTERFACES.md`, and add your own adversarial tests for symlink
   escapes, `..`/absolute path rejection, case-insensitive alias duplicates, and
   second-writer lock races. Commit only what passes. Never trust an agent's
   self-report; rerun the suites yourself.
2. **Review M2 against the brief and the roadmap**, and update the plan where
   reality differs ("with updates if needed"): the plan file,
   `projectos-roadmap-status.json` (regenerate with
   `node scripts/update-roadmap.mjs`, check with `node scripts/test-roadmap.mjs`),
   and the design notes if UI scope shifts.
3. **Implement M2** in the same evidence-gated style: TDD where practical,
   `cargo test` + `cargo clippy` gates, disjoint file ownership for any
   subagents, local commits per task, no remote, honest evidence states
   (`planned|detected|implemented|verified|blocked|accepted`; `accepted` is
   human-only).
4. **Subagent model policy (owner):** default is the cheap non-gated
   `poolside/laguna-s-2.1:free`. Apply the standing rule: if the M1 slice review
   shows the free model's output needed heavy rework, or before
   security-adjacent / multi-crate integration work, switch with
   `hermes config set delegation.model deepseek/deepseek-v4-flash-0731` (the
   owner's chosen upgrade target, ≈$0.11/child).

## Environment (exact — the kernel crate rejects the default toolchain)

```sh
export CARGO_HOME=/private/tmp/projectos-gate-a-cargo
export RUSTUP_HOME=/private/tmp/projectos-gate-a-rustup
export RUSTUP_TOOLCHAIN=1.97.1
export PATH=/private/tmp/projectos-gate-a-cargo/bin:$PATH
```

## Constraints

- Locked decisions D-01..D-22 change only through an ADR.
- Never weaken: ordinary files are authoritative for their bytes; workers get no
  ambient filesystem/network/process/credential authority; receipts distinguish
  observable facts from private traces; final acceptance is human-only.
- Resolve what evidence can resolve; ask the owner only for what is genuinely
  theirs (budget, remote, signing identities, locked-decision changes).
- Known open owner-resources rows: clean-machine/Linux packaging, signing, the
  a11y manual protocol. Do not claim them.
