const { test } = require('node:test');
const assert = require('node:assert/strict');
const { createController } = require('../software.js');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

test('bridge sends Tauri camelCase Software bindings', async () => {
  const calls = [];
  const window = { __TAURI__: { core: { async invoke(command, args) { calls.push([command, args]); return null; } } }, dispatchEvent() {} };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../bridge.js'), 'utf8'), { window, CustomEvent: class {} });
  await window.projectos.softwareReview('worktree-id');
  await window.projectos.softwareExportReview('review-id', 'content-hash');
  assert.deepEqual(JSON.parse(JSON.stringify(calls)), [
    ['software_review', { worktreeId: 'worktree-id' }],
    ['software_export_review', { reviewId: 'review-id', contentHash: 'content-hash' }]
  ]);
});

function harness() {
  const ids = new Map();
  function node(tag) {
    const n = {
      tag, children: [], attributes: {}, listeners: {}, textContent: '', disabled: false,
      hidden: false, open: false, classList: { toggle() {} },
      setAttribute(k, v) { this.attributes[k] = v; },
      append(...children) { this.children.push(...children); },
      replaceChildren(...children) { this.children = children; },
      addEventListener(event, handler) { this.listeners[event] = handler; },
      focus() {}, showModal() { this.open = true; }, close() { this.open = false; }
    };
    Object.defineProperty(n, 'id', { set(value) { ids.set(value, this); this._id = value; }, get() { return this._id; } });
    return n;
  }
  const document = { createElement: node, body: node('body'), activeElement: node('button') };
  const calls = [], repo = { root: '/main', head: 'base', branch: 'main', dirty_files: ['draft.md'] };
  const worktree = { id: 'stage', path: '/staging', base_head: 'base' };
  const review = { id: 'review', worktree_id: 'stage', base_head: 'base', content_hash: 'fingerprint', files: [{ path: '<img src=x onerror=alert(1)>.txt', status: 'modified', before_hash: 'before', after_hash: 'after', diff: '-old\n+<script>bad()</script>' }], capability_notes: ['Local-only review'] };
  let permit = true;
  const bridge = {
    native: true,
    async softwareStatus() { calls.push(['inspect']); return repo; },
    async softwareCreateWorktree(confirmed) { calls.push(['create', confirmed]); return worktree; },
    async softwareReview(id) { calls.push(['review', id]); return review; },
    async softwareExportReview(id, hash) { calls.push(['export', id, hash]); return '/out.patch'; }
  };
  const controller = createController(document).init({ bridge, onBeforeAction: async () => permit });
  controller.setProject({ uuid: 'project', name: 'My repo', declared_type: 'software' });
  function all(n = document.body) { return [n, ...n.children.flatMap(child => all(child))]; }
  function text(n = document.body) { return n.textContent + n.children.map(child => text(child)).join(''); }
  async function click(id) { return ids.get(id).listeners.click(); }
  return { controller, bridge, calls, repo, worktree, review, ids, all, text, click, deny() { permit = false; } };
}

test('opening only inspects; create, review and export each require explicit action', async () => {
  const h = harness();
  await h.controller.open();
  assert.deepEqual(h.calls, [['inspect']]);
  assert.equal(h.ids.get('software-review').disabled, true);
  await h.click('software-create');
  assert.deepEqual(h.calls, [['inspect'], ['create', true]]);
  assert.equal(h.ids.get('software-create').disabled, true);
  await h.click('software-review');
  assert.deepEqual(h.calls.at(-1), ['review', 'stage']);
  await h.click('software-export');
  assert.deepEqual(h.calls.at(-1), ['export', 'review', 'fingerprint']);
  assert.match(h.ids.get('software-message').textContent, /exported to \/out.patch/);
  for (const label of ['Apply to main checkout', 'Run tests', 'Supervised app check']) {
    assert.equal(h.all().find(n => n.tag === 'button' && n.textContent === label).disabled, true);
  }
});

test('editor guard prevents opening and prevents every backend action after opening', async () => {
  const h = harness(); h.deny();
  assert.equal(await h.controller.open(), false);
  assert.equal(h.controller.isOpen(), false);
  assert.deepEqual(h.calls, []);
  const opened = harness(); await opened.controller.open(); opened.deny();
  assert.equal(await opened.click('software-create'), false);
  assert.deepEqual(opened.calls, [['inspect']]);
});

test('workspace change suppresses stale inspect results and disables staging', async () => {
  const h = harness(); let resolve;
  h.bridge.softwareStatus = () => new Promise(done => { resolve = done; });
  const opening = h.controller.open();
  await new Promise(done => setImmediate(done));
  assert.equal(h.controller.isBusy(), true);
  h.controller.setProject({ uuid: 'other', name: 'Other repo', declared_type: 'software' });
  resolve(h.repo); await opening;
  assert.equal(h.ids.get('software-create').disabled, true);
  assert.equal(h.text().includes('/main'), false);
  assert.equal(h.text().includes('Other repo'), true);
});

test('duplicate creation is suppressed and modal cannot close during mutation', async () => {
  const h = harness(); await h.controller.open(); let resolve;
  h.bridge.softwareCreateWorktree = () => { h.calls.push(['create']); return new Promise(done => { resolve = done; }); };
  const creating = h.click('software-create');
  await new Promise(done => setImmediate(done));
  assert.equal(await h.click('software-create'), false);
  assert.equal(h.controller.close(), false);
  assert.equal(h.controller.isOpen(), true);
  resolve(h.worktree); await creating;
  assert.equal(h.calls.filter(([kind]) => kind === 'create').length, 1);
  assert.equal(h.controller.close(), true);
});

test('a refresh invalidates review, and a stale export error requires a fresh review', async () => {
  const h = harness(); await h.controller.open(); await h.click('software-create'); await h.click('software-review');
  assert.equal(h.ids.get('software-export').disabled, false);
  h.bridge.softwareExportReview = async () => { throw new Error('Reviewed content changed. Review again.'); };
  await h.click('software-export');
  assert.equal(h.ids.get('software-export').disabled, true);
  assert.equal(h.ids.get('software-review-panel').hidden, true);
  assert.match(h.ids.get('software-message').textContent, /content changed/);
  await h.click('software-review');
  await h.click('software-refresh');
  assert.equal(h.ids.get('software-export').disabled, true);
  assert.equal(h.ids.get('software-review-panel').hidden, true);
});

test('review rejects a different worktree or base, and malicious patch text stays inert', async () => {
  const h = harness(); await h.controller.open(); await h.click('software-create');
  h.bridge.softwareReview = async () => ({ ...h.review, base_head: 'different' });
  await h.click('software-review');
  assert.equal(h.ids.get('software-export').disabled, true);
  assert.match(h.ids.get('software-message').textContent, /does not match/);
  h.bridge.softwareReview = async () => h.review;
  await h.click('software-review');
  assert.match(h.text(), /<script>bad\(\)<\/script>/);
  assert.match(h.text(), /<img src=x onerror=alert\(1\)>/);
  assert.equal(h.all().some(n => n.tag === 'script' || n.tag === 'img'), false);
});

test('browser preview and a closed project cannot invoke native Git', async () => {
  const h = harness(); h.bridge.native = false; await h.controller.open();
  assert.deepEqual(h.calls, []);
  assert.equal(h.ids.get('software-refresh').disabled, true);
  h.bridge.native = true; h.controller.setProject(null);
  assert.equal(h.ids.get('software-create').disabled, true);
  assert.equal(h.ids.get('software-export').disabled, true);
  assert.equal(h.text().includes('/staging'), false);
});

test('publication projects keep the Software surface inert', async () => {
  const h = harness();
  h.controller.setProject({ uuid: 'publication', name: 'Article', declared_type: 'publication' });
  await h.controller.open();
  assert.deepEqual(h.calls, []);
  assert.match(h.ids.get('software-message').textContent, /registered as Substack/);
  assert.equal(h.ids.get('software-refresh').disabled, true);
});
