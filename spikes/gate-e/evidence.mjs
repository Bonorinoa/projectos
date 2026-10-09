import { spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const dir = dirname(fileURLToPath(import.meta.url)), root = resolve(dir, '../..'), out = resolve(root, 'evidence/gates/gate-e');
mkdirSync(out, { recursive: true });
const result = spawnSync(process.execPath, ['--test', 'tests/contract.test.mjs'], { cwd: dir, encoding: 'utf8' });
const raw = `${result.stdout || ''}${result.stderr || ''}`;
writeFileSync(resolve(out, 'tests.tap'), raw);
const sources = ['spikes/gate-e/package.json','spikes/gate-e/broker.mjs','spikes/gate-e/auth.mjs','spikes/gate-e/tests/contract.test.mjs','spikes/gate-e/evidence.mjs','spikes/gate-e/README.md'];
const sourceHashes = Object.fromEntries(sources.map(file => [file, createHash('sha256').update(readFileSync(resolve(root, file))).digest('hex')]));
const value = key => Number(raw.match(new RegExp(`^# ${key} (\\d+)$`, 'm'))?.[1] || 0);
const report = {
  gate: 'E', status: result.status === 0 ? 'conditional' : 'blocked', mock_contract_status: result.status === 0 ? 'pass' : 'fail',
  generatedAt: new Date().toISOString(), environment: { platform: process.platform, arch: process.arch, node: process.version, openssl: process.versions.openssl },
  command: 'cd spikes/gate-e && node evidence.mjs', exitCode: result.status,
  tests: { total: value('tests'), pass: value('pass'), fail: value('fail'), skipped: value('skipped') },
  evidence: ['evidence/gates/gate-e/tests.tap'], sourceHashes,
  limitations: [
    'This is an executable contract spike in trusted Node, not the native Rust credential/egress boundary or worker confinement.',
    'Transport is injected mock-only: real HTTP redirect prevention, DNS/IPv4/IPv6 confinement, OpenRouter OAuth exchange and live-provider streaming/cancel/billing are untested.',
    'No UI/worker process is connected. Credential isolation checks interface and serialization behavior; OS keyring integration is untested.',
    'Passphrase vault primitives execute on macOS via Node/OpenSSL. Linux Secret Service absence, native vault migration, crash-safe file persistence, passphrase recovery UX, secure memory zeroization and native integration remain untested.',
    'Cost ceilings prevent further calls based on supplied estimates/reported usage; remote providers may exceed estimates or ignore max_tokens. Unknown outcomes stop further calls; reconciliation workflow not yet implemented.',
    'Mock-confirmed pre-inference rejection is the only fallback trigger. A production adapter must establish documented safe rejection semantics before enabling fallback.',
    'OAuth localhost test needs sandbox escalation; manual acceptance and Linux parity are outstanding.'
  ]
};
writeFileSync(resolve(out, 'report.json'), JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify({ status: report.status, tests: report.tests, report: resolve(out, 'report.json') }));
process.exitCode = result.status ?? 1;
