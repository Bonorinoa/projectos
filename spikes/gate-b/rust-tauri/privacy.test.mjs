import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { spawn } from 'node:child_process';
import { mkdtempSync, existsSync, readFileSync, rmSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
const dir=dirname(fileURLToPath(import.meta.url));
test('all embedded opens explicitly require disabled telemetry configuration', () => {
  const source=readFileSync(join(dir,'src/main.rs'),'utf8');
  assert.ok(!/Client::open\(|Client::open_reader\(/.test(source));
  assert.match(source,/DbConfig::new\(\)\.with_query_telemetry\(QueryTelemetry::Disabled\)/);
  assert.equal((source.match(/Client::open_with_config\(source\.clone\(\), local_only_config\(\)\)/g)||[]).length,2);
  assert.match(source,/Client::open_reader_with_config\(source, local_only_config\(\)\)/);
});
test('native contract ignores enabled telemetry environment and creates no profile or HTTP request', async () => {
  const temporary=mkdtempSync('/private/tmp/projectos-graph-privacy-'), profile=join(temporary,'isolated-helix-profile');
  let received=0;
  const server=createServer((req,res)=>{received++;req.resume();res.writeHead(200);res.end('{}');});
  let child;
  try {
    await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(0,'127.0.0.1',resolve);});
    const binary=join(dir,'target/debug/projectos-graph-embedding');
    assert.ok(existsSync(binary),'build native desktop binary before this regression');
    // Only this subprocess gets fixture settings. Parent environment and user
    // home are not changed. No external destination or real credentials exist.
    const env={PATH:process.env.PATH,TMPDIR:temporary,HELIX_HOME:profile,HELIX_TELEMETRY_LEVEL:'basic',
      HELIX_TELEMETRY_INSTALLATION_ID:'019a1111-1111-7111-8111-111111111111',HELIX_TELEMETRY_ENDPOINT:`http://127.0.0.1:${server.address().port}/v1/events`};
    const result=await new Promise((resolve,reject)=>{
      child=spawn(binary,['--contract'],{env,stdio:['ignore','pipe','pipe']});let stdout='',stderr='';
      const deadline=setTimeout(()=>{child.kill();reject(Error('contract timed out'));},30000);
      child.stdout.on('data',data=>stdout+=data);child.stderr.on('data',data=>stderr+=data);
      child.on('error',error=>{clearTimeout(deadline);reject(error);});
      child.on('close',code=>{clearTimeout(deadline);resolve({code,stdout,stderr});});
    });
    assert.equal(result.code,0,result.stderr);const receipt=JSON.parse(result.stdout.trim());
    assert.equal(receipt.queryTelemetry,'explicitly-disabled');assert.equal(receipt.checks,6);
    assert.equal(existsSync(profile),false,'upstream telemetry settings directory must not be created');
    assert.equal(received,0,'no telemetry event must reach synthetic loopback receiver');
  } finally {child?.kill();server.close();rmSync(temporary,{recursive:true,force:true});}
});
