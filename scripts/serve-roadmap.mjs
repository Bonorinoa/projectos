import {createServer} from 'node:http';
import {readFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import {dirname,resolve} from 'node:path';
import {readdirSync,readFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const ignored=new Set(['node_modules','target','build','dist','work','.venv','__pycache__','.git']);
let signature='';
function refreshEvidence() {
  const hash=createHash('sha256');
  function walk(dir) {
    for(const entry of readdirSync(dir,{withFileTypes:true}).sort((a,b)=>a.name.localeCompare(b.name))) {
      if(entry.name.startsWith('.')||ignored.has(entry.name)) continue;
      const path=resolve(dir,entry.name);
      if(entry.isDirectory()) walk(path);
      else if(entry.isFile()) {hash.update(path);hash.update(readFileSync(path));}
    }
  }
  for(const dir of ['spikes','scripts','evidence']) walk(resolve(root,dir));
  const next=hash.digest('hex');
  if(next===signature) return;
  const result=spawnSync(process.execPath,[resolve(root,'scripts/update-roadmap.mjs')],{encoding:'utf8'});
  if(result.status!==0&&result.status!==1) throw new Error(result.stderr||'Status producer failed');
  signature=next;
}
const routes = new Map([
  ['/','design/projectos-live-mvp-roadmap.html'],
  ['/design/projectos-live-mvp-roadmap.html','design/projectos-live-mvp-roadmap.html'],
  ['/projectos-roadmap-status.json','projectos-roadmap-status.json'],
  ['/projectos-source-snapshot.json','projectos-source-snapshot.json']
]);
createServer(async(req,res)=>{
  const url=new URL(req.url,'http://127.0.0.1');
  const path=routes.get(url.pathname);
  if(req.method!=='GET'||!path){res.writeHead(404);res.end();return;}
  try {refreshEvidence();const body=await readFile(resolve(root,path));res.writeHead(200,{'content-type':path.endsWith('.html')?'text/html; charset=utf-8':'application/json','cache-control':'no-store'});res.end(body);}
  catch {res.writeHead(503);res.end('Roadmap files unavailable');}
}).listen(4317,'127.0.0.1',()=>console.log('Roadmap: http://127.0.0.1:4317/design/projectos-live-mvp-roadmap.html'));
