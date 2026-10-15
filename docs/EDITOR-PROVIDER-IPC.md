# Draft editor and live provider integration

Implementation contract for the current sprint. Ordinary UTF-8 Markdown/text
files remain canonical. Native commands hold the project writer session;
renderer calls never supply API endpoints or receive stored credentials.

## Renderer commands (snake_case result fields; camelCase invoke arguments)

- `workspace_open(folder?: string, create: bool)` — a missing folder opens a
  native folder picker. `create` explicitly authorizes registration. Returns
  `Workspace` or null on picker cancellation.
- `workspace_refresh()` — returns the active `Workspace`.
- `workspace_close()` — releases the writer session; refuses during a request.
- `app_confirm_exit()` — confirms an app quit after the renderer's unsaved-text
  review. Refuses active requests and pending proposals, releases the native
  writer session before process exit. Both window Close and macOS Quit/Cmd–Q
  use this path; native `app-close-requested` asks the renderer to begin review.
- `draft_open(relPath: string)` — returns `Document`.
- `draft_create(relPath: string)` — creates a new empty `.md`/`.txt` file without
  overwriting; returns `Document`.
- `draft_save(relPath: string, expectedHash: string, text: string)` — saves with
  stale-write protection and a retained before-image; returns `Document`.
- `provider_status()` — `{ configured: bool, online: bool, busy: bool,
  unresolved: bool, endpoint: string }`.
- `provider_set_online(online: bool)` — offline cancels in-flight work and never
  queues/replays a request; returns status.
- `provider_store_key(apiKey: string)` / `provider_forget_key()` — native OS
  credential storage only. No secret returned. Renderer clears key input.
- `action_prepare(relPath, expectedHash, byteStart, byteEnd, action, model,
  maxTokens, ceilingUsd)` — actions are `Rephrase`, `Complete`, `Explain`,
  `FactCheck`, `ReviewDraft`. Requires a saved draft, online opt-in and exact
  model ID. Fetches public price metadata and freezes the scoped request.
  Returns `{ id, action, rel_path, selected_text, requested_model, endpoint,
  max_tokens, ceiling_usd, estimated_upper_usd, annotation }`.
- `action_execute(preparedId: string)` — called only after the user reviews the
  exact selected text, destination, model and budget and clicks Send. Returns
  `ProposalResult`. No silent retries or fallback. A native event
  `provider-progress` carries `{ attempt_id, text }` (untrusted text only).
- `action_cancel()` — aborts transport; uncertain remote spend is recorded.
- `action_decide(proposalId: string, accept: bool)` — native-stored proposal,
  revalidates draft hash. Replacement acceptance changes the selected bytes;
  annotation acceptance stores a linked artifact and leaves draft bytes intact.
  Returns updated `Workspace`.
- `provider_reconcile(attemptId: string)` — explicitly acknowledges a prior
  unknown outcome before a renewed approval; does not repeat the request.
- `draft_milestone(name: string)` — checkpoint active project graph.
- `draft_export(relPath: string)` — native Save dialog exports saved draft
  content; returns the written destination or null on cancellation.

`Document = { rel_path, text, hash }`.
`Workspace = { name, folder, uuid, documents: string[], objects: Object[],
  edges: Edge[], receipts: Receipt[] }`.
`Object = { id, kind, label, fields, x, y }`; relation names use `REVISION_OF`,
`PRODUCED`, `SUPPORTS`, etc. Canvas/list/trace read the same native records.
`Receipt` extends existing redacted receipt shape with optional requested_model,
resolved_model, usage_tokens, estimated_upper_usd, provider_cost_usd,
billed_usd, cost_state, attempt_id. Unknown billed cost stays null.
`ProposalResult = { id, action, rel_path, original, proposed, annotation, truncated,
requested_model, resolved_model, usage_tokens, provider_cost_usd,
estimated_upper_usd, cost_state, attempt_id }`.

## Scope

First live adapter: OpenRouter BYOK, fixed HTTPS endpoints. Local editing works
offline. Online is off on startup. No account/OAuth, automatic transmission,
silent model fallback, worker execution or third-party effects installation.
Credential and billed live-provider conformance require owner opt-in and are
reported separately from synthetic local HTTP integration tests.

Fresh command-dispatch, native-desktop and public-provider verification is
recorded in [the October 13 verification addendum](VERIFICATION-m2b-2026-10-13.md).
