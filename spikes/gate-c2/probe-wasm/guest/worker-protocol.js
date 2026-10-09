// ProjectOS Gate C2 probe — worker protocol core ported to a WASM guest.
// Ported from spikes/gate-c/worker.ts minus Node builtins (fs, child_process,
// net, dns, readline). In this topology those APIs do not exist by construction
// (C3/C4); what must survive unchanged is the NDJSON protocol engine.
// Host: wasmtime 49.0.2. I/O: javy stdin/stdout convention (one frame per run).
"use strict";
const PROTOCOL = "projectos.worker";
const VERSION = 1;

function send(m) {
  const bytes = new TextEncoder().encode(JSON.stringify(m) + "\n");
  Javy.IO.writeSync(1, bytes);
}

function message(type, id, payload, replyTo) {
  const m = { protocol: PROTOCOL, version: VERSION, type, id, payload: payload || {} };
  if (replyTo !== undefined) m.replyTo = replyTo;
  return m;
}

function readAll() {
  const chunks = [];
  for (;;) {
    const buf = new Uint8Array(65536);
    const n = Javy.IO.readSync(0, buf);
    if (n <= 0) break;
    chunks.push(buf.subarray(0, n));
    if (n < 65536) break;
  }
  const total = chunks.reduce((s, c) => s + c.length, 0);
  const out = new Uint8Array(total);
  let off = 0;
  for (const c of chunks) { out.set(c, off); off += c.length; }
  return new TextDecoder().decode(out);
}

let counter = 0;

function handle(frame) {
  if (!frame || frame.protocol !== PROTOCOL || frame.version !== VERSION) {
    return message("error", String(frame && frame.id || "bad"), { message: "protocol/version mismatch" });
  }
  switch (frame.type) {
    case "hello":
      return message("hello.ack", String(frame.id), {
        protocol: PROTOCOL, version: VERSION,
        capabilities: ["broker.request"],
        confinement: "wasm-guest: no fs/net/process APIs by construction"
      });
    case "broker.request": {
      const op = frame.payload && frame.payload.op;
      const cap = frame.payload && frame.payload.capability;
      if (op === "read" && typeof cap === "string" && cap.length <= 256) {
        return message("broker.result", String(frame.id), { op, capability: cap, result: "ok", len: cap.length });
      }
      return message("broker.result", String(frame.id), { op, capability: cap, result: "denied", reason: "capability not granted" });
    }
    case "count":
      counter += 1;
      return message("count.ack", String(frame.id), { count: counter });
    default:
      return message("error", String(frame.id), { message: "unknown type " + frame.type });
  }
}

const input = readAll();
let frame;
try { frame = JSON.parse(input); } catch (e) { send(message("error", "bad", { message: "parse error" })); }
send(handle(frame));