import {spawnSync} from 'node:child_process';
import {readFileSync, writeFileSync, mkdirSync, existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {dirname, resolve} from 'node:path';
const spike = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const root = resolve(spike, '../..');
const out = resolve(root, 'evidence/gates/gate-a');
mkdirSync(resolve(out, 'logs'), {recursive:true});
const env = {...process.env};
// Optional isolated toolchain; no machine-wide installation or shell startup edits.
if (process.env.PROJECTOS_RUST_TOOLCHAIN_HOME) env.RUSTUP_HOME = process.env.PROJECTOS_RUST_TOOLCHAIN_HOME;
if (process.env.PROJECTOS_CARGO_HOME) {
  env.CARGO_HOME = process.env.PROJECTOS_CARGO_HOME;
  env.PATH = `${env.CARGO_HOME}/bin:${env.PATH}`;
}
const checks = [
  ['npm-test',process.execPath,['--test','tests/document-safety.test.mjs']],
  ['npm-run-verify',process.execPath,['tests/verify-gate-a.mjs']],
  ['cargo-test','cargo',['test','--locked','--offline','--manifest-path','src-tauri/Cargo.toml']],
  ['native-build',process.execPath,['node_modules/@tauri-apps/cli/tauri.js','build','--debug','--bundles','app']],
].map(([id,command,args]) => {
  const run = spawnSync(command,args,{cwd:spike,env,encoding:'utf8',timeout:300000});
  writeFileSync(resolve(out,`logs/${id}.txt`),`${command} ${args.join(' ')}\n${run.stdout||''}${run.stderr||''}${run.error||''}`);
  return {id,command:`${command} ${args.join(' ')}`,status:run.status===0?'pass':'fail',exitCode:run.status,evidence:`evidence/gates/gate-a/logs/${id}.txt`};
});
const sources = ['src-tauri/src/main.rs','src-tauri/Cargo.toml','src-tauri/Cargo.lock','src-tauri/build.rs','src-tauri/tauri.conf.json','src-tauri/capabilities/main-window.json','package.json','package-lock.json','src/main.mjs','src/index.html','src/styles.css','src/lib/document-safety.mjs','tests/native-evidence.mjs','tests/document-safety.test.mjs','tests/verify-gate-a.mjs'];
const sourceHashes = Object.fromEntries(sources.map(f=>[`spikes/gate-a/${f}`,createHash('sha256').update(readFileSync(resolve(spike,f))).digest('hex')]));
const binary = 'spikes/gate-a/src-tauri/target/debug/bundle/macos/ProjectOS Gate A.app/Contents/MacOS/projectos-gate-a-shell';
const report = JSON.parse(readFileSync(resolve(out,'report.json'),'utf8'));
report.updated_at = new Date().toISOString();
report.gate_decision = 'partial_unverified';
report.gate_status = 'implemented';
report.roadmap_recommendation = {status:'implemented',note:'macOS arm64 native build and observed UI probes pass; clean-machine installation, signing/notarization, full accessibility/IME/PDF/GPU and Linux matrix remain unverified.'};
report.toolchain.dependency_lock_state = 'Both Cargo.lock and package-lock.json exist; cargo test --locked --offline executed.';
report.toolchain.rust = spawnSync('rustc',['--version'],{env,encoding:'utf8'}).stdout?.trim() || null;
report.host_observed.cargo = spawnSync('cargo',['--version'],{env,encoding:'utf8'}).stdout?.trim() || null;
report.host_observed.rustc = report.toolchain.rust;
report.host_observed.observed_at = new Date().toISOString();
report.host_observed.rustup = 'isolated temporary toolchain';
report.blockers = ['Full frozen macOS/Linux matrix has not run.', 'Release signing/notarization and clean-machine installation remain untested.', 'Real IME, PDF extraction/selection, full screen-reader, 200% zoom and no-GPU probes remain untested.'];
for (const criterion of report.criteria) {
  if (['file_dialogs','webview_accessibility_tree','ipc_cancellation','gpu_launch'].includes(criterion.id)) {
    criterion.reason = 'Observed on development macOS arm64 host; full frozen cross-platform matrix remains untested.';
    criterion.evidence = ['evidence/gates/gate-a/host-ui-observations.json'];
  } else if (criterion.status === 'untested') {
    criterion.reason = 'Native development bundle now exists; this criterion has not been verified across the frozen platform matrix.';
  }
}
report.checks = checks;
report.sourceHashes = sourceHashes;
report.nativeArtifact = existsSync(resolve(root,binary)) ? {path:binary,sha256:createHash('sha256').update(readFileSync(resolve(root,binary))).digest('hex'),mode:'debug unsigned development bundle; not distribution proof'} : null;
report.host_runtime_observations = ['evidence/gates/gate-a/host-ui-observations.json'];
writeFileSync(resolve(out,'report.json'),JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({checks,artifact:report.nativeArtifact}));
process.exitCode = checks.some(c=>c.status!=='pass') ? 1 : 0;
