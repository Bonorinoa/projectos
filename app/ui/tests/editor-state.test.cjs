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
