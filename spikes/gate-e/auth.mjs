import { randomBytes, createHash, timingSafeEqual, scryptSync, createCipheriv, createDecipheriv } from 'node:crypto';
import { createServer } from 'node:http';
const b64 = b => b.toString('base64url');
const fail = code => Object.assign(new Error(code), { code });
export async function beginOAuth({ exchange, revoke }) {
  const state = b64(randomBytes(32)), verifier = b64(randomBytes(32));
  const challenge = b64(createHash('sha256').update(verifier).digest());
  let consumed = false, token = null, redirect, loggedOut = false, revocation = null;
  async function revokePending() {
    if (!token) return;
    if (!revocation) revocation = (async () => {
      const previous = token;
      await revoke(previous); // Keep pending credential on failure so logout can retry.
      if (token === previous) token = null;
    })().finally(() => { revocation = null; });
    return revocation;
  }
  const server = createServer(async (req, res) => {
    try {
      const url = new URL(req.url, redirect);
      const expected = new URL(redirect);
      const supplied = Buffer.from(url.searchParams.get('state') || '');
      if (consumed || req.method !== 'GET' || req.headers.host !== expected.host || url.origin !== expected.origin || url.pathname !== expected.pathname || req.url.split('?')[0] !== expected.pathname || supplied.length !== state.length || !timingSafeEqual(supplied, Buffer.from(state)) || !url.searchParams.get('code') || url.searchParams.getAll('state').length !== 1 || url.searchParams.getAll('code').length !== 1) throw fail('CALLBACK_DENIED');
      consumed = true;
      const received = await exchange({ code: url.searchParams.get('code'), verifier, redirect });
      token = received;
      if (loggedOut) { await revokePending(); res.writeHead(400); res.end('Authorization closed.'); return; }
      res.writeHead(200); res.end('Authorization complete.');
      server.close();
    } catch { res.writeHead(400); res.end('Authorization rejected.'); if (consumed) server.close(); }
  });
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  redirect = `http://127.0.0.1:${server.address().port}/oauth/callback`;
  return { state, challenge, method: 'S256', redirect,
    async logout() { consumed = true; loggedOut = true; server.close(); await revokePending(); },
    close() { consumed = true; loggedOut = true; server.close(); } };
}
// OpenSSL-backed Node primitives. No custom cryptography or plaintext fallback.
export class PassphraseVault {
  #key = null; #values = null; #salt;
  static kdf = Object.freeze({ name: 'scrypt', N: 32768, r: 8, p: 1, keyBytes: 32 });
  static maxPlaintextBytes = 2 * 1024 * 1024;
  static create(passphrase) {
    if (typeof passphrase !== 'string' || passphrase.length < 12) throw fail('WEAK_PASSPHRASE');
    const vault = new PassphraseVault(); vault.#salt = randomBytes(16);
    vault.#key = vault.#derive(passphrase); vault.#values = Object.create(null); return vault;
  }
  #derive(passphrase) { return scryptSync(passphrase, this.#salt, 32, { N: 32768, r: 8, p: 1, maxmem: 64 * 1024 * 1024 }); }
  unlock(blob, passphrase) {
    this.lock();
    try {
      if (typeof blob !== 'string' || blob.length > 4 * 1024 * 1024) throw fail('VAULT_INVALID');
      const envelope = JSON.parse(blob);
      if (envelope.version !== 1 || envelope.kdf !== 'scrypt-32768-8-1' || envelope.cipher !== 'aes-256-gcm') throw fail('VAULT_INVALID');
      this.#salt = Buffer.from(envelope.salt, 'base64');
      const nonce = Buffer.from(envelope.nonce, 'base64'), tag = Buffer.from(envelope.tag, 'base64');
      if (this.#salt.length !== 16 || nonce.length !== 12 || tag.length !== 16) throw fail('VAULT_INVALID');
      this.#key = this.#derive(passphrase);
      const decipher = createDecipheriv('aes-256-gcm', this.#key, nonce);
      decipher.setAAD(Buffer.from('projectos-vault-v1')); decipher.setAuthTag(tag);
      const plaintext = Buffer.concat([decipher.update(Buffer.from(envelope.data, 'base64')), decipher.final()]);
      if (plaintext.length > PassphraseVault.maxPlaintextBytes) throw fail('VAULT_CAPACITY');
      this.#values = JSON.parse(plaintext.toString());
      if (!this.#values || Array.isArray(this.#values) || typeof this.#values !== 'object' || Object.values(this.#values).some(value => typeof value !== 'string')) throw fail('VAULT_INVALID');
    } catch { this.lock(); throw fail('VAULT_UNLOCK_FAILED'); }
  }
  #require() { if (!this.#key) throw fail('VAULT_LOCKED'); }
  put(handle, secret) {
    this.#require(); if (typeof handle !== 'string' || typeof secret !== 'string') throw fail('VAULT_INVALID');
    const candidate = Object.assign(Object.create(null), this.#values);
    Object.defineProperty(candidate, handle, { value: secret, enumerable: true, configurable: true, writable: true });
    if (Buffer.byteLength(JSON.stringify(candidate)) > PassphraseVault.maxPlaintextBytes) throw fail('VAULT_CAPACITY');
    this.#values = candidate;
  }
  get(handle) { this.#require(); return Object.hasOwn(this.#values, handle) ? this.#values[handle] : undefined; }
  exportEncrypted() {
    this.#require(); const nonce = randomBytes(12), cipher = createCipheriv('aes-256-gcm', this.#key, nonce);
    cipher.setAAD(Buffer.from('projectos-vault-v1'));
    const data = Buffer.concat([cipher.update(JSON.stringify(this.#values)), cipher.final()]);
    return JSON.stringify({ version: 1, kdf: 'scrypt-32768-8-1', cipher: 'aes-256-gcm', salt: this.#salt.toString('base64'), nonce: nonce.toString('base64'), tag: cipher.getAuthTag().toString('base64'), data: data.toString('base64') });
  }
  lock() { this.#key?.fill(0); this.#key = null; this.#values = null; }
  toJSON() { return { locked: !this.#key }; }
}
