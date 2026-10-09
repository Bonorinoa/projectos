import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { CredentialStore, ProviderBroker } from '../broker.mjs';
import { beginOAuth, PassphraseVault } from '../auth.mjs';

const endpoint = 'https://api.mock.example/v1/chat';
const secret = 'SYNTHETIC-SECRET-DO-NOT-LEAK';
const input = 'PRIVATE-SYNTHETIC-PROMPT';
const inputHash = createHash('sha256').update(JSON.stringify(input)).digest('hex');
const complete = (overrides = {}) => ({ endpoint, resolvedModel: 'reported-model', stream: (async function* () { yield 'one'; yield 'two'; })(), usage: { inputTokens: 2, outputTokens: 3 }, billedAmount: 0.02, ...overrides });
function fixture(responses = [complete()], options = {}) {
  const credentials = new CredentialStore(), calls = [], handle = credentials.add(secret, endpoint);
  const transport = { async open(call) { calls.push(call); const response = responses.shift(); if (response instanceof Error) throw response; return typeof response === 'function' ? response(call) : response; } };
  const broker = new ProviderBroker({ credentials, transport, endpoints: [endpoint], spendCeiling: 1, tokenCeiling: 100, ...options });
  const request = { input, approvedInputHash: inputHash, route: [{ endpoint, model: 'fixed-model', handle, estimate: 0.05 }], maxTokens: 10, inputTokenEstimate: 4, timeoutMs: 100 };
  return { broker, credentials, calls, request, handle };
}
test('streaming and requested/resolved model receipt, credentials stay in transport', async () => {
  const f = fixture(), chunks = [];
  const receipt = await f.broker.run(f.request, { onChunk: x => chunks.push(x) });
  assert.deepEqual(chunks, ['one','two']); assert.equal(f.calls[0].authorization, secret);
  assert.equal(f.calls[0].redirect, 'error'); assert.equal(f.calls[0].maxTokens, 10);
  assert.equal(receipt.attempts[0].requestedModel, 'fixed-model'); assert.equal(receipt.attempts[0].resolvedModel, 'reported-model');
  const publicData = JSON.stringify([receipt, f.credentials, f.broker]);
  for (const sensitive of [secret, input, 'one','two', f.handle]) assert.ok(!publicData.includes(sensitive));
  receipt.attempts[0].status = 'altered'; assert.equal(f.broker.receipts()[0].attempts[0].status, 'completed');
});
test('fixed ordered fallback is visible and no hidden routing', async () => {
  const f = fixture([{ endpoint, preflightRejected: true }, complete()]);
  f.request.route.push({ ...f.request.route[0], model: 'fixed-backup' });
  const receipt = await f.broker.run(f.request);
  assert.deepEqual(f.calls.map(x => x.model), ['fixed-model', 'fixed-backup']);
  assert.equal(receipt.attempts[0].status, 'rejected-before-inference'); assert.equal(receipt.attempts[1].fallbackIndex, 1);
});
test('invalid route is rejected before any credential or payload is sent', async () => {
  for (const destination of ['http://api.mock.example/v1/chat','https://evil.example/v1/chat','https://user:pass@api.mock.example/v1/chat','https://api.mock.example/v1/chat?token=x','https://api.mock.example:8443/v1/chat']) {
    const f = fixture(); f.request.route.push({ ...f.request.route[0], endpoint: destination });
    await assert.rejects(f.broker.run(f.request)); assert.equal(f.calls.length, 0);
  }
});
test('credential cannot be reused after endpoint change or logout', async () => {
  const f = fixture(); assert.throws(() => f.credentials.resolve(f.handle, 'https://evil.example/'));
  f.credentials.revoke(f.handle); await assert.rejects(f.broker.run(f.request), /CREDENTIAL_SCOPE_DENIED/); assert.equal(f.calls.length, 0);
});
test('redirect response refuses stream and leaves truthful unknown outcome', async () => {
  const f = fixture([complete({ redirect: 'https://evil.example/' })]); let chunks = 0;
  await assert.rejects(f.broker.run(f.request, { onChunk() { chunks++; } }), /REDIRECT_DENIED/);
  assert.equal(chunks, 0); assert.equal(f.calls.length, 1); assert.equal(f.broker.receipts()[0].status, 'OUTCOME_UNKNOWN');
  await assert.rejects(f.broker.run(f.request), /RECONCILIATION/);
});
test('changed response endpoint rejected', async () => {
  const f = fixture([complete({ endpoint: 'https://evil.example/' })]);
  await assert.rejects(f.broker.run(f.request), /REDIRECT_DENIED/);
});
test('auto routing requires opt-in and still records resolved model', async () => {
  const f = fixture(); f.request.route[0].model = 'openrouter/auto';
  await assert.rejects(f.broker.run(f.request), /ROUTE_DENIED/); assert.equal(f.calls.length, 0);
  f.request.allowAuto = true; assert.equal((await f.broker.run(f.request)).attempts[0].resolvedModel, 'reported-model');
});
test('provider error containing secrets is normalized and never retried automatically', async () => {
  const f = fixture([new Error(`${secret} ${input}`), complete()]); f.request.route.push({ ...f.request.route[0], model: 'backup' });
  await assert.rejects(f.broker.run(f.request), error => error.message === 'PROVIDER_ERROR');
  assert.equal(f.calls.length, 1); assert.ok(!JSON.stringify(f.broker.receipts()).includes(secret));
});
test('timeout aborts transport and forbids ambiguous retry', async () => {
  const f = fixture([() => new Promise(() => {})]); f.request.timeoutMs = 10;
  await assert.rejects(f.broker.run(f.request), /TIMEOUT/); assert.equal(f.calls[0].signal.aborted, true);
  await assert.rejects(f.broker.run(f.request), /RECONCILIATION/);
});
test('midstream cancellation stops events and aborts transport', async () => {
  const f = fixture(), controller = new AbortController(), chunks = [];
  await assert.rejects(f.broker.run(f.request, { signal: controller.signal, onChunk(chunk) { chunks.push(chunk); controller.abort(); } }), /CANCELLED/);
  assert.deepEqual(chunks, ['one']); assert.equal(f.calls[0].signal.aborted, true);
});
test('pre-cancelled request does not call transport', async () => {
  const f = fixture(), controller = new AbortController(); controller.abort();
  await assert.rejects(f.broker.run(f.request, { signal: controller.signal }), /CANCELLED/); assert.equal(f.calls.length, 0);
});
test('missing resolved model fails instead of pretending requested model served', async () => {
  const f = fixture([complete({ resolvedModel: undefined })]); await assert.rejects(f.broker.run(f.request), /RESOLVED_MODEL_REQUIRED/);
});
test('cost estimate, usage, billed amount stay separate', async () => {
  const f = fixture(); const a = (await f.broker.run(f.request)).attempts[0];
  assert.equal(a.estimate, .05); assert.equal(a.billedAmount, .02); assert.equal(a.costState, 'billed'); assert.deepEqual(a.providerUsage, { inputTokens: 2, outputTokens: 3 });
});
test('unknown bill and unknown usage prevent additional spending', async () => {
  for (const overrides of [{ billedAmount: undefined }, { usage: undefined }, { billedAmount: NaN }, { usage: { inputTokens: -2, outputTokens: 2 } }]) {
    const f = fixture([complete(overrides)]); await f.broker.run(f.request);
    await assert.rejects(f.broker.run(f.request), /RECONCILIATION/); assert.equal(f.calls.length, 1);
  }
});
test('spend and total token ceilings block calls before send', async () => {
  for (const options of [{ spendCeiling: .01 }, { tokenCeiling: 13 }]) {
    const f = fixture([complete()], options); await assert.rejects(f.broker.run(f.request), /CEILING_REACHED/); assert.equal(f.calls.length, 0);
  }
  const f = fixture([complete(), complete()], { tokenCeiling: 18 }); await f.broker.run(f.request);
  await assert.rejects(f.broker.run(f.request), /CEILING_REACHED/); assert.equal(f.calls.length, 1);
});
test('provider ceiling violation is visible and blocks all further calls', async () => {
  const f = fixture([complete({ billedAmount: 5 })]); assert.equal((await f.broker.run(f.request)).status, 'provider-ceiling-violation');
  await assert.rejects(f.broker.run(f.request), /RECONCILIATION/);
});
test('concurrent requests cannot race the cost budget', async () => {
  let release; const f = fixture([() => new Promise(resolve => { release = resolve; })]);
  const active = f.broker.run(f.request); await assert.rejects(f.broker.run(f.request), /CONCURRENT/);
  release(complete()); await active; assert.equal(f.calls.length, 1);
});
test('offline draft survives reconnect and requires exact hash review', async () => {
  const f = fixture(); f.broker.setOnline(false); const draft = f.broker.draft(f.request);
  await assert.rejects(f.broker.run(f.request), /OFFLINE/); f.broker.setOnline(true); assert.equal(f.calls.length, 0);
  await assert.rejects(f.broker.executeDraft(draft.id, { approved: false, inputHash: draft.inputHash }), /REVIEW/);
  await assert.rejects(f.broker.executeDraft(draft.id, { approved: true, inputHash: 'stale' }), /REVIEW/);
  await assert.rejects(f.broker.executeDraft(draft.id, { approved: true, inputHash: draft.inputHash, reviewHash: 'stale-route' }), /REVIEW/);
  await f.broker.executeDraft(draft.id, { approved: true, inputHash: draft.inputHash, reviewHash: draft.reviewHash }); assert.equal(f.calls.length, 1);
  await assert.rejects(f.broker.executeDraft(draft.id, { approved: true, inputHash: draft.inputHash }), /REVIEW/);
});
test('input change outside exact egress approval is denied', async () => {
  const f = fixture(); f.request.input += ' changed'; await assert.rejects(f.broker.run(f.request), /EGRESS_SCOPE_DENIED/); assert.equal(f.calls.length, 0);
});
test('credential echoed by provider is redacted even across chunk boundaries', async () => {
  const f = fixture([complete({ resolvedModel: secret, stream: (async function* () { yield secret.slice(0, 8); yield secret.slice(8); yield 'safe'; })() })]), chunks = [];
  const receipt = await f.broker.run(f.request, { onChunk: x => chunks.push(x) });
  assert.equal(chunks.join(''), '[REDACTED]safe'); assert.equal(receipt.attempts[0].resolvedModel, '[REDACTED]');
});
test('stream redaction handles self-overlapping secret prefixes', () => {
  const store = new CredentialStore(); store.add('aaaaaaaaaaaa', endpoint); const redact = store.streamRedactor();
  const output = redact('aaaaaa') + redact('aaaaaaaa') + redact('', true);
  assert.ok(!output.includes('aaaaaaaaaaaa')); assert.ok(output.includes('[REDACTED]'));
});
test('revocation during transport open retains request-lifetime secret redaction', async () => {
  let finish; const f = fixture([() => new Promise(resolve => { finish = resolve; })]), chunks = [];
  const active = f.broker.run(f.request, { onChunk: value => chunks.push(value) });
  f.credentials.revoke(f.handle);
  finish(complete({ resolvedModel: secret, stream: (async function* () { yield secret.slice(0, 7); yield secret.slice(7); })() }));
  const receipt = await active;
  assert.equal(receipt.attempts[0].resolvedModel, '[REDACTED]'); assert.equal(chunks.join(''), '[REDACTED]');
  await assert.rejects(f.broker.run(f.request), /CREDENTIAL_SCOPE_DENIED/);
});
test('switching offline during a stream cancels the active call', async () => {
  const f = fixture(), chunks = [];
  await assert.rejects(f.broker.run(f.request, { onChunk: x => { chunks.push(x); f.broker.setOnline(false); } }));
  assert.deepEqual(chunks, ['one']); assert.equal(f.calls[0].signal.aborted, true);
});
test('request snapshot cannot be changed while transport waits', async () => {
  let release; const f = fixture([() => new Promise(resolve => { release = resolve; })]);
  const active = f.broker.run(f.request); f.request.input = 'unapproved'; f.request.maxTokens = 999;
  release(complete()); await active; assert.equal(f.calls[0].input, input); assert.equal(f.calls[0].maxTokens, 10);
});
test('OAuth PKCE S256 uses random state, ephemeral loopback, exact callback and one shot', async () => {
  const exchanges = [], revoked = [];
  const auth = await beginOAuth({ exchange: async data => { exchanges.push(data); return 'synthetic-token'; }, revoke: async token => revoked.push(token) });
  const second = await beginOAuth({ exchange: async () => {}, revoke: async () => {} });
  try {
    assert.notEqual(auth.state, second.state); assert.notEqual(auth.redirect, second.redirect); assert.equal(auth.method, 'S256');
    assert.equal((await fetch(`${auth.redirect}?state=bad&code=code`)).status, 400);
    assert.equal((await fetch(`${auth.redirect.replace('/oauth/callback','/wrong')}?state=${auth.state}&code=code`)).status, 400);
    assert.equal((await fetch(`${auth.redirect}?state=${auth.state}&state=${auth.state}&code=code`)).status, 400);
    assert.equal(exchanges.length, 0);
    assert.equal((await fetch(`${auth.redirect}?state=${auth.state}&code=code`)).status, 200);
    assert.equal(exchanges.length, 1); assert.equal(exchanges[0].redirect, auth.redirect);
    assert.equal(createHash('sha256').update(exchanges[0].verifier).digest('base64url'), auth.challenge);
    await assert.rejects(fetch(`${auth.redirect}?state=${auth.state}&code=again`));
    await auth.logout(); await auth.logout(); assert.deepEqual(revoked, ['synthetic-token']);
  } finally { auth.close(); second.close(); }
});
test('passphrase vault encrypts, locks, recovers encrypted backup and rejects wrong passphrase', () => {
  const passphrase = 'a sufficiently long synthetic passphrase'; const v = PassphraseVault.create(passphrase);
  v.put('handle', secret); const blob = v.exportEncrypted(), secondBlob = v.exportEncrypted();
  assert.notEqual(blob, secondBlob); assert.ok(!blob.includes(secret)); assert.equal(PassphraseVault.kdf.N, 32768);
  v.lock(); assert.throws(() => v.get('handle'), /LOCKED/); assert.throws(() => v.unlock(blob, 'wrong'), /UNLOCK_FAILED/);
  assert.throws(() => v.get('handle'), /LOCKED/); v.unlock(blob, passphrase); assert.equal(v.get('handle'), secret);
  v.lock(); v.unlock(secondBlob, passphrase); assert.equal(v.get('handle'), secret); v.lock();
});
test('logout during OAuth exchange revokes late-arriving credential', async () => {
  let finish, started; const ready = new Promise(resolve => { started = resolve; }); const revoked = [];
  const auth = await beginOAuth({ exchange: () => new Promise(resolve => { finish = resolve; started(); }), revoke: async value => revoked.push(value) });
  try {
    const callback = fetch(`${auth.redirect}?state=${auth.state}&code=synthetic`); await ready;
    await auth.logout(); finish('late-synthetic-token'); assert.equal((await callback).status, 400);
    assert.deepEqual(revoked, ['late-synthetic-token']);
  } finally { auth.close(); }
});
test('transient OAuth revocation failure remains retryable and success is one shot', async () => {
  let attempts = 0;
  const auth = await beginOAuth({ exchange: async () => 'synthetic-token', revoke: async () => { if (++attempts === 1) throw Error('transient'); } });
  try {
    assert.equal((await fetch(`${auth.redirect}?state=${auth.state}&code=synthetic`)).status, 200);
    await assert.rejects(auth.logout(), /transient/); await auth.logout(); await auth.logout(); assert.equal(attempts, 2);
  } finally { auth.close(); }
});
test('vault capacity rejects oversized writes before mutation and accepted backup restores', () => {
  const passphrase = 'a sufficiently long synthetic passphrase', v = PassphraseVault.create(passphrase);
  v.put('h', 'existing'); assert.throws(() => v.put('h', 'x'.repeat(3 * 1024 * 1024)), /VAULT_CAPACITY/);
  assert.equal(v.get('h'), 'existing');
  const accepted = 'x'.repeat(PassphraseVault.maxPlaintextBytes - 32); v.put('near-limit', accepted);
  const blob = v.exportEncrypted(); assert.ok(blob.length < 4 * 1024 * 1024); v.lock(); v.unlock(blob, passphrase);
  assert.equal(v.get('near-limit'), accepted); assert.equal(v.get('h'), 'existing'); v.lock();
});
test('vault handles cannot collide with inherited object properties', () => {
  const v = PassphraseVault.create('a sufficiently long synthetic passphrase');
  assert.equal(v.get('constructor'), undefined); v.put('__proto__', secret); const blob = v.exportEncrypted();
  v.lock(); v.unlock(blob, 'a sufficiently long synthetic passphrase'); assert.equal(v.get('__proto__'), secret); v.lock();
});
test('vault tampering is authenticated and never falls back to plaintext', () => {
  const passphrase = 'a sufficiently long synthetic passphrase', v = PassphraseVault.create(passphrase); v.put('h', secret);
  const original = v.exportEncrypted(); v.lock();
  for (const field of ['data','nonce','tag','salt']) {
    const envelope = JSON.parse(original), bytes = Buffer.from(envelope[field], 'base64'); bytes[0] ^= 1; envelope[field] = bytes.toString('base64');
    assert.throws(() => v.unlock(JSON.stringify(envelope), passphrase), /UNLOCK_FAILED/); assert.throws(() => v.get('h'), /LOCKED/);
  }
  assert.throws(() => v.unlock(JSON.stringify({ h: secret }), passphrase), /UNLOCK_FAILED/);
  assert.throws(() => PassphraseVault.create('short'), /WEAK_PASSPHRASE/);
});
