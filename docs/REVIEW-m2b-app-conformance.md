# M2b Native App Layer — Conformance Review

Audit target: `docs/EDITOR-PROVIDER-IPC.md` (the contract) vs the native Tauri
layer and renderer. Read-only audit; one file written (this one).

Scope reviewed:
- `app/src-tauri/src/main.rs` (Tauri command layer)
- `app/src-tauri/src/provider_transport.rs` (fixed-endpoint OpenRouter transport)
- `app/src-tauri/src/credentials.rs` (OS-keyring boundary)
- `app/src-tauri/src/workspace.rs` (writer session)
- `app/ui/bridge.js`, `app/ui/editor.js`, `app/ui/editor-state.js`, `app/ui/index.html`, `app/ui/app.js`
- kernel cross-check: `kernel/src/document.rs`, `proposals.rs`, `provider.rs`, `receipts.rs`, `registration.rs`

Method: direct file reads (grep-free). Test evidence run in this review:
`cargo test` in `app/src-tauri` → **19 passed, 2 ignored**; `node --test` in
`app/ui` → **9 passed**.

The two ignored Rust tests are the owner-opt-in live-provider paths:
`live_public_catalog` (provider_transport.rs:681-687, network) and
`native_isolated_keyring_conformance` (credentials.rs:78-95, real Keychain).
Per the contract's Scope (§57-63) these report separately; they are the two
UNPROVEN areas below.

Rating: **PASS** (no material GAPs found), with 2 UNPROVEN live-provider items.

---

## 1. Renderer command conformance

`Result` / command-arg conventions: result fields are snake_case (confirmed in
every output struct below); renderer invoke arguments are camelCase
(`relPath`, `expectedHash`, `byteStart`, `byteEnd`, `maxTokens`, `ceilingUsd`,
`preparedId`, `proposalId`, `attemptId`, `apiKey`, `annotationId`). The Rust
parameters are snake_case (`rel_path`, `byte_start`, …); Tauri v2 renames
command parameters to camelCase for the JS invoke contract by default, so the
two meet. **Sub-item not covered by the automated suites** (neither cargo nor
node exercises the `#[tauri::command]` binding itself); it follows Tauri's
documented default but is UNPROVEN-by-test.

| # | Command (contract) | Result | Verdict | Evidence |
|---|---|---|---|---|
| 1 | `workspace_open(folder?, create)` | `Workspace or null`; folder picker on None; `create` authorizes registration | **PASS** | main.rs:27-35; renderer editor.js:105,118 |
| 2 | `workspace_refresh()` | active `Workspace` | **PASS** | main.rs:36; editor.js:74,139,225,255 |
| 3 | `workspace_close()` | releases session; refuses during a request | **PASS** | main.rs:37 + idle guard :24; editor.js:124 |
| 4 | `draft_open(relPath)` | `Document` | **PASS** | main.rs:38; editor.js:129 |
| 5 | `draft_create(relPath)` | new empty `.md/.txt` without overwriting → `Document` | **PASS** | main.rs:39 → workspace.rs:118; O_EXCL refuse-overwrite document.rs:149 |
| 6 | `draft_save(relPath, expectedHash, text)` | `Document`, stale-write protected, before-image retained | **PASS** | main.rs:40; workspace.rs:134-142; document.rs:170,181 |
| 7 | `provider_status()` | `{configured,online,busy,unresolved,endpoint}` | **PASS** | main.rs:42, ProviderStatus :20 |
| 8 | `provider_set_online(online)` | status; offline cancels in-flight, no queue/replay | **PASS** | main.rs:43; editor.js:184 |
| 9 | `provider_store_key(apiKey)` / `provider_forget_key()` | OS keyring only; no secret returned; renderer clears input | **PASS** | main.rs:44-45; credentials.rs:27,52; editor.js:177,179,181,172-173 |
| 10 | `action_prepare(relPath, expectedHash, byteStart, byteEnd, action, model, maxTokens, ceilingUsd)` | `PreparedView` (see §2) | **PASS** | main.rs:53-77; action list :46-52; renderer editor.js:194 |
| 11 | `action_execute(preparedId)` | `ProposalResult`; `provider-progress` event; no retry/fallback | **PASS** | main.rs:78-104; Progress :21; editor.js:213; listen bridge.js:24 |
| 12 | `action_cancel()` | abort transport; uncertain remote spend recorded | **PASS** | main.rs:105 → receives `remote("cancelled")` outcome-unknown through main.rs:95-96 |
| 13 | `action_decide(proposalId, accept)` | revalidates hash; replacement mutates bytes, annotation leaves bytes intact → `Workspace` | **PASS** | main.rs:106; workspace.rs:143-177; proposals.rs:226-231,234 |
| 14 | `provider_reconcile(attemptId)` | acknowledges unknown outcome, does NOT repeat request → status | **PASS** | main.rs:107 (flag flip + local record only, no network); editor.js:255 |
| 15 | `draft_milestone(name)` | checkpoint active project graph | **PASS** | main.rs:108; workspace.rs:195-210 |
| 16 | `draft_export(relPath)` | native Save dialog exports draft content | **PASS** | main.rs:109-120; editor.js:142 |

Extra (non-contract, additive) command: `artifact_annotation(annotation_id)`
(main.rs:41) reads a stored annotation body for display; not in the contract
but also not a violation.

## 2. Result-shape conformance

| Shape (contract) | Implementation | Verdict | Evidence |
|---|---|---|---|
| `Document={rel_path,text,hash}` | `Document` | **PASS** | workspace.rs:14 |
| `Workspace={name,folder,uuid,documents,objects,edges,receipts}` | `WorkspaceView` | **PASS** | workspace.rs:21-23 |
| `Object={id,kind,label,fields,x,y}` | `CanvasObject` | **PASS** | workspace.rs:16-17 |
| relations `REVISION_OF`/`PRODUCED`/`SUPPORTS`, … | `relation_label` mapping | **PASS** | workspace.rs:223-227 |
| `Receipt` extends redacted shape w/ requested_model, resolved_model, usage_tokens, estimated_upper_usd, provider_cost_usd, billed_usd, cost_state, attempt_id | `Attempt` (+ persisted `RedactedReceipt`) | **PASS** | workspace.rs:26-35, view merging :101-106; kernel receipts.rs:39-52 |
| `ProposalResult={id,action,rel_path,original,proposed,annotation,requested_model,resolved_model,usage_tokens,provider_cost_usd,estimated_upper_usd,cost_state,attempt_id}` | `ProposalResult` (+ additive `truncated`) | **PASS** | main.rs:19 |

## 3. Cross-cutting checks

### 3.1 No renderer-supplied endpoint or credential — **PASS**
- No command accepts an endpoint/base-URL argument; the renderer never sends
  one (all invoke calls in editor.js/bridge.js carry only doc/scope/action/
  model/budget/proposal fields).
- Transport pins all URLs via a `Target` enum: `Completion`,
  `Models`, and — **`#[cfg(test)]`-only** — `Loopback`
  (provider_transport.rs:17-32; `Loopback` at :20, `complete_fixture` at
  :203-211). Production binary contains no configurable endpoint path.
  `ENDPOINT`/`MODELS_ENDPOINT` are hard consts (provider_transport.rs:9-10;
  main.rs:12).
- Renderer never receives a stored key. `credentials::load` returns a
  `Zeroizing<String>` used only to build the `Authorization` header inside the
  transport (provider_transport.rs:232-234, marked sensitive); `store` returns
  `Result<(),String>`, `configured` returns `bool` (credentials.rs:19-49).
  The renderer clears the key field on submit/close (editor.js:177,172-173).

### 3.2 Secret hygiene — **PASS** (live-provider sub-item UNPROVEN)
- `Redactor` keeps an undecided key suffix so a key split across SSE events or
  UTF-8 chunk boundaries is re-composed and redacted before it can escape
  (provider_transport.rs:277-298; char-boundary handling :283).
  Proven by tests: split across events + UTF-8 bytes (provider_transport.rs:
  521-531) and across every byte boundary plus metadata (model/id) (:649-657,
  :660-676).
- `text`, `model` (canonical/resolved), and `id` (generation) fields are each
  key-redacted before surfacing (:351, :356; content via `push` :382-384).
- Errors carry static `&'static str` codes only — no key, no provider body
  (TransportError :89-96). Tests assert `{err:?}` never contains the key
  (:547) and `{response:?}` never does (:675).
- Receipts carry numeric cost and redacted model strings, never the key
  (workspace.rs:26-35, 101-106; main.rs:98). `provider-progress` payload is the
  already-redacted accumulated stream text (main.rs:91; Progress :21).
- **UNPROVEN**: real OS Keychain item (ignored test, credentials.rs:78-95) and
  real OpenRouter round-trip policy (ignored public-catalog test,
  provider_transport.rs:681-687). Both require owner opt-in per contract §62-63.

### 3.3 Offline semantics — **PASS**
- `online` defaults to false (`Inner::default()` via `AppState::default()`,
  main.rs:13-14); renderer starts offline (editor.js:6; status message)
  editor.js:279.
- `provider_set_online(false)` clears `prepared` and sends cancel to the active
  watch channel (main.rs:43); the transport's `cancelled` branch returns
  `remote("cancelled")` (provider_transport.rs:269-271), surfacing as
  `outcome-unknown` + `requires_reconciliation` with the explicit message "No
  retry was made" (main.rs:95-96). There is no queue/replay structure anywhere.

### 3.4 Budget / cost — **PASS**
- `max_tokens` (1..=8192) and ceiling (finite, 0..=10 USD) validated native
  (main.rs:56-57) and in renderer (editor-state.js:52-53); transport additionally
  caps against the frozen model's `max_completion_tokens`/`context_length`
  (provider_transport.rs:53-56) and applies `cost_ceiling` (:64).
- Price/model frozen at `action_prepare`: `ModelInfo.pricing` is captured in the
  `Prepared` (main.rs:76) and re-played at dispatch into the request and the
  `provider.max_price` binding (main.rs:88; provider_transport.rs:238-241), with
  a frozen `allowed_resolved_models` allow-list re-checked at resolution
  (:224-226, :348; resolves only the requested ID or the catalog's exact
  `canonical_slug`).
- `estimated_upper_usd` is a conservative upper bound, explicitly "not billed
  cost" (provider_transport.rs:49-66); billed is never claimed.
- Billed cost stays null while unknown: `billed_usd` is never set in
  `action_execute` (main.rs:97-98 sets only `provider_cost_usd`), `cost_state`
  is `provider-reported | unknown`; UI renders "billed cost unknown" when null
  (app.js:368). Runtime over-ceiling spend flags `budget-exceeded` +
  reconciliation (main.rs:99).

### 3.5 Execution gating — **PASS**
- `action_execute` binds mid-flight state to a one-shot `prepared` matching the
  requested id (main.rs:82, cleared :87), revalidates the exact anchor against
  current disk bytes (:84), requires online (:81), project unchanged + no
  unresolved outcome (:83). Review gate is the renderer approval dialog
  (editor.js:202,209,213) which also re-checks `doc.hash`/dirty before Send.
- **No silent retry**: `reqwest` `redirect none` + `retry::never`
  (provider_transport.rs:99-100); `action_execute` returns on first error with
  "No retry was made" (main.rs:95-96). **No fallback**: `allow_fallbacks:false`,
  `require_parameters:true` in the payload (:238) and `resolved_model_mismatch`
  otherwise (:348).
- `provider_reconcile` only flips `outcome="acknowledged-unknown"` /
  `requires_reconciliation=false` and writes the local record (main.rs:107); it
  performs no request. UI only offers it for `requires_reconciliation` receipts
  (app.js:409-413; editor-state.js:45).

### 3.6 Stale-write revalidation — **PASS**
- `draft_save`: `workspace.save` re-resolves the anchor and refuses if
  `doc.hash != expected` (workspace.rs:126,137); `document.rs` re-compares disk
  bytes to `expected_hash` immediately before rename (:170, :181).
- `action_decide`: acceptance re-anchors against the current doc hash
  (workspace.rs:145) and kernel `apply` re-verifies `before_hash == current disk
  hash` plus recomputed proposal identity (:170-174, :195-208).
- Replacement mutates only the selected bytes (`text[..start] + proposed +
  text[end..]`, proposals.rs:234-249); annotation returns before any document
  write (proposals.rs:226-231) and instead persists `annotations.toml` + a
  linked `Evidence` graph node (workspace.rs:146-168), leaving draft bytes
  intact — confirmed by test `real_editor_save_reopen_stale_and_annotation`
  (workspace.rs:244).

### 3.7 Writer-session discipline — **PASS**
- `workspace_close` calls `idle`, which refuses while `active` is set
  (main.rs:37, :24); `active` is set from prepare through execution and cleared
  only after the connection resolves (main.rs:67,87,93), so close is refused for
  the whole in-flight span.
- Single writer session: `registration::open` creates `.projectos/lock` with
  `create_new(true)`; a concurrent open fails `ProjectLocked`, released on handle
  drop (registration.rs:224-259, :45-52). Confirmed by test
  `session_really_holds_writer_lock` (workspace.rs:252-263).

### 3.8 File authority — **PASS**
- Project files are read from disk on every access; `Document.hash` is
  `stable_hash` of the on-disk UTF-8 bytes (workspace.rs:114-117; provider.rs:73).
  The list is walked from disk (document.rs:244-291). Saves trust only the disk:
  the on-disk bytes must equal `expectedHash` before rename (document.rs:170,181).
- The only app-stored byte copy is the content-addressed **before-image**
  (`.projectos/objects/<hash>`), byte-verified against its own hash on write and
  read (document.rs:233-241, :216-223); it is a recovery artifact identical to
  the superseded file, never an alternative authority over the live document.

## 4. Findings

- **No contractual GAPs found.** Every documented command, result shape, and
  security property (endpoint pinning, secret hygiene, offline cancel/no-replay,
  budget freeze, no-retry/no-fallback gating, stale-write revalidation, writer
  hold, file-authority) is implemented and, except as noted, test-proven.
- **UNPROVEN (expected):** (a) OS Keychain store/load round-trip, and (b) real
  OpenRouter public-price + billed-cost conformance. Both are owner opt-in and
  their tests are `#[ignore]`; the contract defers these to a separate owner
  stage (§62-63).
- **UNPROVEN-by-test (benign):** the exact Tauri camelCase argument binding
  (renderer camelCase → Rust snake_case) is Tauri's documented default and is
  not exercised by either suite. Wire it with an e2e/invoke smoke test if it
  needs to be locked.
- **Observation (benign):** in `action_prepare` there is an async window between
  clearing `active` (main.rs:69) and storing `prepared` (main.rs:76) where a
  concurrent `workspace_close` could slip through `idle`; the resulting `prepared`
  against a cleared workspace fails harmlessly on the next `action_execute`
  ("Open a project first."). No confidentiality/integrity impact; not a GAP.
- **Observation:** `ProposalResult` adds `truncated` beyond the contract's field
  list, and the app adds a non-contract `artifact_annotation` command — both
  additive, neither a divergence.
- Automated evidence reproduced in this review: cargo **19 passed / 2 ignored**
  (0.26s); node **9 passed** (75ms).