const { test } = require('node:test');
const assert = require('node:assert/strict');
const { byteSelection, isDirty, canApply, requestOptions, canEdit, editorText, diskText, lineEndingInfo, needsCloseGuard, needsReconciliation } = require('../editor-state.js');
test('textarea UTF-16 ranges become exact UTF-8 byte scopes', () => {
  const text = 'A😀 café\nα';
  const range = byteSelection(text, 3, 8);
  assert.deepEqual(range, { byteStart: 5, byteEnd: 11 });
  const bytes = new TextEncoder().encode(text);
  assert.equal(new TextDecoder('utf-8', { fatal: true }).decode(bytes.slice(range.byteStart, range.byteEnd)), ' café');
  assert.deepEqual(byteSelection(text, 0, 0, true), { byteStart: 0, byteEnd: bytes.length });
});
test('empty or split-surrogate scopes fail instead of sending altered text', () => {
  assert.throws(() => byteSelection('abc', 1, 1), /Select/);
  assert.throws(() => byteSelection('😀', 0, 1), /complete characters/);
  assert.throws(() => byteSelection('😀', 1, 2), /complete characters/);
  assert.throws(() => byteSelection('a', -1, 1), /Invalid/);
  assert.throws(() => byteSelection('a', 0, 2), /Invalid/);
});
test('acceptance requires exact original document path, saved hash and clean text', () => {
  const doc = { rel_path: 'draft.md', hash: 'original', text: 'saved text' };
  const proposal = { rel_path: 'draft.md' };
  assert.equal(isDirty(doc, 'saved text'), false);
  assert.equal(isDirty(doc, 'changed text'), true);
  assert.equal(canApply(proposal, doc, 'saved text', 'original'), true);
  assert.equal(canApply(proposal, doc, 'changed text', 'original'), false);
  assert.equal(canApply(proposal, { ...doc, hash: 'new' }, 'saved text', 'original'), false);
  assert.equal(canApply({ rel_path: 'other.md' }, doc, 'saved text', 'original'), false);
  assert.equal(canApply(proposal, null, '', 'original'), false);
});

test('the native provider limits are enforced, including explicit free-route budgets', () => {
  assert.deepEqual(requestOptions(' org/model ', '8192', '0'), { model: 'org/model', maxTokens: 8192, ceilingUsd: 0 });
  assert.equal(requestOptions('org/model', 1, 10).ceilingUsd, 10);
  for (const tokens of [0, 8193, 1.5, NaN]) assert.throws(() => requestOptions('org/model', tokens, .05), /tokens/);
  for (const ceiling of ['', -1, 10.01, NaN, Infinity]) assert.throws(() => requestOptions('org/model', 1024, ceiling), /ceiling/);
  assert.throws(() => requestOptions('', 1024, .05), /model ID/);
});
test('pending document operations protect editor bytes while live requests permit editing', () => {
  const doc = { rel_path: 'draft.md', hash: 'h', text: 'saved' };
  assert.equal(canEdit(doc, false), true);
  assert.equal(canEdit(doc, true), false);
  assert.equal(canEdit(null, false), false);
  // AI in-flight state is deliberately separate from document operation state.
  const inFlight = true, operationBusy = false;
  assert.equal(inFlight && canEdit(doc, operationBusy), true);
});

test('CRLF files remain clean and save with their original ending style', () => {
  const doc = { rel_path: 'windows.md', hash: 'raw-hash', text: 'α\r\n😀 café\r\n' };
  const displayed = editorText(doc.text);
  assert.equal(displayed, 'α\n😀 café\n');
  assert.equal(isDirty(doc, displayed), false);
  assert.equal(diskText(doc, displayed), doc.text);
  assert.equal(diskText(doc, displayed + 'new\n'), 'α\r\n😀 café\r\nnew\r\n');
  assert.equal(canApply({ rel_path: doc.rel_path }, doc, displayed, doc.hash), true);
});
test('textarea scopes map through CRLF and multibyte characters to exact raw bytes', () => {
  const raw = 'α\r\n😀 café\r\nlast';
  const display = editorText(raw), start = display.indexOf('😀'), end = display.indexOf('last');
  const bytes = new TextEncoder().encode(raw), selection = byteSelection(raw, start, end);
  assert.deepEqual(selection, { byteStart: 4, byteEnd: 16 });
  assert.equal(new TextDecoder('utf-8', { fatal: true }).decode(bytes.slice(selection.byteStart, selection.byteEnd)), '😀 café\r\n');
  assert.deepEqual(byteSelection(raw, start, end, true), { byteStart: 0, byteEnd: bytes.length });
  assert.throws(() => byteSelection(raw, start + 1, end), /complete characters/);
});
test('mixed line endings are retained byte-for-byte unless edited, then use a disclosed predominant style', () => {
  const doc = { text: 'a\r\nb\nc\r\n' };
  assert.deepEqual(lineEndingInfo(doc.text), { ending: '\r\n', mixed: true });
  assert.equal(diskText(doc, editorText(doc.text)), doc.text);
  assert.equal(diskText(doc, 'a\nb\nc\nd\n'), 'a\r\nb\r\nc\r\nd\r\n');
  const oldMac = { text: 'a\rb\r' };
  assert.equal(diskText(oldMac, 'a\nb\nc\n'), 'a\rb\rc\r');
});
test('clean paid output still protects window closure, and unresolved receipt states are actionable', () => {
  const doc = { text: 'saved' };
  assert.equal(needsCloseGuard(doc, 'saved', { id: 'paid-result' }, false, false), true);
  assert.equal(needsCloseGuard(doc, 'saved', null, false, false), false);
  assert.equal(needsCloseGuard(doc, 'changed', null, false, false), true);
  for (const outcome of ['outcome-unknown', 'in-flight', 'budget-exceeded']) {
    assert.equal(needsReconciliation({ outcome, requires_reconciliation: true }), true);
  }
  assert.equal(needsReconciliation({ outcome: 'reconciled', requires_reconciliation: false }), false);
});

// Execute the actual controller against a minimal DOM/native bridge. These
// tests exercise both lifecycle listeners and their real save/approval gate.
const vm = require('node:vm');
const fs = require('node:fs');
const path = require('node:path');
async function lifecycleHarness() {
  const elements = new Map(), windowEvents = new Map();
  function node(id) {
    if (elements.has(id)) return elements.get(id);
    const n = {
      id, value: '', textContent: '', disabled: false, hidden: false, open: false, modalShows: 0,
      selectionStart: 0, selectionEnd: 0, dataset: {}, listeners: new Map(),
      classList: { toggle() {} },
      setAttribute() {}, replaceChildren() {}, append() {}, appendChild() {}, focus() {},
      closest() { return null; }, querySelector() { return node(id + '-child'); },
      addEventListener(event, callback) { this.listeners.set(event, callback); },
      showModal() { this.open = true; this.modalShows++; }, close() { this.open = false; }
    };
    elements.set(id, n); return n;
  }
  const review = node('review-test-button'); review.dataset.action = 'ReviewDraft';
  const workspace = { name: 'Test project', folder: '/test', uuid: 'test', documents: ['draft.md'], objects: [], edges: [], receipts: [] };
  const native = { calls: [], doc: { rel_path: 'draft.md', text: 'saved', hash: 'original' }, status: { configured: true, online: true, busy: false, unresolved: false }, rejectExit: false };
  const bridge = {
    native: true,
    async listenClose(callback) { native.close = callback; },
    async listenProgress() {},
    async invoke(command, args) {
      native.calls.push({ command, args });
      if (native.customInvoke) { const value = await native.customInvoke(command, args); if (value !== undefined) return value; }
      if (command === 'provider_status') return native.status;
      if (command === 'draft_create' || command === 'draft_open') return { ...native.doc };
      if (command === 'draft_save') { native.doc = { rel_path: args.relPath, text: args.text, hash: 'saved-hash' }; return { ...native.doc }; }
      if (command === 'workspace_refresh') return workspace;
      if (command === 'action_prepare') return { id: 'prepared', action: 'ReviewDraft', rel_path: 'draft.md', selected_text: native.doc.text, requested_model: 'provider/model', endpoint: 'https://openrouter.ai', max_tokens: 1024, ceiling_usd: .05, estimated_upper_usd: .01, annotation: false };
      if (command === 'action_execute') return { id: 'paid-output', action: 'ReviewDraft', rel_path: 'draft.md', original: native.doc.text, proposed: 'suggestion', annotation: false, requested_model: 'provider/model', cost_state: 'reported' };
      if (command === 'app_confirm_exit') { if (native.rejectExit) throw new Error('Native exit refused'); return; }
      throw new Error('Unexpected test command: ' + command);
    }
  };
  const window = {
    projectos: bridge, ProjectOSEditorState: require('../editor-state.js'), projectosViews: { setView() {} },
    __TAURI__: { window: { getCurrentWindow() { return { async onCloseRequested(callback) { native.windowClose = callback; } }; } } },
    addEventListener(event, callback) { windowEvents.set(event, callback); }
  };
  const document = { getElementById: node, createElement: name => node('created-' + name), createTextNode: value => ({ textContent: value }), addEventListener() {}, querySelectorAll: selector => selector === '[data-action]' ? [review] : [] };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../editor.js'), 'utf8'), { window, document, CustomEvent: class {}, TextEncoder });
  await new Promise(resolve => setImmediate(resolve));
  windowEvents.get('workspace-changed')({ detail: workspace });
  node('draft-path').value = 'draft.md';
  await node('create-draft').onclick();
  native.calls.length = 0;
  return { node, native, review, windowEvents };
}
function confirmCalls(native) { return native.calls.filter(call => call.command === 'app_confirm_exit').length; }
test('native app quit and window close share one unsaved gate; Keep editing preserves bytes', async () => {
  const { node, native, windowEvents } = await lifecycleHarness();
  node('draft-text').value = 'unsaved work';
  const quit = native.close();
  assert.equal(node('unsaved-dialog').open, true);
  let prevented = false;
  await native.windowClose({ preventDefault() { prevented = true; } });
  await native.close();
  assert.equal(prevented, true);
  assert.equal(node('unsaved-dialog').modalShows, 1);
  assert.equal(confirmCalls(native), 0);
  const unload = { preventDefault() { this.prevented = true; } };
  windowEvents.get('beforeunload')(unload);
  assert.equal(unload.prevented, true);
  node('unsaved-cancel').onclick(); await quit;
  assert.equal(confirmCalls(native), 0);
  assert.equal(node('draft-text').value, 'unsaved work');
});
test('app quit Save writes the exact draft before native exit confirmation', async () => {
  const { node, native } = await lifecycleHarness();
  node('draft-text').value = 'saved on quit';
  const quit = native.close(); node('unsaved-save').onclick(); await quit;
  assert.equal(native.doc.text, 'saved on quit');
  const commands = native.calls.map(call => call.command);
  assert.ok(commands.indexOf('draft_save') < commands.indexOf('app_confirm_exit'));
  assert.equal(confirmCalls(native), 1);
  assert.equal(node('draft-text').value, 'saved on quit');
});
test('app quit Discard keeps disk bytes unchanged and confirms exactly once', async () => {
  const { node, native } = await lifecycleHarness();
  node('draft-text').value = 'discard this';
  const quit = native.close(); node('unsaved-discard').onclick(); await quit;
  assert.equal(native.doc.text, 'saved');
  assert.equal(node('draft-text').value, 'saved');
  assert.equal(native.calls.some(call => call.command === 'draft_save'), false);
  assert.equal(confirmCalls(native), 1);
});
test('a clean native window close confirms exit without clearing the document', async () => {
  const { node, native } = await lifecycleHarness();
  let prevented = false;
  await native.windowClose({ preventDefault() { prevented = true; } });
  assert.equal(prevented, true);
  assert.equal(node('unsaved-dialog').modalShows, 0);
  assert.equal(confirmCalls(native), 1);
  assert.equal(node('draft-text').value, 'saved');
});
test('native exit refusal retains the editor and allows a subsequent close request', async () => {
  const { node, native } = await lifecycleHarness(); native.rejectExit = true;
  await native.close();
  assert.equal(node('draft-text').value, 'saved');
  assert.equal(node('app-status').textContent, 'Native exit refused');
  native.rejectExit = false; await native.close();
  assert.equal(confirmCalls(native), 2);
});
test('paid proposals and active file operations cannot be bypassed through app quit', async () => {
  const { node, native, review } = await lifecycleHarness();
  node('provider-model').value = 'provider/model'; node('max-tokens').value = '1024'; node('ceiling-usd').value = '.05';
  await review.onclick(); await node('send-action').onclick(); await native.close();
  assert.equal(confirmCalls(native), 0);
  assert.equal(node('proposal-text').textContent, 'suggestion');
  assert.match(node('app-status').textContent, /Accept or reject/);
  const second = await lifecycleHarness(); let finish;
  second.native.customInvoke = command => command === 'workspace_refresh' ? new Promise(resolve => { finish = resolve; }) : undefined;
  const operation = second.node('refresh-project').onclick();
  await second.native.close();
  assert.equal(confirmCalls(second.native), 0);
  assert.match(second.node('app-status').textContent, /Finish.*operation/);
  finish({ name: 'Test', folder: '/test', documents: [] }); await operation;
});
test('app quit refuses an in-flight provider request and a native busy session', async () => {
  const { node, native, review } = await lifecycleHarness();
  node('provider-model').value = 'provider/model'; node('max-tokens').value = '1024'; node('ceiling-usd').value = '.05';
  await review.onclick();
  let finish;
  native.customInvoke = command => command === 'action_execute' ? new Promise(resolve => { finish = resolve; }) : undefined;
  const execution = node('send-action').onclick();
  await native.close();
  assert.equal(confirmCalls(native), 0);
  assert.match(node('app-status').textContent, /Cancel or finish/);
  finish({ id: 'paid-output', action: 'ReviewDraft', rel_path: 'draft.md', original: 'saved', proposed: 'suggestion', annotation: false, requested_model: 'provider/model' });
  await execution;
  const second = await lifecycleHarness(); second.native.status.busy = true;
  await second.native.close();
  assert.equal(confirmCalls(second.native), 0);
  assert.match(second.node('app-status').textContent, /Cancel or finish/);
});
