// Gate C2 probe — what host/ambient surfaces exist inside the WASM guest? (C3/C4)
// Expect: no process, no spawn, no fetch, no require, no node builtins.
"use strict";
const info = {};
info.readonly = true;
info.processGlobal = typeof globalThis.process;
info.spawnPresent = typeof (globalThis.process && globalThis.process.spawn);
info.fetchPresent = typeof globalThis.fetch;
info.requirePresent = typeof globalThis.require;
info.TextDecoder = typeof TextDecoder;
try { info.nodeBuiltinImport = typeof require("node:child_process"); }
catch (e) { info.nodeBuiltinImport = "denied: " + String(e).slice(0, 140); }
let netAttempt = "not-attempted";
try {
  if (typeof WebSocket !== "undefined") { netAttempt = "WebSocket-present"; }
} catch (e) { netAttempt = "WebSocket-denied"; }
info.netAttempt = netAttempt;
Javy.IO.writeSync(1, new TextEncoder().encode(JSON.stringify(info) + "\n"));