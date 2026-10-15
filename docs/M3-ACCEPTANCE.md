# M3 Software acceptance criteria

Date: 2026-10-13. Status: review criteria; this document records no passing evidence.

The current authorized slice is the local repository, managed-worktree and patch-review foundation. A usable foundation is progress on M3, not completion of its Software golden path. [The Gate C owner decision](DECISION-gate-c-scope.md) still holds worker isolation and constrained computer use pending signing/notarization resources and a Linux environment. Running user-repository test commands or enabling automated desktop control is outside this slice. Tests of the trusted broker against disposable fixtures are permitted verification.

## Required foundation evidence

| Area | Acceptance evidence |
|---|---|
| Repository identity | Opening a declared repository does not silently discover a parent repository. Repository root, Git directory/common directory and managed-worktree identity are bound to the selected project. Symlinked roots/metadata, unexpected linked repositories and nested repositories are rejected or explicitly supported with confinement evidence. Bare/unborn repositories receive truthful diagnostics. |
| Git process boundary | Arguments use a fixed allowlist and are passed as structured arguments, never shell strings. Ambient `GIT_*` variables cannot redirect the repository, index, object store, configuration or executable behavior. Hooks, filters/textconv, fsmonitor, credential helpers, external diff and pager/editor launch are disabled or have explicit tested policy. No network command or automatic local commit is available through the foundation. |
| Repository view | Staged, unstaged and untracked state are distinguished; unusual filenames are parsed without splitting on whitespace/newlines. Binary, oversized, rename and mode changes are truthful and bounded. Status/diff do not execute repository-controlled programs. Errors do not pretend the checkout is clean. |
| Worktree creation | A run creates staging under a broker-owned location from an exact recorded base object ID. Run/project identity is retained. The canonical checkout, index and user files remain unchanged. The worktree is never registered as another ProjectOS project and receives no independent canonical `.projectos` graph. Repository hooks/filters cannot run during checkout. |
| Worktree lifecycle | Every read and write resolves a managed run owned by the current project, never an arbitrary UI-supplied absolute path. Cancel/removal does not delete user checkouts or external paths. Removal of changed staging is explicit; failed or interrupted cleanup is observable and recoverable. |
| Patch review | A proposal records the exact base object ID, source run, paths, before/after content hashes and bounded diff. The UI reviews those same bytes. Changing staging after review invalidates the proposal instead of changing the accepted content silently. Unsupported file types, symlinks, submodules and traversal paths are rejected visibly. |
| Canonical acceptance | Revalidate the base ref and each touched file immediately before applying. Preserve dirty, staged and untracked conflicts. Accept only declared paths and bytes. Reject stale proposals. Canonical changes use the existing graph-owned mutation/revision protocol and retain before-images; a partial multi-file failure must not report full success or silently lose earlier edits. |
| Receipts | Record project/run IDs, base and resulting tree/content identity, capability scope, explicit approval, changed paths, result and failures. Do not claim a commit/tree test result where only individual file hashes were observed. Sensitive content and machine paths follow the current redaction contract. |
| UI integration | Opening Software view, status refresh, managed staging, bounded patch review and accept/reject are tested in the actual app. Keyboard access, visible focus, useful errors and stale-state feedback are inspected. Worker/test/computer-use controls remain visibly held, with no placeholder green success. |

Run fixtures in disposable directories, including two independent repositories, canonical dirty/staged/untracked conflicts, stale HEAD/file/staging changes, paths containing whitespace/newlines, nested repositories, symlinked Git metadata and hooks/filter/fsmonitor marker programs. Verify both expected output and absence of marker side effects.

## Remaining M3 exit evidence

The [roadmap](../design/projectos-live-mvp-roadmap.html) requires two repositories to produce reviewed patches in isolation without silent commit, push, PR creation or application control. Complete M3 additionally requires:

1. A bounded Software recipe with an implementation-plan artifact and isolated worker capabilities, after Gate C resources and enforcement evidence allow it.
2. Test results bound to the exact commit/tree tested, with cancellation, resource ceilings and visible failures.
3. The eight constrained-computer-use criteria retained in Gate C, including approved target identity, visible consent, bounded/redacted capture, emergency stop and pause after identity or permission loss.
4. Explicit local commit approval and separate remote-operation approval/offline enforcement, with ambiguous external outcomes reconciled rather than retried silently.
5. End-to-end receipts and macOS/Linux evidence plus the owner's final workflow acceptance.

No automated test count, file presence, mock bridge or static HTML inspection substitutes for those gates.
