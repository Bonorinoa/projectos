import {spawnSync} from 'node:child_process';
import {mkdir, readFile, writeFile,copyFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {dirname, join, resolve} from 'node:path';
const here=dirname(fileURLToPath(import.meta.url)), root=resolve(here,'../..');
const bundle=join(here,'build/ProjectOSSandboxProbe.app');
const binary=join(bundle,'Contents/MacOS/app-sandbox-probe');
const source=join(here,'app-sandbox-probe.c'), entitlements=join(here,'app-sandbox.entitlements.plist');
const run=(command,args)=>{
 const r=spawnSync(command,args,{encoding:'utf8',timeout:15000});
 return {command:[command,...args],status:r.status,signal:r.signal,stdout:r.stdout,stderr:r.stderr,error:r.error?.message};
};
await mkdir(dirname(binary),{recursive:true});
await copyFile(join(here,'app-sandbox.Info.plist'),join(bundle,'Contents/Info.plist'));
const compile=run('/usr/bin/cc',['-std=c11','-Wall','-Wextra','-Werror','-O2',source,'-o',binary]);
if(compile.status)throw Error(compile.stderr);
// Only this generated fixture bundle is stripped of Finder metadata before signing.
const generatedMetadataCleanup=run('/usr/bin/xattr',['-cr',bundle]);
if(generatedMetadataCleanup.status)throw Error(generatedMetadataCleanup.stderr);
const signing=run('/usr/bin/codesign',['--force','--sign','-','--entitlements',entitlements,bundle]);
if(signing.status)throw Error(signing.stderr);
const signature=run('/usr/bin/codesign',['--display','--entitlements','-',binary]);
const execution=run(binary,[join(here,'fixtures/ambient-secret.txt')]);
const controlBinary=join(here,'build/app-sandbox-control');
const controlCompile=run('/usr/bin/cc',['-std=c11','-Wall','-Wextra','-Werror','-O2',source,'-o',controlBinary]);
const controlExecution=controlCompile.status===0?run(controlBinary,[join(here,'fixtures/ambient-secret.txt')]):null;
let result;try{result=JSON.parse(execution.stdout)}catch{result=null}
const report={gate:'C',generatedAt:new Date().toISOString(),kind:'supported-app-sandbox-native-probe',compile,generatedMetadataCleanup,signing,signature,execution,result,controlCompile,controlExecution,
 sourceHashes:Object.fromEntries(await Promise.all([source,entitlements,join(here,'app-sandbox.Info.plist'),fileURLToPath(import.meta.url)].map(async path=>[path.slice(root.length+1),createHash('sha256').update(await readFile(path)).digest('hex')]))),
 decision:result && [1,13].includes(result.ambientFileErrno)&&[1,13].includes(result.localhostErrno)?'filesystem-and-network-subset-proven':'unproven',
 limitations:['Native helper only: no TS runtime or broker IPC integration.','Ad hoc signature only: universal/notarization/clean-machine distribution untested.','DNS failure alone does not prove policy denial.','A successful fork/child exit 42 means locked child-process boundary FAILS.','OS memory ceiling remains unproven; this probe does not weaken it.']};
await writeFile(join(root,'evidence/gates/gate-c/app-sandbox-report.json'),JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({decision:report.decision,executionStatus:execution.status,result},null,2));
