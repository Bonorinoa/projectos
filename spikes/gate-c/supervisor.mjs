import { realpath, open, writeFile } from "node:fs/promises";
import { constants as fsConstants } from "node:fs";
import { randomBytes } from "node:crypto";
import { dirname, join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const PROTOCOL = "projectos.worker";
const VERSION = 1;
const MAX_FRAME_BYTES = 32 * 1024;
const MAX_OUTPUT_BYTES = 64 * 1024;

function parseArgs(argv) {
  const values = {};
  for (let i = 2; i < argv.length; i += 2) values[argv[i].replace(/^--/, "")] = argv[i + 1];
  return values;
}

function sandboxQuote(value) {
  return `\"${value.replaceAll("\\", "\\\\").replaceAll("\"", "\\\"")}\"`;
}

async function sandboxProfile({ node, worker, allowControlledChild }) {
  const nodeReal = await realpath(node);
  const nodeRoot = dirname(nodeReal);
  const workerReal = await realpath(worker);
  const childRule = allowControlledChild
    ? `(allow process-fork)\n(allow process-exec (literal \"/bin/sleep\"))\n(allow file-read* (literal \"/bin/sleep\"))`
    : "";
  return `(version 1)
(deny default)
(allow process-exec (literal ${sandboxQuote(nodeReal)}))
${childRule}
(allow file-read*
  (literal \"/\")
  (subpath \"/System/Library\")
  (subpath \"/usr/lib\")
  (subpath \"/usr/share\")
  (subpath \"/dev\")
  (subpath ${sandboxQuote(nodeRoot)})
  (literal ${sandboxQuote(workerReal)}))
(allow file-read-metadata)
(allow sysctl-read)
(allow mach-lookup)
(allow ipc-posix-shm)
(allow file-write-data)
(allow signal (target self))
`;
}

async function readScopedFile(rootHandle, identity, requested) {
  if (requested.includes("\0")) throw Object.assign(new Error("NUL path denied"), { code: "CAP_PATH_ESCAPE" });
  const stdout = await new Promise((resolveOutput, reject) => {
    const broker = spawn(join(here, "build/file-broker"), ["--fd", String(identity.dev), String(identity.ino), requested],
      { stdio: ["ignore", "pipe", "pipe", rootHandle.fd], env: {} });
    let output = "";
    const timer = setTimeout(() => { broker.kill("SIGKILL"); reject(new Error("broker timeout")); }, 1500);
    broker.stdout.on("data", (chunk) => {
      output += chunk.toString();
      if (Buffer.byteLength(output) > 65536) { broker.kill("SIGKILL"); reject(new Error("broker output limit")); }
    });
    broker.on("error", (error) => { clearTimeout(timer); reject(error); });
    broker.on("close", (code) => { clearTimeout(timer); code === 0 ? resolveOutput(output) : reject(new Error("broker failed")); });
  });
  const payload = JSON.parse(stdout);
  if (!payload.ok) throw Object.assign(new Error(payload.error.message), { code: payload.error.code });
  const { hex, ...metadata } = payload.result;
  return { ...metadata, content: Buffer.from(hex, "hex").toString("utf8") };
}

async function main() {
  const args = parseArgs(process.argv);
  const node = args.node ?? process.execPath;
  const worker = resolve(args.worker ?? join(here, "worker.ts"));
  const guardian = resolve(args.guardian ?? join(here, "build/guardian"));
  const profilePath = resolve(args.profile ?? join(here, "build/worker.sb"));
  const pidFile = resolve(args["pid-file"] ?? join(here, "build/worker.pid"));
  const action = args.action ?? "env-report";
  const expectedVersion = Number(args["expected-version"] ?? VERSION);
  const timeoutMs = Number(args["timeout-ms"] ?? 5000);
  const cpuSeconds = Number(args["cpu-seconds"] ?? 3);
  const memoryBytes = Number(args["memory-bytes"] ?? 536870912);
  const fileBytes = Number(args["file-bytes"] ?? 1048576);
  const allowControlledChild = args["allow-controlled-child"] === "true";
  const capabilityRoot = resolve(args["cap-root"] ?? join(here, "fixtures/capability-root"));
  const capabilityRootHandle = await open(capabilityRoot, fsConstants.O_RDONLY | fsConstants.O_DIRECTORY | fsConstants.O_NOFOLLOW);
  const capabilityIdentity = await capabilityRootHandle.stat({ bigint: true });
  const capability = `cap:${randomBytes(24).toString("base64url")}`;
  const profile = await sandboxProfile({ node, worker, allowControlledChild });
  await writeFile(profilePath, profile, { mode: 0o600 });

  const command = "/usr/bin/sandbox-exec";
  const commandArgs = ["-f", profilePath, node, "--experimental-strip-types", worker];
  const child = spawn(guardian, [String(process.pid), pidFile, String(cpuSeconds), String(memoryBytes), String(fileBytes), command, ...commandArgs], {
    stdio: ["pipe", "pipe", "pipe"],
    env: { PROJECTOS_PROTOCOL_VERSION: String(expectedVersion) }
  });

  let totalOutput = 0;
  let stderrText = "";
  let settled = false;
  let helloSeen = false;
  const transcript = [];
  const startedAt = Date.now();
  const runId = "run-1";

  const finish = (result, exitCode = 0) => {
    if (settled) return;
    settled = true;
    clearTimeout(timer);
    if (!child.killed) child.kill("SIGTERM");
    void capabilityRootHandle.close();
    process.stdout.write(`${JSON.stringify({ ...result, action, elapsedMs: Date.now() - startedAt, stderr: stderrText.slice(0, 4000), transcript }, null, 2)}\n`);
    process.exitCode = exitCode;
  };

  const timer = setTimeout(() => finish({ status: "terminated", reason: "wall-time-ceiling" }, 2), timeoutMs);

  child.stderr.on("data", (chunk) => {
    totalOutput += chunk.length;
    stderrText += chunk.toString("utf8");
    if (totalOutput > MAX_OUTPUT_BYTES) finish({ status: "terminated", reason: "output-backpressure" }, 2);
  });

  child.stdout.on("data", (chunk) => {
    totalOutput += chunk.length;
    if (totalOutput > MAX_OUTPUT_BYTES) finish({ status: "terminated", reason: "output-backpressure" }, 2);
  });

  const lines = createInterface({ input: child.stdout, crlfDelay: Infinity });
  lines.on("line", async (line) => {
    if (Buffer.byteLength(line) > MAX_FRAME_BYTES || totalOutput > MAX_OUTPUT_BYTES) {
      finish({ status: "terminated", reason: "output-backpressure" }, 2);
      return;
    }
    let incoming;
    try {
      incoming = JSON.parse(line);
    } catch {
      finish({ status: "terminated", reason: "malformed-frame" }, 2);
      return;
    }
    const transcriptEntry = incoming.payload?.block
      ? { ...incoming, payload: { ...incoming.payload, block: `<${Buffer.byteLength(incoming.payload.block)} bytes>` } }
      : incoming;
    transcript.push(transcriptEntry);
    if (incoming.protocol !== PROTOCOL || incoming.version !== expectedVersion) {
      finish({ status: "terminated", reason: "protocol-mismatch" }, 2);
      return;
    }
    if (incoming.type === "hello") {
      helloSeen = true;
      child.stdin.write(`${JSON.stringify({ protocol: PROTOCOL, version: expectedVersion, type: "hello_ack", id: "hello-ack-1", replyTo: incoming.id, payload: { selectedVersion: expectedVersion } })}\n`);
      const payload = { action };
      if (args.path) payload.path = resolve(args.path);
      if (args["relative-path"]) payload.relativePath = args["relative-path"];
      if (action === "broker-read") payload.capability = args["invalid-capability"] === "true" ? "cap:invalid" : capability;
      child.stdin.write(`${JSON.stringify({ protocol: PROTOCOL, version: expectedVersion, type: "run", id: runId, payload })}\n`);
      return;
    }
    if (incoming.type === "broker.request") {
      let payload;
      if (incoming.payload?.capability !== capability || incoming.payload?.op !== "fs.read") {
        payload = { ok: false, error: { code: "CAPABILITY_DENIED", message: "unknown or invalid capability" } };
      } else {
        try {
          const result = await readScopedFile(capabilityRootHandle, capabilityIdentity, String(incoming.payload?.args?.relativePath ?? ""));
          payload = { ok: true, result };
        } catch (error) {
          payload = { ok: false, error: { code: error.code ?? "BROKER_ERROR", message: String(error.message).slice(0, 200) } };
        }
      }
      child.stdin.write(`${JSON.stringify({ protocol: PROTOCOL, version: expectedVersion, type: "broker.response", id: `response-${incoming.id}`, replyTo: incoming.id, payload })}\n`);
      return;
    }
    if ((incoming.type === "result" || incoming.type === "error") && incoming.replyTo === runId) {
      finish({ status: incoming.type === "result" ? "completed" : "worker-error", helloSeen, result: incoming.payload }, incoming.type === "result" ? 0 : 1);
    }
  });

  child.on("exit", (code, signal) => {
    if (!settled) finish({ status: "terminated", reason: "worker-exit", code, signal, helloSeen }, 2);
  });
  child.on("error", (error) => finish({ status: "launch-error", reason: error.message }, 2));
}

main().catch((error) => {
  process.stderr.write(`${error.stack ?? error}\n`);
  process.exitCode = 1;
});
