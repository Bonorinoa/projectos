import { createHash } from "node:crypto";
import { access, mkdir, readFile, realpath, rm, symlink, writeFile, copyFile, chmod, stat } from "node:fs/promises";
import { constants as fsConstants } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn, spawnSync } from "node:child_process";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../..");
const build = join(here, "build");
const evidence = join(root, "evidence/gates/gate-c");
const supervisor = join(here, "supervisor.mjs");
const guardianSource = join(here, "guardian.c");
const guardian = join(build, "guardian");
const worker = join(here, "worker.ts");
const protocol = join(here, "protocol.schema.json");
const capRoot = join(here, "fixtures/capability-root");
const ambientSecret = join(here, "fixtures/ambient-secret.txt");
const symlinkPath = join(capRoot, "escape-link.txt");
const outsideFile = join(here, "fixtures/outside.txt");
const injectedSecretName = "PROJECTOS_GATE_C_INJECTED_SECRET";
const injectedSecretValue = "must-not-reach-worker";
const raw = {};
const runtimeManifest = JSON.parse(await readFile(join(here, "runtime-pin.json"), "utf8"));
if (process.version !== runtimeManifest.version || process.arch !== runtimeManifest.arch || process.platform !== runtimeManifest.os) {
  throw new Error("Runtime platform/version differs from runtime-pin.json; reviewed re-pin required");
}
if (await sha256(process.execPath) !== runtimeManifest.sha256) throw new Error("Runtime SHA256 differs from reviewed pin");

function command(commandName, args, options = {}) {
  const result = spawnSync(commandName, args, {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 16 * 1024 * 1024,
    ...options
  });
  return {
    command: [commandName, ...args],
    status: result.status,
    signal: result.signal,
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? "",
    error: result.error?.message ?? null
  };
}

function parseSupervisorOutput(output) {
  try {
    return JSON.parse(output);
  } catch {
    return { status: "unparseable", stdout: output };
  }
}

function runSupervisor(name, args, timeout = 15_000) {
  const result = command(process.execPath, [supervisor, "--node", join(build, "runtime/node"), "--guardian", guardian, "--worker", worker, ...args], {
    timeout,
    env: { ...process.env, [injectedSecretName]: injectedSecretValue }
  });
  const parsed = parseSupervisorOutput(result.stdout);
  raw[name] = { ...result, parsed };
  return parsed;
}

async function sha256(path) {
  return createHash("sha256").update(await readFile(path)).digest("hex");
}

function criterion(id, label, status, evidenceText, blocker = null) {
  return { id, label, status, evidence: evidenceText, blocker };
}

async function waitUntil(predicate, timeoutMs = 4000, intervalMs = 40) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const result = await predicate();
      if (result) return result;
    } catch {}
    await new Promise((resolvePromise) => setTimeout(resolvePromise, intervalMs));
  }
  return null;
}

function isAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error.code !== "ESRCH";
  }
}

function childrenOf(parentPid) {
  const result = command("/bin/ps", ["-axo", "pid=,ppid=,command="]);
  if (result.status !== 0) return [];
  return result.stdout.split("\n").map((line) => line.trim().match(/^(\d+)\s+(\d+)\s+(.*)$/)).filter(Boolean)
    .filter((match) => Number(match[2]) === parentPid)
    .map((match) => ({ pid: Number(match[1]), command: match[3] }));
}

async function processLifecycleProbe(name, controlledChild, signal) {
  const pidFile = join(build, `${name}.pid`);
  await rm(pidFile, { force: true });
  const args = [supervisor, "--node", join(build, "runtime/node"), "--guardian", guardian, "--worker", worker, "--action", controlledChild ? "controlled-child" : "sleep", "--timeout-ms", "30000", "--pid-file", pidFile];
  if (controlledChild) args.push("--allow-controlled-child", "true");
  const parent = spawn(process.execPath, args, {
    cwd: root,
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, [injectedSecretName]: injectedSecretValue }
  });
  let stdout = "";
  let stderr = "";
  parent.stdout.on("data", (chunk) => { stdout += chunk.toString(); });
  parent.stderr.on("data", (chunk) => { stderr += chunk.toString(); });

  const workerPidText = await waitUntil(async () => {
    try { return (await readFile(pidFile, "utf8")).trim(); } catch { return null; }
  });
  const workerPid = workerPidText ? Number(workerPidText) : NaN;
  const children = controlledChild
    ? await waitUntil(() => {
        const found = childrenOf(workerPid).filter((entry) => entry.command.includes("/bin/sleep"));
        return found.length ? found : null;
      }, 5000)
    : [];

  if (Number.isFinite(parent.pid)) {
    try { process.kill(parent.pid, signal); } catch {}
  }
  await new Promise((resolvePromise) => setTimeout(resolvePromise, 600));
  const workerAlive = Number.isFinite(workerPid) && isAlive(workerPid);
  const childStates = (children ?? []).map((entry) => ({ ...entry, alive: isAlive(entry.pid) }));
  if (workerAlive) {
    try { process.kill(workerPid, "SIGKILL"); } catch {}
  }
  for (const child of childStates.filter((entry) => entry.alive)) {
    try { process.kill(child.pid, "SIGKILL"); } catch {}
  }
  raw[name] = { parentPid: parent.pid, workerPid, children: childStates, workerAlive, signal, stdout, stderr };
  return { started: Number.isFinite(workerPid), controlledChildStarted: !controlledChild || (children?.length ?? 0) > 0, workerAlive, childStates };
}

await mkdir(build, { recursive: true });
await mkdir(evidence, { recursive: true });
await mkdir(join(build, "runtime"), { recursive: true });
await copyFile(process.execPath, join(build, "runtime/node"));
await chmod(join(build, "runtime/node"), 0o755);
for (const name of ["file-broker", "broker-race-test"]) {
  raw[`compile_${name}`] = command("/usr/bin/cc", ["-std=c11", "-Wall", "-Wextra", "-Werror", "-O2", join(here, `${name}.c`), "-o", join(build, name)]);
  if (raw[`compile_${name}`].status !== 0) throw new Error(raw[`compile_${name}`].stderr);
}
const raceRoot = join(build, "race-root");
const raceOutside = join(build, "race-outside");
await mkdir(join(raceRoot, "slot"), { recursive: true });
await mkdir(raceOutside, { recursive: true });
await writeFile(join(raceRoot, "slot/data.txt"), "INSIDE\n");
await writeFile(join(raceOutside, "data.txt"), "OUTSIDE_SECRET\n");
raw.broker_race = command(join(build, "broker-race-test"), [raceRoot, raceOutside], { timeout: 30000 });
const race = JSON.parse(raw.broker_race.stdout);
await mkdir(join(build, "broker-contract"), { recursive: true });
const contractRoot = join(build, "broker-contract");
await writeFile(join(contractRoot, "allowed.txt"), "INSIDE\n");
await writeFile(join(contractRoot, "oversized.txt"), Buffer.alloc(17000, 120));
await rm(join(contractRoot, "final-link"), { force: true });
await rm(join(contractRoot, "parent-link"), { force: true });
await symlink(outsideFile, join(contractRoot, "final-link"));
await symlink(raceOutside, join(contractRoot, "parent-link"));
await rm(join(contractRoot, "fifo"), { force: true });
raw.mkfifo = command("/usr/bin/mkfifo", [join(contractRoot, "fifo")]);
const contractIdentity = await stat(contractRoot, { bigint: true });
const nativeCases = [
  ["allowed", "allowed.txt", null, true],
  ["absolute", "/etc/passwd", "CAP_PATH_ABSOLUTE"],
  ["traversal", "sub/../allowed.txt", "CAP_PATH_ESCAPE"],
  ["final_symlink", "final-link", "CAP_SYMLINK"],
  ["parent_symlink", "parent-link/data.txt", "CAP_SYMLINK"],
  ["root_identity", "allowed.txt", "CAP_ROOT_CHANGED", false, "0"],
  ["size", "oversized.txt", "CAP_SIZE_LIMIT"],
  ["fifo", "fifo", "CAP_NOT_FILE"]
];
const nativeResults = nativeCases.map(([name, path, error, allowed, inode]) => {
  const result = command(join(build, "file-broker"), [contractRoot, String(contractIdentity.dev), inode ?? String(contractIdentity.ino), path], { timeout: 1500 });
  const payload = parseSupervisorOutput(result.stdout);
  const passed = result.status === 0 && (allowed ? payload.ok === true && payload.result.hex === Buffer.from("INSIDE\n").toString("hex") : payload.ok === false && payload.error?.code === error);
  raw[`native_${name}`] = { ...result, payload, passed };
  return { name, passed };
});
raw.runtime_dependency_audit = command("/usr/bin/otool", ["-L", join(build, "runtime/node")]);
await rm(symlinkPath, { force: true });
await symlink("../outside.txt", symlinkPath);

const compile = command("/usr/bin/cc", ["-std=c11", "-Wall", "-Wextra", "-Werror", "-O2", guardianSource, "-o", guardian]);
raw.compile = compile;
if (compile.status !== 0) throw new Error(`guardian compilation failed: ${compile.stderr}`);

const envReport = runSupervisor("environment", ["--action", "env-report"]);
const brokerAllowed = runSupervisor("broker_allowed", ["--action", "broker-read", "--cap-root", capRoot, "--relative-path", "allowed.txt"]);
const brokerInvalid = runSupervisor("broker_invalid", ["--action", "broker-read", "--cap-root", capRoot, "--relative-path", "allowed.txt", "--invalid-capability", "true"]);
const traversal = runSupervisor("path_traversal", ["--action", "broker-read", "--cap-root", capRoot, "--relative-path", "../outside.txt"]);
const symlinkEscape = runSupervisor("symlink_escape", ["--action", "broker-read", "--cap-root", capRoot, "--relative-path", "escape-link.txt"]);
const ambientFile = runSupervisor("ambient_file", ["--action", "direct-file", "--path", ambientSecret]);
const network = runSupervisor("network", ["--action", "network"], 20_000);
const redirect = runSupervisor("redirect", ["--action", "redirect"], 10_000);
const child = runSupervisor("child_process", ["--action", "child"]);
const mismatch = runSupervisor("protocol_mismatch", ["--action", "env-report", "--expected-version", "2"]);
const cancelled = runSupervisor("wall_cancel", ["--action", "sleep", "--timeout-ms", "500"], 10_000);
const cpu = runSupervisor("cpu_limit", ["--action", "cpu", "--cpu-seconds", "1", "--timeout-ms", "5000"], 10_000);
const flood = runSupervisor("output_backpressure", ["--action", "flood", "--timeout-ms", "5000"], 10_000);
const orphan = await processLifecycleProbe("parent_crash", false, "SIGKILL");
const treeCancel = await processLifecycleProbe("tree_cancel", true, "SIGTERM");

const man = command("/usr/bin/man", ["sandbox-exec"], { env: { ...process.env, MANPAGER: "cat" } });
raw.sandbox_exec_manual = man;
const nodeReal = await realpath(process.execPath);
let appSandboxReport = null;
try { appSandboxReport = JSON.parse(await readFile(join(evidence, "app-sandbox-report.json"), "utf8")); } catch {}
if (appSandboxReport) {
  for (const [path, expected] of Object.entries(appSandboxReport.sourceHashes ?? {})) {
    if (await sha256(join(root, path)) !== expected) { appSandboxReport = null; break; }
  }
}
const platform = {
  os: process.platform,
  arch: process.arch,
  nodeVersion: process.version,
  nodePath: nodeReal,
  swVers: command("/usr/bin/sw_vers", []).stdout.trim(),
  uname: command("/usr/bin/uname", ["-a"]).stdout.trim()
};

const deniedCode = (probe) => ["EPERM", "EACCES"].includes(probe?.error?.code);
const networkPayload = network?.result ?? {};
const criteria = [
  criterion("C-PROTOCOL-POSITIVE", "Protocol v1 negotiation", envReport.status === "completed" && envReport.helloSeen ? "pass" : "fail", `helloSeen=${envReport.helloSeen ?? false}; worker completed=${envReport.status === "completed"}`),
  criterion("C-PROTOCOL-NEGATIVE", "Unsupported protocol rejected", mismatch.reason === "protocol-mismatch" ? "pass" : "fail", `reason=${mismatch.reason ?? "missing"}`),
  criterion("C-PACKAGE-MACOS", "Shippable TypeScript worker package on macOS", "fail", `The worker now runs a locally copied SHA256-pinned Node ${process.version} arm64 runtime. This proves local bundling only: upstream provenance/license bundle, universal build, supported confinement, signing/notarization and clean-machine install remain unproven.`, "Complete the signed distributable package matrix before claiming macOS shipment."),
  criterion("C-PACKAGE-LINUX", "TypeScript worker ships on frozen Linux matrix", "untested", "No Linux host or frozen distribution/package matrix was available in this run.", "Gate C requires both operating systems."),
  criterion("C-ENV", "No inherited secrets", envReport.status === "completed" && Array.isArray(envReport.result?.keys) && !envReport.result.keys.includes(injectedSecretName) ? "pass" : "fail", `Worker environment keys: ${(envReport.result?.keys ?? []).join(", ")}`),
  criterion("C-CAPABILITY-VALID", "Valid opaque capability permits brokered read", brokerAllowed.result?.broker?.ok === true && brokerAllowed.result?.broker?.result?.content === "gate-c brokered content\n" ? "pass" : "fail", JSON.stringify(brokerAllowed.result?.broker ?? null)),
  criterion("C-CAPABILITY-INVALID", "Invalid capability is denied", brokerInvalid.result?.broker?.ok === false && brokerInvalid.result?.broker?.error?.code === "CAPABILITY_DENIED" ? "pass" : "fail", JSON.stringify(brokerInvalid.result?.broker ?? null)),
  criterion("C-PATH-ESCAPE", "Parent path traversal is denied", traversal.result?.broker?.error?.code === "CAP_PATH_ESCAPE" ? "pass" : "fail", JSON.stringify(traversal.result?.broker ?? null)),
  criterion("C-SYMLINK-STATIC", "Static symlink escape is denied", symlinkEscape.result?.broker?.error?.code === "CAP_SYMLINK" ? "pass" : "fail", JSON.stringify(symlinkEscape.result?.broker ?? null)),
  criterion("C-SYMLINK-RACE", "Adversarial symlink-swap race is denied", raw.broker_race.status === 0 && race.escaped === 0 ? "pass" : "fail", JSON.stringify(race)),
  criterion("C-NATIVE-BROKER-CONTRACT", "Native root identity, symlink, traversal, FIFO and byte limits", nativeResults.every((test) => test.passed) ? "pass" : "fail", JSON.stringify(nativeResults)),
  criterion("C-RUNTIME-PIN", "Pinned local runtime bundle", "pass", `Copied reviewed ${runtimeManifest.version} ${runtimeManifest.os}/${runtimeManifest.arch} runtime into build/runtime/node; SHA256=${runtimeManifest.sha256}; dependency audit retained. Signing/universal/Linux distribution remains unproven.`),
  criterion("C-APP-SANDBOX-SUBSET", "Supported App Sandbox native file/network subset", appSandboxReport?.decision === "filesystem-and-network-subset-proven" ? "pass" : "untested", JSON.stringify(appSandboxReport?.result ?? null), "This subset is not TS/XPC integration; child-process denial and memory remain unproven."),
  criterion("C-AMBIENT-FS", "Ambient filesystem read is denied", ambientFile.result?.denied === true && !JSON.stringify(ambientFile).includes("PROJECTOS_GATE_C_AMBIENT_SECRET_MUST_NOT_BE_READ") ? "pass" : "fail", JSON.stringify(ambientFile.result ?? null)),
  criterion("C-DNS", "DNS egress is denied", networkPayload.dns?.denied === true ? "pass" : "fail", JSON.stringify(networkPayload.dns ?? null)),
  criterion("C-IPV4", "Direct IPv4 egress is denied by policy", deniedCode(networkPayload.ipv4) ? "pass" : "fail", JSON.stringify(networkPayload.ipv4 ?? null)),
  criterion("C-IPV6", "Direct IPv6 egress is denied by policy", deniedCode(networkPayload.ipv6) ? "pass" : "fail", JSON.stringify(networkPayload.ipv6 ?? null)),
  criterion("C-LOCALHOST4", "IPv4 localhost egress is denied", deniedCode(networkPayload.localhost4) ? "pass" : "fail", JSON.stringify(networkPayload.localhost4 ?? null)),
  criterion("C-LOCALHOST6", "IPv6 localhost egress is denied", deniedCode(networkPayload.localhost6) ? "pass" : "fail", JSON.stringify(networkPayload.localhost6 ?? null)),
  criterion("C-REDIRECT", "Redirects cannot widen egress", deniedCode(redirect.result?.firstHop) && redirect.result?.redirectFollowed === false ? "pass" : "fail", JSON.stringify(redirect.result ?? null)),
  criterion("C-CHILD-SPAWN", "Ungoverned child-process spawning is denied", child.result?.denied === true && deniedCode(child.result) ? "pass" : "fail", JSON.stringify(child.result ?? null)),
  criterion("C-WALL-CANCEL", "Wall-time cancellation terminates worker", cancelled.reason === "wall-time-ceiling" ? "pass" : "fail", `status=${cancelled.status}; reason=${cancelled.reason}; elapsedMs=${cancelled.elapsedMs}`),
  criterion("C-CPU-CEILING", "CPU-time ceiling terminates worker", cpu.helloSeen === true && cpu.reason === "worker-exit" && [137, 152].includes(cpu.code) && cpu.elapsedMs < 5000 ? "pass" : "fail", `status=${cpu.status}; reason=${cpu.reason}; code=${cpu.code}; elapsedMs=${cpu.elapsedMs}`),
  criterion("C-PARENT-CRASH", "Parent crash does not orphan worker", orphan.started && orphan.workerAlive === false ? "pass" : "fail", JSON.stringify(orphan)),
  criterion("C-TREE-CANCEL", "Cancellation kills full controlled process tree", treeCancel.started && treeCancel.controlledChildStarted && !treeCancel.workerAlive && treeCancel.childStates.every((entry) => !entry.alive) ? "pass" : "fail", JSON.stringify(treeCancel)),
  criterion("C-BACKPRESSURE", "Output cap terminates flooding worker", flood.reason === "output-backpressure" ? "pass" : "fail", `status=${flood.status}; reason=${flood.reason}; elapsedMs=${flood.elapsedMs}`),
  criterion("C-MEMORY-CEILING", "OS-enforced memory ceiling", "fail", envReport.stderr?.includes("setrlimit RLIMIT_AS: Invalid argument") ? "Darwin rejected setrlimit(RLIMIT_AS) with EINVAL; the spike has no proven OS-enforced memory ceiling." : "No successful OS memory-limit proof was recorded.", "A supported macOS memory-confinement mechanism must be selected and proven; V8 heap flags alone are not an OS authority boundary."),
  criterion("C-MACOS-PRODUCTION-CONFINEMENT", "Supported production macOS confinement", "fail", "sandbox-exec is deprecated. A signed App Sandbox app-bundle native helper now proves file/localhost denial, but fork and /bin/sh execution succeed (exit 42). No TS/XPC production integration is proven.", "Minimal App Sandbox entitlements do not deny child execution; prove an additional runtime boundary and scoped broker IPC without weakening the locked gate."),
  criterion("C-LINUX-CONFINEMENT", "Linux confinement and packaging", "untested", "No Linux host was available; bubblewrap/namespace/cgroup/seccomp behavior was not tested.", "Freeze and execute the required Linux matrix."),
  criterion("C-CU-TARGET", "Computer use: explicit target identity", "untested", "Computer-use worker is a separate hard gate and is not implemented in this spike."),
  criterion("C-CU-CONSENT-STOP", "Computer use: visible consent and emergency stop", "untested", "Computer-use worker is a separate hard gate and is not implemented in this spike."),
  criterion("C-CU-CAPTURE", "Computer use: scoped/redacted capture", "untested", "Computer-use worker is a separate hard gate and is not implemented in this spike."),
  criterion("C-CU-IDENTITY-LOSS", "Computer use: pause on target identity loss", "untested", "Computer-use worker is a separate hard gate and is not implemented in this spike."),
  criterion("C-CU-X11", "Computer use: Linux X11 matrix", "untested", "No Linux/X11 host or portal matrix was available."),
  criterion("C-CU-WAYLAND", "Computer use: Linux Wayland matrix", "untested", "No Linux/Wayland compositor or portal matrix was available."),
  criterion("C-CU-PERMISSIONS", "Computer use: permission denial and revocation", "untested", "Accessibility and screen-capture denial/revocation were not exercised."),
  criterion("C-CU-CREDENTIALS", "Computer use: allowlisted credential-exclusion profile", "untested", "No allowlisted test application/profile was configured.")
];

const hashes = {
  nodeRuntimeSha256: await sha256(nodeReal),
  workerSha256: await sha256(worker),
  protocolSha256: await sha256(protocol),
  supervisorSha256: await sha256(supervisor),
  guardianSourceSha256: await sha256(guardianSource),
  guardianBinarySha256: await sha256(guardian)
  ,nativeBrokerSourceSha256: await sha256(join(here, "file-broker.c")),
  nativeBrokerBinarySha256: await sha256(join(build, "file-broker")),
  brokerRaceSourceSha256: await sha256(join(here, "broker-race-test.c")),
  runnerSha256: await sha256(join(here, "run-tests.mjs")),
  runtimePinSha256: await sha256(join(here, "runtime-pin.json"))
};
const counts = Object.fromEntries(["pass", "fail", "untested"].map((status) => [status, criteria.filter((entry) => entry.status === status).length]));
const report = {
  schemaVersion: 1,
  gate: "C",
  milestone: "M0",
  generatedAt: new Date().toISOString(),
  decision: "fail",
  recommendedRoadmapStatus: "blocked",
  decisionBasis: [
    "Gate C requires a worker that ships on both macOS and Linux; Linux is untested.",
    "A pinned local runtime bundle and native descriptor-relative broker are implemented; universal/signing/distribution proof remains incomplete.",
    "The current macOS enforcement probe uses sandbox-exec, which the installed manual marks deprecated and unsuitable as the selected production boundary.",
    "Darwin rejected RLIMIT_AS, so this spike does not prove an enforceable OS memory ceiling.",
    "Computer-use isolation remains separately untested."
  ],
  platform,
  hashes,
  sourceHashes: Object.fromEntries(await Promise.all([
    "spikes/gate-c/worker.ts", "spikes/gate-c/protocol.schema.json", "spikes/gate-c/supervisor.mjs",
    "spikes/gate-c/guardian.c", "spikes/gate-c/file-broker.c", "spikes/gate-c/broker-race-test.c",
    "spikes/gate-c/runtime-pin.json", "spikes/gate-c/run-tests.mjs", "spikes/gate-c/README.md",
    "spikes/gate-c/app-sandbox-probe.c", "spikes/gate-c/app-sandbox.entitlements.plist",
    "spikes/gate-c/app-sandbox.Info.plist", "spikes/gate-c/run-app-sandbox.mjs", "docs/adr-worker-isolation.md"
  ].map(async (path) => [path, await sha256(join(root, path))]))),
  counts,
  criteria,
  artifacts: {
    protocol: "spikes/gate-c/protocol.schema.json",
    worker: "spikes/gate-c/worker.ts",
    supervisor: "spikes/gate-c/supervisor.mjs",
    guardian: "spikes/gate-c/guardian.c",
    testRunner: "spikes/gate-c/run-tests.mjs",
    rawResults: "evidence/gates/gate-c/raw-results.json"
  }
};

await writeFile(join(evidence, "raw-results.json"), `${JSON.stringify(raw, null, 2)}\n`);
await writeFile(join(evidence, "report.json"), `${JSON.stringify(report, null, 2)}\n`);
process.stdout.write(`${JSON.stringify({ decision: report.decision, recommendedRoadmapStatus: report.recommendedRoadmapStatus, counts, report: join(evidence, "report.json") }, null, 2)}\n`);
