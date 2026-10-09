import {readFileSync, writeFileSync, readdirSync, existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {dirname, resolve, relative} from 'node:path';
import {fileURLToPath} from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const ignore = new Set(['node_modules','target','build','dist','work','.venv','__pycache__','.git']);
const sources = {};
function walk(dir) {
  for (const entry of readdirSync(dir,{withFileTypes:true}).sort((a,b)=>a.name.localeCompare(b.name))) {
    if (entry.name.startsWith('.') || ignore.has(entry.name)) continue;
    const path = resolve(dir,entry.name);
    if (entry.isDirectory()) walk(path);
    else if (entry.isFile()) sources[relative(root,path).replaceAll('\\','/')] = createHash('sha256').update(readFileSync(path)).digest('hex');
  }
}
walk(resolve(root,'spikes'));
walk(resolve(root,'scripts'));
const revision = 'sha256:' + createHash('sha256').update(JSON.stringify(Object.entries(sources).sort())).digest('hex');
const snapshot = {schemaVersion:1, revision, scope:'All non-generated spike and script files; excludes reports, docs, design, build/runtime outputs and caches.', files:sources};
writeFileSync(resolve(root,'projectos-source-snapshot.json'), JSON.stringify(snapshot,null,2)+'\n');
const tasks = {}, diagnostics = [];
for (const [letter,id] of [['a','shell'],['b','graph'],['c','worker'],['d','recovery'],['e','provider']]) {
  const reportPath = `evidence/gates/gate-${letter}/report.json`;
  if (!existsSync(resolve(root,reportPath))) {tasks[`m0.${id}`]={status:'detected',evidence:[],note:'No gate report.'}; continue;}
  const report = JSON.parse(readFileSync(resolve(root,reportPath),'utf8'));
  const hashes = report.sourceHashes || report.source_sha256 || {};
  const missingHashes = Object.keys(hashes).length === 0;
  const mismatches = Object.entries(hashes).filter(([file,hash])=>!existsSync(resolve(root,file)) || createHash('sha256').update(readFileSync(resolve(root,file))).digest('hex')!==hash);
  const failed = (report.checks || []).some(check=>check.status==='fail') || report.mock_contract_status==='fail' || (report.failed || 0)>0 || (report.errors || 0)>0;
  const status = letter==='c' || failed ? 'blocked' : 'implemented';
  const notes = {
    a:'Native macOS arm64 shell builds and launches; local UI and cancellation probes passed. Clean-machine, signing/notarization and full Linux/accessibility/IME/PDF matrix remain open.',
    b:'Python embedded contracts pass on macOS; pinned Rust/Tauri probe evidence is separate. Engine selection, license review, universal/Linux packaging and production-scale memory remain open.',
    c:'Native file broker and active symlink-swap tests pass. Supported production confinement, hard OS memory ceiling and signed cross-platform package remain blocked.',
    d:'Reproducible fault-injection contract model implemented. Real selected graph-engine, power-loss durability and cross-platform filesystem proof remain open.',
    e:'Mock provider, PKCE callback and encrypted-vault contracts implemented. Native credential/egress boundary, OS keyring, live opt-in conformance and Linux remain open.'
  };
  if (mismatches.length) diagnostics.push(`${id}: evidence source mismatch: ${mismatches.map(([p])=>p).join(', ')}`);
  if (missingHashes) diagnostics.push(`${id}: no exact-source hashes recorded; implementation evidence not established`);
  tasks[`m0.${id}`]={status:mismatches.length?'blocked':missingHashes?'detected':status,evidence:[reportPath],note:mismatches.length?'Evidence is stale; rerun after source changes.':missingHashes?'Report lacks exact-source hashes; rerun evidence before trusting implementation status.':failed?'Latest contract checks failed; see report before continuing.':notes[letter]};
  if (letter==='b' && existsSync(resolve(root,'evidence/gates/gate-b/rust-integration.json'))) tasks[`m0.${id}`].evidence.push('evidence/gates/gate-b/rust-integration.json');
}
const manifest = {updatedAt:new Date().toISOString(),commit:revision,sourceSnapshot:'projectos-source-snapshot.json',sourceStats:{files:Object.keys(sources).length,source:Object.keys(sources).filter(f=>/\.(rs|ts|js|mjs|py|c|h|html|css)$/.test(f)).length,tests:Object.keys(sources).filter(f=>/test|spec/.test(f)).length},tasks,diagnostics};
writeFileSync(resolve(root,'projectos-roadmap-status.json'),JSON.stringify(manifest,null,2)+'\n');
const htmlPath=resolve(root,'design/projectos-live-mvp-roadmap.html');
const html=readFileSync(htmlPath,'utf8');
const marker= /<!-- STATUS_SNAPSHOT_START -->[\s\S]*?<!-- STATUS_SNAPSHOT_END -->/;
if (!marker.test(html)) throw new Error('Roadmap snapshot markers missing');
const embedded=`<!-- STATUS_SNAPSHOT_START -->\n  <script id="status-snapshot" type="application/json">${JSON.stringify(manifest).replaceAll('<','\\u003c')}</script>\n  <!-- STATUS_SNAPSHOT_END -->`;
writeFileSync(htmlPath,html.replace(marker,embedded));
console.log(JSON.stringify({revision,tasks,diagnostics},null,2));
process.exitCode=diagnostics.length?1:0;
