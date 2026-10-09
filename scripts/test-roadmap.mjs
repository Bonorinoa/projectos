import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import { readFileSync, writeFileSync, mkdirSync, mkdtempSync, cpSync, rmSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { createServer } from 'node:http';
import * as fs from 'node:fs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const html = readFileSync(join(root, 'design/projectos-live-mvp-roadmap.html'), 'utf8');
const inline = [...html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/g)].filter(match => !/application\/json/.test(match[1])).map(match => match[2]).join('\n');
const snapshot = JSON.parse(html.match(/<script id="status-snapshot" type="application\/json">([\s\S]*?)<\/script>/)[1]);
function ui(initial = snapshot, saved = null) {
  const elements = new Map(), listeners = new Map(), storage = new Map();
  if (saved) storage.set('projectos-roadmap-manual', JSON.stringify(saved));
  function element(id) {
    if (!elements.has(id)) elements.set(id, { textContent: id === 'status-snapshot' ? JSON.stringify(initial) : '', innerHTML: '', dataset: {},
      style: { setProperty() {} }, classList: { add() {}, remove() {} }, setAttribute() {}, addEventListener() {}, querySelector() { return element('close'); },
      querySelectorAll() { return []; }, scrollIntoView() {}, showModal() {}, close() {} });
    return elements.get(id);
  }
  const context = vm.createContext({ console, Date, JSON, Map, Set, URL, Blob, Array, Object, String, Number, Math,
    document: { getElementById: element, querySelectorAll: () => [], addEventListener: (name, callback) => listeners.set(name, callback) },
    localStorage: { getItem: key => storage.get(key) || null, setItem: (key, value) => storage.set(key, value) },
    location: { protocol: 'file:', hostname: '' }, setTimeout: () => 1, clearTimeout() {}, setInterval() {},
    fetch: async () => { throw Error('unexpected network'); } });
  // Expose closure state for behavioral tests without editing the production HTML.
  const instrumented = inline.replace(/\}\)\(\);\s*$/, 'globalThis.__roadmap = {state, stages, applyManifest, render, manualUnlocked, refreshServedManifest}; })();');
  vm.runInContext(instrumented, context);
  return { context, ...context.__roadmap, elements, storage, listeners };
}
function fullyVerified(commit = 'candidate-one') {
  const probe = ui(), tasks = {};
  for (const stage of probe.stages) if (!stage.manual) for (const task of stage.tasks) tasks[task.id] = { status: 'verified', evidence: ['evidence/fixture.json'] };
  return { commit, updatedAt: new Date().toISOString(), tasks };
}
function accept(u, id = 'manual.signoff') {
  u.listeners.get('change')({ target: { closest: () => ({ dataset: { manualTask: id }, checked: true }) } });
}
function temporaryRepo() {
  const temp = mkdtempSync('/private/tmp/projectos-roadmap-test-');
  for (const dir of ['scripts','spikes','design','evidence']) mkdirSync(join(temp,dir));
  cpSync(join(root,'scripts/update-roadmap.mjs'), join(temp,'scripts/update-roadmap.mjs'));
  cpSync(join(root,'design/projectos-live-mvp-roadmap.html'), join(temp,'design/projectos-live-mvp-roadmap.html'));
  const source = 'spikes/fixture.mjs'; writeFileSync(join(temp,source), 'export const fixture = 1;\n');
  const sourceHashes = { [source]: createHash('sha256').update(readFileSync(join(temp,source))).digest('hex') };
  const reports = {};
  for (const letter of ['a','b','c','d','e']) {
    mkdirSync(join(temp,`evidence/gates/gate-${letter}`), { recursive: true });
    reports[letter] = { sourceHashes, checks: [{ status: 'pass' }], failed: 0, errors: 0, mock_contract_status: 'pass' };
    writeFileSync(join(temp,`evidence/gates/gate-${letter}/report.json`), JSON.stringify(reports[letter]));
  }
  return { temp, source, reports, save(letter) { writeFileSync(join(temp,`evidence/gates/gate-${letter}/report.json`), JSON.stringify(reports[letter])); },
    run() { const result = spawnSync(process.execPath, [join(temp,'scripts/update-roadmap.mjs')], { encoding: 'utf8' });
      assert.ok([0,1].includes(result.status), result.stderr); return JSON.parse(readFileSync(join(temp,'projectos-roadmap-status.json'),'utf8')); },
    cleanup() { rmSync(temp, { recursive: true, force: true }); } };
}
test('inline JavaScript compiles and literal element references exist', () => {
  assert.doesNotThrow(() => new vm.Script(inline));
  const ids = [...html.matchAll(/\bid="([^"]+)"/g)].map(match => match[1]);
  assert.equal(ids.length, new Set(ids).size, 'duplicate HTML IDs');
  for (const match of inline.matchAll(/getElementById\(['"]([^'"]+)['"]\)/g)) assert.ok(ids.includes(match[1]), `missing ${match[1]}`);
  assert.ok(ui().elements.get('roadmap').innerHTML.includes('Owner manual acceptance'));
});
test('malicious imported notes and evidence stay escaped text', () => {
  const u = ui(), attack = '<img src=x onerror="localStorage.setItem(\'projectos-roadmap-manual\',\'evil\')">';
  u.applyManifest({ commit: 'attack', updatedAt: new Date().toISOString(), tasks: { 'm0.shell': { status: 'implemented', note: attack, evidence: [attack] } } }); u.render();
  const rendered = u.elements.get('roadmap').innerHTML;
  assert.ok(!rendered.includes('<img')); assert.ok(rendered.includes('&lt;img')); assert.equal(u.state.statuses['manual.signoff'], 'planned');
});
test('agent manifests cannot mark human tasks accepted or verified', () => {
  const u = ui();
  for (const status of ['accepted','verified']) u.applyManifest({ commit: 'agent-attempt', updatedAt: new Date().toISOString(), tasks: { 'manual.signoff': { status, evidence: ['pretend'] }, 'm0.shell': { status: 'accepted', evidence: ['pretend'] } } });
  assert.equal(u.state.statuses['manual.signoff'], 'planned'); assert.notEqual(u.state.statuses['m0.shell'], 'accepted');
  accept(u); assert.equal(u.state.statuses['manual.signoff'], 'planned');
});
test('manual acceptance persists only for the same fully verified candidate', () => {
  const candidate = fullyVerified(), u = ui(candidate); assert.equal(u.manualUnlocked(), true); accept(u);
  assert.equal(u.state.statuses['manual.signoff'], 'accepted');
  const saved = JSON.parse(u.storage.get('projectos-roadmap-manual'));
  assert.equal(ui(candidate, saved).state.statuses['manual.signoff'], 'accepted');
  assert.equal(ui(fullyVerified('different-candidate'), saved).state.statuses['manual.signoff'], 'planned');
  assert.equal(ui(snapshot, saved).state.statuses['manual.signoff'], 'planned');
});
test('new source revision clears acceptance and inherited nonbaseline verification', () => {
  const u = ui(fullyVerified()); accept(u);
  u.applyManifest({ commit: 'candidate-two', updatedAt: new Date().toISOString(), tasks: {} });
  assert.equal(u.state.statuses['manual.signoff'], 'planned'); assert.equal(u.manualUnlocked(), false);
  assert.notEqual(u.state.statuses['agents.freeze'], 'verified');
});
test('served snapshot mismatch does not apply an unbound manifest', async () => {
  const u = ui(), previous = u.state.manifest.commit; u.context.location = { protocol: 'http:', hostname: '127.0.0.1' };
  let count = 0; u.context.fetch = async () => ({ ok: true, json: async () => ++count === 1 ? { commit: 'stale', updatedAt: new Date().toISOString(), tasks: { 'm0.shell': { status:'verified', evidence:['fixture'] } } } : { revision: 'changed' } });
  await u.refreshServedManifest(); assert.equal(u.state.manifest.commit, previous);
  assert.match(u.elements.get('snapshot-note').textContent, /Source revision differs/);
});
test('fresh failing gate results block roadmap promotion', () => {
  const repo = temporaryRepo();
  try {
    repo.reports.a.checks[0].status='fail'; repo.save('a'); repo.reports.d.errors=1; repo.save('d'); repo.reports.e.mock_contract_status='fail'; repo.save('e');
    const manifest=repo.run(); for (const id of ['shell','recovery','provider']) assert.equal(manifest.tasks[`m0.${id}`].status,'blocked');
  } finally { repo.cleanup(); }
});
test('reports without any source hashes cannot claim implemented evidence', () => {
  const repo = temporaryRepo();
  try { repo.reports.e.sourceHashes={}; repo.save('e'); assert.notEqual(repo.run().tasks['m0.provider'].status,'implemented'); }
  finally { repo.cleanup(); }
});
test('changed source changes revision and blocks stale source-hashed gate results', () => {
  const repo = temporaryRepo();
  try { const first=repo.run(); writeFileSync(join(repo.temp,repo.source),'export const fixture = 2;\n'); const second=repo.run();
    assert.notEqual(second.commit,first.commit); assert.equal(second.tasks['m0.shell'].status,'blocked');
    assert.equal(JSON.parse(readFileSync(join(repo.temp,'projectos-source-snapshot.json'),'utf8')).revision,second.commit);
  } finally { repo.cleanup(); }
});
test('server refresh fingerprints code and evidence without a network socket', () => {
  const repo = temporaryRepo();
  try {
    const serverSource=readFileSync(join(root,'scripts/serve-roadmap.mjs'),'utf8').replace(/^import .*;\n/gm,'')
      .replace(/const root=.*;\n/,`const root=${JSON.stringify(repo.temp)};\n`)+ '\n globalThis.__refresh=refreshEvidence;';
    const context=vm.createContext({ console:{log(){}}, URL, createHash, spawnSync, ...fs, readFile:async()=>{},
      fileURLToPath, dirname, resolve, process, createServer:()=>({listen(){}}) });
    vm.runInContext(serverSource,context); context.__refresh();
    const first=JSON.parse(readFileSync(join(repo.temp,'projectos-roadmap-status.json'),'utf8'));
    context.__refresh(); assert.equal(JSON.parse(readFileSync(join(repo.temp,'projectos-roadmap-status.json'),'utf8')).updatedAt,first.updatedAt);
    writeFileSync(join(repo.temp,repo.source),'export const fixture = 3;\n'); context.__refresh();
    const second=JSON.parse(readFileSync(join(repo.temp,'projectos-roadmap-status.json'),'utf8')); assert.notEqual(second.commit,first.commit); assert.equal(second.tasks['m0.shell'].status,'blocked');
  } finally { repo.cleanup(); }
});
