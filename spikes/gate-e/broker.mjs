import { createHash, randomUUID } from 'node:crypto';

const hash = value => createHash('sha256').update(JSON.stringify(value)).digest('hex');
const allowedError = code => Object.assign(new Error(code), { code });
export class CredentialStore {
  #entries = new Map();
  add(secret, endpoint) {
    const url = validateEndpoint(endpoint);
    const handle = randomUUID();
    this.#entries.set(handle, { secret, endpoint: url });
    return handle;
  }
  resolve(handle, endpoint) {
    const entry = this.#entries.get(handle);
    if (!entry || entry.endpoint !== endpoint) throw allowedError('CREDENTIAL_SCOPE_DENIED');
    return entry.secret;
  }
  revoke(handle) { this.#entries.delete(handle); }
  redact(value) {
    return this.redactionScope().redact(value);
  }
  redactionScope() {
    const secrets = [...this.#entries.values()].map(x => x.secret).filter(Boolean);
    return {
      redact(value) {
        let text = String(value);
        for (const secret of secrets) text = text.split(secret).join('[REDACTED]');
        return text;
      },
      streamRedactor() {
        let pending = '';
        return (chunk = '', final = false) => {
          pending += String(chunk);
          for (const secret of secrets) pending = pending.split(secret).join('[REDACTED]');
          // Retain only a suffix that might become a secret in the next chunk.
          let keep = 0;
          if (!final) for (const secret of secrets) for (let n = 1; n < secret.length && n <= pending.length; n++) {
            if (pending.endsWith(secret.slice(0, n))) keep = Math.max(keep, n);
          }
          const output = pending.slice(0, pending.length - keep);
          pending = pending.slice(pending.length - keep);
          return output;
        };
      }
    };
  }
  streamRedactor() { return this.redactionScope().streamRedactor(); }
  toJSON() { return { type: 'opaque-credential-store' }; }
}
function validateEndpoint(endpoint) {
  const u = new URL(endpoint);
  if (u.protocol !== 'https:' || u.username || u.password || u.search || u.hash || (u.port && u.port !== '443')) throw allowedError('ENDPOINT_DENIED');
  return u.href;
}
function publicReceipt(r) { return structuredClone(r); }
export class ProviderBroker {
  #credentials; #transport; #endpoints; #online = true; #drafts = new Map(); #receipts = [];
  #spent = 0; #tokens = 0; #unknown = false; #busy = false;
  #spendCeiling; #tokenCeiling; #active = null;
  constructor({ credentials, transport, endpoints, spendCeiling, tokenCeiling }) {
    if (!Number.isFinite(spendCeiling) || spendCeiling < 0 || !Number.isSafeInteger(tokenCeiling) || tokenCeiling < 0) throw allowedError('INVALID_BUDGET');
    this.#credentials = credentials; this.#transport = transport;
    this.#endpoints = new Set(endpoints.map(validateEndpoint));
    this.#spendCeiling = spendCeiling; this.#tokenCeiling = tokenCeiling;
  }
  setOnline(value) { this.#online = Boolean(value); if (!this.#online) this.#active?.abort('OFFLINE'); } // No draft dispatch here.
  receipts() { return this.#receipts.map(publicReceipt); }
  draft(request) {
    const id = randomUUID(), inputHash = hash(request.input);
    this.#drafts.set(id, structuredClone(request));
    return { id, inputHash, reviewHash: hash(request), route: request.route.map(x => ({ endpoint: x.endpoint, model: x.model })), maxTokens: request.maxTokens, estimates: request.route.map(x => x.estimate) };
  }
  async executeDraft(id, { inputHash, reviewHash, approved, signal, onChunk }) {
    const request = this.#drafts.get(id);
    if (!approved || !request || hash(request.input) !== inputHash || hash(request) !== reviewHash) throw allowedError('DRAFT_REVIEW_REQUIRED');
    const result = await this.run(request, { signal, onChunk });
    this.#drafts.delete(id); return result;
  }
  async run(request, { signal, onChunk = () => {} } = {}) {
    request = structuredClone(request);
    if (!this.#online) throw allowedError('OFFLINE');
    if (this.#busy) throw allowedError('CONCURRENT_CALL_DENIED');
    if (this.#unknown) throw allowedError('OUTCOME_RECONCILIATION_REQUIRED');
    if (!Number.isSafeInteger(request.maxTokens) || request.maxTokens <= 0 || !Number.isSafeInteger(request.inputTokenEstimate) || request.inputTokenEstimate < 0 || !Number.isFinite(request.timeoutMs) || request.timeoutMs <= 0 || !Array.isArray(request.route) || !request.route.length) throw allowedError('INVALID_REQUEST');
    if (request.approvedInputHash !== hash(request.input)) throw allowedError('EGRESS_SCOPE_DENIED');
    // Validate the whole fixed route before sending any payload or credential.
    const route = request.route.map(item => {
      const endpoint = validateEndpoint(item.endpoint);
      if (!this.#endpoints.has(endpoint) || typeof item.model !== 'string' || !item.model || (!request.allowAuto && /(^|\/)auto$|router/i.test(item.model))) throw allowedError('ROUTE_DENIED');
      if (!Number.isFinite(item.estimate) || item.estimate < 0) throw allowedError('ESTIMATE_REQUIRED');
      return { ...item, endpoint };
    });
    this.#busy = true;
    const receipt = { runId: randomUUID(), inputHash: hash(request.input), status: 'running', attempts: [] };
    this.#receipts.push(receipt);
    try {
      for (let i = 0; i < route.length; i++) {
        if (!this.#online) throw allowedError('OFFLINE');
        if (signal?.aborted) throw allowedError('CANCELLED');
        const item = route[i];
        if (this.#spent + item.estimate > this.#spendCeiling || this.#tokens + request.maxTokens + request.inputTokenEstimate > this.#tokenCeiling) throw allowedError('CEILING_REACHED');
        const controller = new AbortController();
        this.#active = controller;
        const abort = () => controller.abort('CANCELLED');
        signal?.addEventListener('abort', abort, { once: true });
        let timer;
        const stop = new Promise((_, reject) => {
          controller.signal.addEventListener('abort', () => reject(allowedError(['TIMEOUT','OFFLINE'].includes(controller.signal.reason) ? controller.signal.reason : 'CANCELLED')), { once: true });
          timer = setTimeout(() => controller.abort('TIMEOUT'), request.timeoutMs);
        });
        stop.catch(() => {}); // A credential denial may occur before a race subscribes.
        const attempt = { endpoint: item.endpoint, requestedModel: item.model, resolvedModel: null, fallbackIndex: i,
          estimate: item.estimate, providerUsage: null, billedAmount: null, costState: 'unknown', status: 'pending' };
        receipt.attempts.push(attempt);
        let sent = false;
        try {
          const secret = this.#credentials.resolve(item.handle, item.endpoint);
          const redaction = this.#credentials.redactionScope(); // Keep until this attempt completes, even after logout/revoke.
          sent = true;
          const response = await Promise.race([this.#transport.open({ endpoint: item.endpoint, authorization: secret,
            model: item.model, input: request.input, maxTokens: request.maxTokens, redirect: 'error', signal: controller.signal }), stop]);
          if (controller.signal.aborted) throw allowedError('CANCELLED');
          if (response.redirect || response.endpoint !== item.endpoint) throw allowedError('REDIRECT_DENIED');
          if (response.preflightRejected === true) {
            attempt.status = 'rejected-before-inference'; attempt.costState = 'known'; attempt.billedAmount = 0;
            if (i + 1 < route.length) continue;
            throw allowedError('ROUTE_EXHAUSTED');
          }
          if (typeof response.resolvedModel !== 'string' || !response.resolvedModel) throw allowedError('RESOLVED_MODEL_REQUIRED');
          attempt.resolvedModel = redaction.redact(response.resolvedModel);
          const iterator = response.stream[Symbol.asyncIterator]();
          const redactChunk = redaction.streamRedactor();
          while (true) {
            const part = await Promise.race([iterator.next(), stop]);
            if (controller.signal.aborted) throw allowedError('CANCELLED');
            if (part.done) { const tail = redactChunk('', true); if (tail) onChunk(tail); break; }
            const safeChunk = redactChunk(part.value); if (safeChunk) onChunk(safeChunk);
          }
          const usage = response.usage;
          if (usage && Number.isSafeInteger(usage.inputTokens) && usage.inputTokens >= 0 && Number.isSafeInteger(usage.outputTokens) && usage.outputTokens >= 0) {
            attempt.providerUsage = { inputTokens: usage.inputTokens, outputTokens: usage.outputTokens };
            this.#tokens += usage.inputTokens + usage.outputTokens;
          } else this.#unknown = true;
          if (Number.isFinite(response.billedAmount) && response.billedAmount >= 0) {
            attempt.billedAmount = response.billedAmount; attempt.costState = 'billed'; this.#spent += response.billedAmount;
          } else { this.#unknown = true; this.#spent += item.estimate; }
          attempt.status = 'completed'; receipt.status = 'completed';
          if (this.#spent > this.#spendCeiling || this.#tokens > this.#tokenCeiling || (attempt.providerUsage && attempt.providerUsage.outputTokens > request.maxTokens)) {
            this.#unknown = true; attempt.status = 'provider-ceiling-violation'; receipt.status = 'provider-ceiling-violation';
          }
          return publicReceipt(receipt);
        } catch (error) {
          const code = ['CANCELLED','TIMEOUT','OFFLINE','REDIRECT_DENIED','RESOLVED_MODEL_REQUIRED','ROUTE_EXHAUSTED','CREDENTIAL_SCOPE_DENIED'].includes(error.code) ? error.code : 'PROVIDER_ERROR';
          attempt.status = code;
          if (sent && attempt.costState === 'unknown') this.#unknown = true;
          receipt.status = this.#unknown ? 'OUTCOME_UNKNOWN' : code;
          throw allowedError(code); // Provider error text is deliberately never serialized.
        } finally { clearTimeout(timer); signal?.removeEventListener('abort', abort); controller.abort('CANCELLED'); this.#active = null; }
      }
    } catch (error) {
      if (receipt.status === 'running') receipt.status = error.code || 'BROKER_ERROR';
      throw error;
    } finally { this.#busy = false; }
  }
}
