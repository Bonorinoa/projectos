import {spawnSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {dirname,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
const dir=dirname(fileURLToPath(import.meta.url)),root=resolve(dir,'../../..'),out=resolve(root,'evidence/gates/gate-b');
mkdirSync(resolve(out,'logs'),{recursive:true});
const env={...process.env};
env.RUSTUP_TOOLCHAIN='1.97.1';
if(process.env.PROJECTOS_CARGO_HOME) {env.CARGO_HOME=process.env.PROJECTOS_CARGO_HOME;env.PATH=`${env.CARGO_HOME}/bin:${env.PATH}`;}
if(process.env.PROJECTOS_RUST_TOOLCHAIN_HOME) env.RUSTUP_HOME=process.env.PROJECTOS_RUST_TOOLCHAIN_HOME;
const checks=[];
function execute(id,cmd,args,timeout=300000) {
  const r=spawnSync(cmd,args,{cwd:dir,env,encoding:'utf8',timeout,maxBuffer:32*1024*1024});
  writeFileSync(resolve(out,`logs/rust-${id}.txt`),`${cmd} ${args.join(' ')}\n${r.stdout||''}${r.stderr||''}${r.error||''}`);
  checks.push({id,status:r.status===0?'pass':'fail',exitCode:r.status,evidence:`evidence/gates/gate-b/logs/rust-${id}.txt`});
  return r;
}
const build=execute('desktop-build','cargo',['+1.97.1','build','--locked','--offline','--features','desktop']);
const tests=build.status===0?execute('rust-tests','cargo',['+1.97.1','test','--locked','--offline','--features','desktop']):{status:null};
const bundle=tests.status===0?execute('desktop-bundle',process.execPath,[resolve(root,'spikes/gate-a/node_modules/@tauri-apps/cli/tauri.js'),'build','--debug','--features','desktop','--bundles','app']):{status:null};
const run=bundle.status===0?execute('contract','target/debug/projectos-graph-embedding',['--contract']):{status:null,stdout:''};
if(bundle.status!==0) checks.push({id:'contract',status:'fail',evidence:'Build or test failed; did not execute a potentially stale binary.'});
let receipt=null;try{receipt=JSON.parse(run.stdout?.trim());}catch{}
execute('dynamic-libraries','otool',['-L','target/debug/projectos-graph-embedding']);
const metadata=execute('dependency-inventory','cargo',['+1.97.1','metadata','--locked','--offline','--features','desktop','--filter-platform','aarch64-apple-darwin','--format-version','1']);
let inventory=[];if(metadata.status===0) inventory=JSON.parse(metadata.stdout).packages.map(p=>({name:p.name,version:p.version,license:p.license,source:p.source})).sort((a,b)=>a.name.localeCompare(b.name));
writeFileSync(resolve(out,'dependency-license-inventory.json'),JSON.stringify({status:metadata.status===0?'recorded':'unavailable',scope:'Cargo metadata filtered to aarch64-apple-darwin; informational license inventory, not legal approval',packages:inventory},null,2)+'\n');
const files=['Cargo.toml','Cargo.lock','rust-toolchain.toml','build.rs','src/main.rs','tauri.conf.json','ui/index.html','ui/probe.js','evidence.mjs','README.md'];
const sourceHashes=Object.fromEntries(files.map(f=>[`spikes/gate-b/rust-tauri/${f}`,createHash('sha256').update(readFileSync(resolve(dir,f))).digest('hex')]));
sourceHashes['spikes/gate-a/src-tauri/icons/icon.png']=createHash('sha256').update(readFileSync(resolve(root,'spikes/gate-a/src-tauri/icons/icon.png'))).digest('hex');
const report={gate:'B',status:checks.every(c=>c.status==='pass')&&receipt?.status==='pass'?'partial_unverified':'blocked',generatedAt:new Date().toISOString(),upstreamRevision:'c72e64f0c7e480f3d125f071659d5e3ad7b8da15',rustToolchain:'1.97.1',checks,receipt,sourceHashes,licenseInventory:'evidence/gates/gate-b/dependency-license-inventory.json',binarySha256:run.status===0?createHash('sha256').update(readFileSync(resolve(dir,'target/debug/projectos-graph-embedding'))).digest('hex'):null,limitations:['Published helix-db 3.0.0 crate has no embedded feature; this pins Git source with embedded enabled. Rust 1.90 fails an upstream parent-module import; the pinned upstream toolchain 1.97.1 succeeds.','Six Rust operations on a tiny corpus are not the entire shared engine gate. Python adapter has separately rerun eight contract groups for each candidate.','Clean universal/macOS/Linux installation, signing, dependency license review, full Rust adapter/export/recovery/concurrency and production-scale memory remain unverified.','The SDK has network-capable dependencies for other modes; this probe only opens local Disk sources. Production central egress enforcement remains required.']};
writeFileSync(resolve(out,'rust-integration.json'),JSON.stringify(report,null,2)+'\n');
const primary=JSON.parse(readFileSync(resolve(out,'report.json'),'utf8'));
primary.updatedAt=report.generatedAt;
primary.rustIntegration='evidence/gates/gate-b/rust-integration.json';
primary.sourceHashes={...sourceHashes,...Object.fromEntries(['requirements.lock','fixtures/corpus.json','src/run_gate_b.py','src/common.py','src/ladybug_adapter.py','src/helix_adapter.py'].map(f=>[`spikes/gate-b/${f}`,createHash('sha256').update(readFileSync(resolve(dir,'..',f))).digest('hex')]))};
primary.rustIntegrationStatus=report.status;
primary.checks=checks;
primary.as_of=report.generatedAt.slice(0,10);
primary.roadmap_recommendation.note='Both Python candidate contracts pass. A pinned Helix Rust/Tauri macOS arm64 debug app builds and passes six local operations; full adapter, license closure, universal/Linux packages and representative-scale memory remain open.';
primary.selection_recommendation.rationale[3]='Helix remains provisional: Python SDK/runtime licensing discrepancies, missing upstream crate license metadata and an AGPL-3.0 metrics dependency require review; the narrow Rust/Tauri host probe does not establish release readiness.';
primary.candidates.helix.criteria.rust_adapter_and_sdk_runtime_pin={status:'untested',note:'Partial host proof: pinned Git source compiles with Rust1.97.1 and six Rust operations pass. Full shared Rust contract remains untested.',evidence:['rust-integration.json']};
primary.limitations[0]='Pinned Helix Git source with embedded feature now builds in a native Tauri macOS arm64 debug app; six Rust operations pass. Published crate lacks embedded feature. Full production Rust adapter remains unverified.';
writeFileSync(resolve(out,'report.json'),JSON.stringify(primary,null,2)+'\n');
console.log(JSON.stringify({status:report.status,checks,receipt,packages:inventory.length}));
process.exitCode=report.status==='blocked'?1:0;
