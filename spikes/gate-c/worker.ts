import { readFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import { connect } from "node:net";
import { lookup } from "node:dns/promises";
import { createInterface } from "node:readline";

const PROTOCOL = "projectos.worker";
const VERSION = 1;
let nextId = 1;
const pending = new Map<string, (message: Message) => void>();

type Message = {
  protocol: typeof PROTOCOL;
  version: number;
  type: string;
  id: string;
  replyTo?: string;
  payload?: Record<string, unknown>;
};

function message(type: string, id: string, payload: Record<string, unknown> = {}, replyTo?: string): Message {
  return { protocol: PROTOCOL, version: VERSION, type, id, ...(replyTo ? { replyTo } : {}), payload };
}

function send(value: Message): void {
  process.stdout.write(`${JSON.stringify(value)}\n`);
}

function serializeError(error: unknown): Record<string, unknown> {
  const candidate = error as NodeJS.ErrnoException;
  return {
    name: candidate?.name ?? "Error",
    code: candidate?.code ?? null,
    message: String(candidate?.message ?? error).slice(0, 300)
  };
}

async function brokerRequest(op: string, capability: string, args: Record<string, unknown>): Promise<Message> {
  const id = `broker-${nextId++}`;
  send(message("broker.request", id, { op, capability, args }));
  return await new Promise((resolve) => pending.set(id, resolve));
}

async function tcpProbe(host: string, port: number): Promise<Record<string, unknown>> {
  return await new Promise((resolve) => {
    const socket = connect({ host, port });
    const timer = setTimeout(() => {
      socket.destroy();
      resolve({ host, denied: true, outcome: "timeout" });
    }, 1200);
    socket.once("connect", () => {
      clearTimeout(timer);
      socket.destroy();
      resolve({ host, denied: false, outcome: "connected" });
    });
    socket.once("error", (error) => {
      clearTimeout(timer);
      resolve({ host, denied: true, error: serializeError(error) });
    });
  });
}

async function childProbe(command: string, args: string[]): Promise<Record<string, unknown>> {
  return await new Promise((resolve) => {
    let settled = false;
    let child;
    try {
      child = spawn(command, args, { stdio: "ignore" });
    } catch (error) {
      resolve({ denied: true, error: serializeError(error) });
      return;
    }
    child.once("error", (error) => {
      settled = true;
      resolve({ denied: true, error: serializeError(error) });
    });
    child.once("spawn", () => {
      if (settled) return;
      child.kill("SIGKILL");
      resolve({ denied: false, pid: child.pid });
    });
  });
}

async function execute(action: string, payload: Record<string, unknown>): Promise<Record<string, unknown>> {
  switch (action) {
    case "env-report": {
      return { keys: Object.keys(process.env).sort(), values: { PROJECTOS_PROTOCOL_VERSION: process.env.PROJECTOS_PROTOCOL_VERSION ?? null } };
    }
    case "broker-read": {
      const response = await brokerRequest("fs.read", String(payload.capability), { relativePath: String(payload.relativePath) });
      return { broker: response.payload };
    }
    case "direct-file": {
      try {
        const content = await readFile(String(payload.path), "utf8");
        return { denied: false, content: content.slice(0, 80) };
      } catch (error) {
        return { denied: true, error: serializeError(error) };
      }
    }
    case "network": {
      let dns: Record<string, unknown>;
      try {
        const result = await lookup("example.com");
        dns = { denied: false, result };
      } catch (error) {
        dns = { denied: true, error: serializeError(error) };
      }
      return {
        dns,
        ipv4: await tcpProbe("93.184.216.34", 80),
        ipv6: await tcpProbe("2606:2800:220:1:248:1893:25c8:1946", 80),
        localhost4: await tcpProbe("127.0.0.1", 9),
        localhost6: await tcpProbe("::1", 9)
      };
    }
    case "redirect": {
      // A redirect cannot be reached when the initiating socket itself is denied.
      return { firstHop: await tcpProbe("93.184.216.34", 80), redirectFollowed: false };
    }
    case "child": {
      return await childProbe("/bin/sh", ["-c", "exit 0"]);
    }
    case "controlled-child": {
      const child = spawn("/bin/sleep", ["300"], { stdio: "ignore" });
      await new Promise<void>((resolve, reject) => {
        child.once("spawn", resolve);
        child.once("error", reject);
      });
      send(message("result", `event-${nextId++}`, { controlledChildPid: child.pid, intermediate: true }));
      await new Promise(() => {});
      return {};
    }
    case "sleep": {
      await new Promise((resolve) => setTimeout(resolve, Number(payload.ms ?? 60_000)));
      return { completed: true };
    }
    case "cpu": {
      let value = 0;
      while (true) value = Math.imul(value + 1, 2654435761);
    }
    case "memory": {
      const chunks: Buffer[] = [];
      while (true) {
        const chunk = Buffer.allocUnsafe(16 * 1024 * 1024);
        chunk.fill(0xa5);
        chunks.push(chunk);
      }
    }
    case "flood": {
      const block = "x".repeat(8 * 1024);
      while (true) send(message("result", `flood-${nextId++}`, { intermediate: true, block }));
    }
    default:
      throw new Error(`unknown action: ${action}`);
  }
}

const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
lines.on("line", async (line) => {
  let incoming: Message;
  try {
    incoming = JSON.parse(line) as Message;
  } catch (error) {
    send(message("error", `error-${nextId++}`, { error: serializeError(error) }));
    return;
  }

  if (incoming.type === "broker.response" && incoming.replyTo) {
    const resolve = pending.get(incoming.replyTo);
    if (resolve) {
      pending.delete(incoming.replyTo);
      resolve(incoming);
    }
    return;
  }

  if (incoming.protocol !== PROTOCOL || incoming.version !== VERSION) {
    send(message("error", `error-${nextId++}`, { code: "PROTOCOL_MISMATCH", supported: [VERSION] }, incoming.id));
    return;
  }

  if (incoming.type === "hello_ack") return;
  if (incoming.type !== "run") return;

  try {
    const action = String(incoming.payload?.action ?? "");
    const result = await execute(action, incoming.payload ?? {});
    send(message("result", `result-${nextId++}`, result, incoming.id));
  } catch (error) {
    send(message("error", `error-${nextId++}`, { error: serializeError(error) }, incoming.id));
  }
});

send(message("hello", "hello-1", { worker: { name: "projectos-gate-c", version: "0.1.0" }, supportedVersions: [VERSION] }));
