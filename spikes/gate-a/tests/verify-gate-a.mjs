import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const spikeRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const repositoryRoot = path.resolve(spikeRoot, '..', '..');
const read = (relative) => fs.readFileSync(path.join(spikeRoot, relative), 'utf8');
const json = (relative) => JSON.parse(read(relative));

const configuration = json('src-tauri/tauri.conf.json');
const capability = json('src-tauri/capabilities/main-window.json');
const packageJson = json('package.json');
const matrix = json('platform-matrix.json');
const report = JSON.parse(fs.readFileSync(path.join(repositoryRoot, 'evidence/gates/gate-a/report.json'), 'utf8'));
const rust = read('src-tauri/src/main.rs');
const frontend = read('src/main.mjs');

assert.equal(packageJson.devDependencies['@tauri-apps/cli'], '2.12.0');
assert.match(read('src-tauri/Cargo.toml'), /tauri = \{ version = "=2\.12\.0"/);
assert.equal(configuration.app.security.freezePrototype, true);
assert.equal(configuration.app.security.assetProtocol.enable, false);
assert.deepEqual(configuration.app.security.assetProtocol.scope, []);

const csp = configuration.app.security.csp;
for (const directive of [
  "default-src 'self'",
  "base-uri 'none'",
  "form-action 'none'",
  "frame-ancestors 'none'",
  "frame-src 'none'",
  "media-src 'none'",
  "object-src 'none'",
  "script-src 'self'",
  "style-src 'self'"
]) assert.ok(csp.includes(directive), directive);
assert.doesNotMatch(csp, /https:\/\/(?!ipc\.localhost)/);
assert.doesNotMatch(csp, /'unsafe-(inline|eval)'/);

assert.equal(capability.local, true);
assert.deepEqual(capability.windows, ['main']);
assert.deepEqual(capability.permissions, ['core:default']);
assert.doesNotMatch(JSON.stringify(capability), /(fs|http|shell|process|upload|download):/i);

assert.match(rust, /\.on_navigation\(navigation_is_local\)/);
assert.match(rust, /privileged_commands_exposed: 0/);
assert.match(rust, /AtomicBool/);
assert.match(frontend, /renderUntrustedAsText\(byId\('safe-preview'\)/);
assert.doesNotMatch(frontend, /safe-preview['"]\)\.innerHTML/);

assert.equal(matrix.schema_version, 1);
assert.equal(matrix.frozen, true);
assert.ok(matrix.targets.length >= 3);
for (const target of matrix.targets) {
  for (const field of ['id', 'os_release', 'architecture', 'package_formats', 'display_server', 'gpu_modes']) {
    assert.ok(target[field], `${target.id}: ${field}`);
  }
}

const expectedCriteria = [
  'installation', 'signing_notarization', 'file_dialogs', 'drag_drop', 'clipboard',
  'ime', 'native_menus', 'markdown_latex_editing', 'pdf_text_selection',
  'large_virtualized_lists', 'webview_accessibility_tree', 'zoom_200_percent',
  'ipc_cancellation', 'gpu_launch', 'no_gpu_launch', 'untrusted_markdown_sanitized',
  'untrusted_html_sanitized', 'untrusted_pdf_sanitized',
  'strict_csp_no_remote_assets', 'unexpected_navigation_blocked',
  'privileged_ipc_unavailable'
];
assert.deepEqual(report.criteria.map(({ id }) => id), expectedCriteria);
for (const criterion of report.criteria) {
  assert.ok(['pass', 'fail', 'untested'].includes(criterion.status), criterion.id);
  assert.ok(Array.isArray(criterion.evidence), criterion.id);
}

console.log(JSON.stringify({
  status: 'pass',
  checks: 31,
  note: 'Static harness/configuration contract passed; this is not native platform evidence.'
}, null, 2));
