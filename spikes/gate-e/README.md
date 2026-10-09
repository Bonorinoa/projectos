# Gate E provider contract spike

Mock-only implementation. No real API key, OAuth account, provider request, or billed call is required. `api.mock.example` is a fixture destination and is never contacted. The only real socket is an ephemeral IPv4 loopback OAuth callback used by the tests.

Run from the repository root:

```sh
cd spikes/gate-e
node --version # exact evidence runtime: v22.22.3
npm test
node evidence.mjs
```

The evidence runner records TAP output, runtime/OpenSSL versions, exit status and SHA-256 hashes of the implementation, tests and reproduction files. Running it regenerates `evidence/gates/gate-e/report.json` and `tests.tap`. Loopback binding requires execution outside a socket-denying sandbox. No npm packages are installed: cryptography uses Node's maintained OpenSSL implementation, AES-256-GCM with fresh random 96-bit nonce and scrypt N=32768/r=8/p=1, fresh 128-bit salt and 256-bit key. The exact Node version is pinned in `package.json`; native Rust cryptographic dependency selection remains future work.

`broker.mjs` provides a credential store with opaque handles and exact endpoint binding; transport alone receives authorization. The broker checks the full fixed route, input hash, cost estimate and token budget before sending. It records requested and provider-reported models, separate estimate/usage/billed/unknown cost fields, visible fallback attempts and redacted immutable receipt copies. Input/output text stays out of receipts. Provider errors expose fixed codes. Known credential text is redacted from reported models and stream chunks, including split chunks. Requests are snapshotted before asynchronous work, concurrent calls cannot race the budget, and uncertain sends block subsequent calls. The safe fallback fixture must explicitly report a pre-inference rejection; ambiguous errors never cause automatic spending retries.

Offline drafts retain input/route/budget snapshots. Reconnection only changes online state. Execution requires affirmative review and matching input and entire-request hashes. Switching offline aborts an active transport; the mock observes its abort signal. Actual OS DNS/network denial is Gate C/native broker work and is not established by this Node switch.

`auth.mjs` implements PKCE S256, cryptographic random state/verifier, a random one-shot localhost port, exact method/host/origin/path validation, duplicate state/code rejection and logout revocation through an injected mock adapter. The encrypted vault supports lock/unlock and encrypted-backup restoration, and rejects wrong passphrases and authenticated-content tampering without plaintext fallback. Vault recovery here means restoration from an encrypted backup with its passphrase, not recovery after forgotten passphrase or lost key. JavaScript garbage collection cannot guarantee zeroization of strings; use the native credential broker for production secrets.

This passes only the mock contract portion of Gate E. Full Gate E remains conditional until Rust integration, OS keyring, Linux vault parity, real egress enforcement and explicit opt-in provider/OAuth conformance tests pass. No claim is made that a provider-reported model proves the actual backend model or that caller-supplied estimates can enforce remote billing. No remote endpoint or browser authorization page is opened by this spike.
