# M2b editor and OpenRouter verification — October 13, 2026

This addendum verifies the current implementation against the editor/provider
contract. It supplements the earlier app conformance review; it does not grant
owner acceptance or change the separate Gate C scope decision.

## Implemented behavior

- The desktop workbench opens, creates, edits, previews and saves ordinary
  Markdown/text files through the native writer session. Saved bytes remain
  canonical, with retained before-images and stale-write protection.
- OpenRouter is the first live adapter. The user enters their own key in the
  app's OS credential store and opts into online access each session. A request
  reviews the exact saved passage, model, destination and budget before Send.
- Native-owned proposals support explicit acceptance/rejection. Annotations
  leave draft bytes intact. Streaming, cancellation, truncation and unknown
  remote outcomes are represented without silent retries or invented billing.
- Window Close and application Quit share Save / Discard / Keep editing review.
  The native exit command refuses active requests and pending proposals and
  releases the writer lock before exiting.

## Gaps closed during this verification

The earlier review did not exercise the registered Tauri command dispatcher.
Three tests now share the production command handler and cover camelCase invoke
arguments, native serialization, writer-lock lifecycle, stale saves, path
refusal, offline request refusal, native proposal authority, annotations,
consumed/stale proposals and exit-policy refusals. The successful exit policy
uses its production helper in the mock because MockRuntime does not implement
process exit; actual process exit was checked through Computer below.

The Computer walkthrough found that macOS's predefined Quit menu invokes
AppKit termination directly, bypassing both the window-close listener and
Tauri's cancellable exit event. A custom Quit menu action now sends the same
guarded close request as window Close. The standard editing/window menus remain.
The renderer no longer needs permission to destroy the window directly.

Export feedback now identifies the written destination or explicitly reports
cancellation, instead of reporting only that the dialog closed.

## Verification results

| Check | Result | Scope of evidence |
| --- | --- | --- |
| Kernel locked test suite | 121 passed | Current kernel filesystem, mutation, proposals, recovery and existing behavior |
| Native locked test suite | 22 passed; 2 opt-in tests excluded by default | Native workspace, real local HTTP transport, parser/limits/redaction and registered command dispatch |
| Frontend Node test suite | 16 passed | Unicode/CRLF scope invariants and actual editor controller quit handling |
| Public OpenRouter catalog opt-in | 1 passed | Real public HTTPS metadata for `openai/gpt-4o-mini`; no key, draft or inference |
| Isolated OS Keychain opt-in | 1 passed | Synthetic key round trip under a unique test service, cleaned up; production key untouched |
| Native debug build | Passed | Current macOS source and bundled frontend |
| Syntax and whitespace checks | Passed | Frontend scripts and `git diff --check` |

The local HTTP tests require permission to bind loopback sockets. A restricted
attempt failed at socket creation; the same suite passed with loopback permitted.
These failures did not reach provider dispatch.

## Live Computer walkthrough

Used the rebuilt native app and disposable project
`/private/tmp/projectos-editor-20261013-quit`; no user draft was modified.

1. Registered the folder through the absolute-path dialog and opened `draft.md`.
2. Edited text containing `café` and `🌍`; preview displayed a literal
   `<script>fixture()</script>` as text.
3. Cmd–Q showed Unsaved changes. Keep editing preserved the unsaved text.
4. Cmd–Q then Save and continue wrote exact expected UTF-8 bytes and removed
   `.projectos/lock` before exit.
5. Relaunched, reopened the same folder without stale-lock recovery, and loaded
   the saved bytes.
6. Saved a milestone and checked the persisted `milestones.toml` record.
7. Exported through the native Save dialog to
   `/private/tmp/projectos-verified-export-20261013.md`; disk bytes matched the
   saved draft exactly.
8. Window Close with a dirty draft showed the same dialog. Discard preserved
   the saved file and removed the writer lock.
9. In the final build, export cancellation displayed the explicit cancellation
   message; a clean window Close exited and released the lock.
10. OpenRouter settings showed no configured production key and online access
    off. No credential was entered and no paid inference was attempted.

The temporary review bundle initially linked to the repository executable. A
later relaunch stalled inside the macOS loader before app startup. Copying the
verified executable into the temporary bundle resolved that verification-harness
issue. The final build launched and completed the checks above.

## Remaining boundaries

Account-backed inference and billed-cost conformance are untested without an
owner-supplied OpenRouter key and approved request. Linux credential conformance,
signed distribution, the native folder Browse workflow and Gate C computer-use
worker infrastructure are outside this verification. This is implementation
verification, not a signed release or human acceptance claim.

Source fingerprints for this verification are in
`../evidence/m2b/verification-2026-10-13-source-sha256.json`.
