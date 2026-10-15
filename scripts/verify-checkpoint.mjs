import {spawnSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync,readdirSync} from 'node:fs';
import {resolve,dirname,relative} from 'node:path';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';

const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const output=resolve(root,'evidence/m3');mkdirSync(output,{recursive:true});
const cargo=process.env.PROJECTOS_CARGO||'cargo';
const version=spawnSync(cargo,['--version'],{encoding:'utf8'});
if(version.status!==0||!/^cargo 1\.97\.1\b/.test(version.stdout))throw Error('Use the locked Rust 1.97.1 cargo via PROJECTOS_CARGO/PATH.');
const checks=[];
function run(name,command,args,cwd=root){
  const result=spawnSync(command,args,{cwd,encoding:'utf8',maxBuffer:32*1024*1024});
  const log=(String(result.stdout||'')+String(result.stderr||''))
    .replaceAll(root,'<PROJECTOS>').replaceAll(process.env.HOME||'__NO_HOME__','<USER_HOME>');
  const path=`evidence/m3/${name}.txt`;writeFileSync(resolve(root,path),log);
  checks.push({name,status:result.status===0?'pass':'fail',exitCode:result.status,command:[command,...args].map(x=>x.replaceAll(process.env.HOME||'__NO_HOME__','<USER_HOME>')),cwd:relative(root,cwd)||'.',log:path});
  console.log(`${name}: ${checks.at(-1).status}`);
}
run('kernel',cargo,['test','--locked'],resolve(root,'kernel'));
run('native',cargo,['test','--locked'],resolve(root,'app/src-tauri'));
const uiTests=readdirSync(resolve(root,'app/ui/tests')).filter(x=>x.endsWith('.test.cjs')).map(x=>`app/ui/tests/${x}`);
run('node',process.execPath,['--test',...uiTests,'scripts/test-roadmap.mjs']);
for(const name of readdirSync(resolve(root,'app/ui')).filter(x=>x.endsWith('.js')))run(`syntax-${name}`,process.execPath,['--check',`app/ui/${name}`]);
run('diff-check','git',['diff','--check']);
run('debug-build',cargo,['build','--locked'],resolve(root,'app/src-tauri'));
const sourceHashes={};
function hashTree(dir){for(const e of readdirSync(resolve(root,dir),{withFileTypes:true}).sort((a,b)=>a.name.localeCompare(b.name))){
  if(e.name.startsWith('.')||['target','node_modules','evidence'].includes(e.name))continue;
  const path=`${dir}/${e.name}`;if(e.isDirectory())hashTree(path);else if(/\.(rs|js|cjs|mjs|css|html|toml|lock)$/.test(path))sourceHashes[path]=createHash('sha256').update(readFileSync(resolve(root,path))).digest('hex');
}}
for(const dir of ['kernel','app','scripts'])hashTree(dir);
const report={schemaVersion:1,verifiedAt:new Date().toISOString(),platform:process.platform,architecture:process.arch,cargo:version.stdout.trim(),sourceHashes,checks,passed:checks.every(x=>x.status==='pass'),excluded:['OS keyring conformance requires explicit owner opt in','Live OpenRouter catalog/inference is not part of offline verification','Linux and signed production confinement remain Gate C blockers']};
writeFileSync(resolve(output,'report.json'),JSON.stringify(report,null,2)+'\n');
process.exitCode=report.passed?0:1;
