import { fork, spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { readdir, readFile } from "node:fs/promises";
import path from "node:path";
import { monitorEventLoopDelay } from "node:perf_hooks";
import { fileURLToPath } from "node:url";
import { runBoundedCommand, terminateTree } from "./run-with-deadline.mjs";

const SELF = fileURLToPath(import.meta.url);
const monotonicMs = () => Number(process.hrtime.bigint()) / 1e6;

function decodedError(value) {
  const error = new Error(value.message);
  if (value.code !== undefined) error.code = value.code;
  if (value.errors) return new AggregateError(value.errors.map(decodedError), value.message);
  return error;
}

function encodedError(error) {
  return { message: error.message, code: error.code,
    ...(error instanceof AggregateError ? { errors: error.errors.map(encodedError) } : {}) };
}

function birthIdentity(pid) {
  if (process.platform !== "linux") return undefined;
  try {
    const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
    return stat.slice(stat.lastIndexOf(")") + 2).split(" ")[19];
  } catch (error) {
    if (error.code === "ENOENT" || error.code === "ESRCH") return undefined;
    throw error;
  }
}

export function earliestCleanupDeadline(batchDeadline, failedCase, activeCases) {
  return Math.min(batchDeadline, failedCase === undefined ? Infinity : failedCase.total, ...Array.from(activeCases, (active) => active.total));
}

export async function waitForLauncherClose(closed, deadline) {
  let timer;
  try {
    await Promise.race([closed, new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error("launcher close remains unconfirmed within total deadline")), Math.max(0, deadline - monotonicMs()));
    })]);
    if (monotonicMs() >= deadline) throw new Error("launcher close remains unconfirmed within total deadline");
  } finally { clearTimeout(timer); }
}

async function stopDomain(domain, deadline) {
  if (domain === undefined) return;
  if (process.platform === "linux") {
    const current = birthIdentity(domain.pid);
    if (current === undefined) {
      try { process.kill(-domain.pid, 0); }
      catch (error) { if (error.code === "ESRCH") return; throw error; }
    }
    if (current !== undefined && current !== domain.birth) throw new Error(`launcher PID identity changed: ${domain.pid}`);
  }
  await terminateTree({ pid: domain.pid }, true, deadline, monotonicMs);
}

// 同一轮完成风暴只扫描一次 /proc，避免监督 loop 被逐 case 全表扫描堵塞。
export function createCompletionCleanup(launcherPid, {
  listProcesses = () => readdir("/proc"),
  readEnvironment = (pid) => readFile(`/proc/${pid}/environ`),
  readProcessStat = (pid) => readFile(`/proc/${pid}/stat`, "utf8"),
  terminate = terminateTree,
} = {}) {
  async function identity(pid) {
    const stat = await readProcessStat(pid);
    const fields = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
    if (fields.length < 20 || !/^\d+$/.test(fields[19])) throw new Error(`cannot confirm process ${pid}: malformed stat identity`);
    return { state: fields[0], ppid: Number(fields[1]), group: Number(fields[2]), birth: fields[19] };
  }
  function tokenItem(environment, byToken) {
    const prefix = "STOCK_GAME_RUST_CASE_TOKEN=";
    const tokenEntry = environment.toString().split("\0").find((value) => value.startsWith(prefix));
    return tokenEntry === undefined ? undefined : byToken.get(tokenEntry.slice(prefix.length));
  }
  let queued = [];
  let scheduled = false;
  async function flush() {
    const batch = queued;
    queued = [];
    const byToken = new Map(batch.map((item) => [item.record.token, item]));
    try {
      if (process.platform === "linux") {
        const entries = (await listProcesses()).filter((entry) => /^\d+$/.test(entry));
        const owned = [];
        await Promise.all(entries.map(async (entry) => {
          let before;
          let candidate = false;
          try {
            // 首次 environ 只筛候选；归属必须由同 birth 内重读的 environ 确认。
            if (tokenItem(await readEnvironment(Number(entry)), byToken) === undefined) return;
            candidate = true;
            before = await identity(Number(entry));
            if (before.state === "Z" || before.state === "X") return;
            const item = tokenItem(await readEnvironment(Number(entry)), byToken);
            if (item === undefined) return;
            const after = await identity(Number(entry));
            if (before.birth !== after.birth) throw new Error(`case PID identity changed during environment read: ${entry}`);
            if (after.state === "Z" || after.state === "X") return;
            owned.push({ item, pid: Number(entry), birth: before.birth });
          } catch (error) {
            if (["ENOENT", "ESRCH"].includes(error.code)) return;
            if (["EACCES", "EPERM"].includes(error.code)) {
              let current;
              try { current = await identity(Number(entry)); }
              catch (statError) { if (["ENOENT", "ESRCH"].includes(statError.code)) return; throw statError; }
              // 僵尸已无可执行实体；退出瞬间 environ 也可能拒绝访问，不等同活进程权限未知。
              if (current.state === "Z" || current.state === "X") return;
              if (!candidate && current.ppid !== launcherPid && current.group !== launcherPid && before?.ppid !== launcherPid && before?.group !== launcherPid) return;
              throw new Error(`cannot confirm owned process ${entry} (state=${current.state}, ppid=${current.ppid}, pgid=${current.group}, starttime=${current.birth}): ${error.message}`, { cause: error });
            }
            throw error;
          }
        }));
        await Promise.all(owned.map(async ({ item, pid, birth }) => {
          let current;
          try { current = await identity(pid); }
          catch (error) { if (["ENOENT", "ESRCH"].includes(error.code)) return; throw error; }
          if (current.birth !== birth) throw new Error(`case PID identity changed: ${pid}`);
          if (current.state === "Z" || current.state === "X") return;
          await terminate({ pid }, false, item.record.total, monotonicMs);
        }));
      }
      for (const item of batch) item.resolve();
    } catch (error) { for (const item of batch) item.reject(error); }
    scheduled = false;
    if (queued.length > 0) { scheduled = true; setImmediate(() => { void flush(); }); }
  }
  return (record) => new Promise((resolve, reject) => {
    if (record.pid === undefined) { reject(new Error(`case ${record.context} finished without a spawned PID`)); return; }
    queued.push({ record, resolve, reject });
    if (!scheduled) { scheduled = true; setImmediate(() => { void flush(); }); }
  });
}

export async function executeSupervisedRustTestBinaries({ artifacts, policy, cwd, env, remainingMs, log = () => undefined, launcherPath = SELF, terminateDomain = stopDomain }) {
  const timeoutMs = Math.floor(remainingMs("Rust supervisor preparation"));
  const batchDeadline = monotonicMs() + timeoutMs;
  let response;
  let domain;
  let supervisor;
  let failure;
  try {
    await runBoundedCommand({ command: process.execPath, args: [SELF, "--watchdog"], cwd, env,
      timeoutMs, cleanupReserveMs: Math.min(1000, timeoutMs - 1),
      spawnProcess: (command, args, options) => {
        supervisor = spawn(command, args, { ...options, stdio: ["ignore", "inherit", "inherit", "ipc"] });
        supervisor.on("message", (message) => {
          if (message.kind === "domain") domain = message.domain;
          else if (message.kind === "log") log(JSON.stringify(message.value));
          else if (message.kind === "result" || message.kind === "failure") response = message;
        });
        supervisor.once("spawn", () => supervisor.send({ kind: "initialize", artifacts, policy, cwd, batchDeadline, launcherPath }));
        return supervisor;
      },
    });
  } catch (error) { failure = error; }
  if (response?.kind === "failure") {
    const original = decodedError(response.error);
    failure = failure === undefined ? original : new AggregateError([original, failure], `${original.message}; supervisor failed: ${failure.message}`);
  }
  if (failure === undefined && response?.kind !== "result") failure = new Error("Rust independent supervisor exited without a confirmed result");
  try { if (response?.domain_closed !== true) await waitForLauncherClose(terminateDomain(domain, batchDeadline), batchDeadline); }
  catch (error) { failure = failure === undefined ? error : new AggregateError([failure, error], `${failure.message}; supervisor cleanup failed: ${error.message}`); }
  if (failure !== undefined) throw failure;
  return response.value;
}

async function supervise(input) {
  const records = new Map();
  const loopDelay = monitorEventLoopDelay({ resolution: 10 });
  loopDelay.enable();
  let launcher;
  let domain;
  let terminal;
  let stopping;
  let completed;
  let cleanupCompletedCase;
  let launcherClosedResolve;
  let domainCleaned = false;
  const launcherClosed = new Promise((resolve) => { launcherClosedResolve = resolve; });
  const send = (message) => { if (process.connected) process.send(message); };
  const log = (value) => send({ kind: "log", value });
  const clearRecord = (record) => { clearTimeout(record.executionTimer); clearTimeout(record.totalTimer); };

  async function fail(error, record) {
    if (stopping !== undefined) return stopping;
    terminal = error;
    for (const active of records.values()) clearTimeout(active.executionTimer);
    const deadline = earliestCleanupDeadline(input.batchDeadline, record, records.values());
    stopping = (async () => {
      try {
        await waitForLauncherClose(Promise.all([stopDomain(domain, deadline), launcherClosed]), deadline);
        domainCleaned = true;
        if (record) log({ phase: "rust-case-termination-confirmed", token: record.token, context: record.context,
          confirmed_ms: monotonicMs(), total_deadline_ms: record.total, event_loop_delay_max_ms: loopDelay.max / 1e6 });
      } catch (cleanupError) {
        terminal = new AggregateError([terminal, cleanupError], `${terminal.message}; process tree termination remains unconfirmed: ${cleanupError.message}`);
      }
      for (const active of records.values()) clearRecord(active);
      send({ kind: "failure", domain_closed: domainCleaned, error: encodedError(terminal) });
      process.exitCode = 1;
      if (process.connected) process.disconnect();
    })();
    return stopping;
  }

  function expire(record) {
    record.expired = true;
    return fail(new Error(`${record.context} failed: bounded command exhausted its execution budget within the total ${record.timeoutMs}ms deadline`), record);
  }

  async function complete(message) {
    const record = records.get(message.token);
    if (record === undefined || record.expired || terminal !== undefined) return;
    if (message.error !== undefined) {
      await fail(new Error(`${record.context} failed: ${message.error.message}`, { cause: decodedError(message.error) }), record);
      return;
    }
    if (monotonicMs() >= record.execution) { await expire(record); return; }
    clearTimeout(record.executionTimer);
    record.finishing = true;
    try {
      await cleanupCompletedCase(record);
      if (monotonicMs() >= record.total) throw new Error(`${record.context} cleanup exceeded total deadline`);
      if (terminal !== undefined) return;
      clearRecord(record);
      records.delete(record.token);
      log({ phase: "rust-case-complete", context: record.context, token: record.token, pid: record.pid,
        wall_ms: monotonicMs() - record.started, total_deadline_ms: record.total });
      launcher.send({ kind: "completed", token: record.token });
    } catch (error) { await fail(error, record); }
  }

  launcher = fork(input.launcherPath, ["--launcher"], { cwd: input.cwd, env: process.env,
    detached: process.platform !== "win32", stdio: ["ignore", "inherit", "inherit", "ipc"] });
  domain = { pid: launcher.pid, birth: birthIdentity(launcher.pid) };
  cleanupCompletedCase = createCompletionCleanup(launcher.pid);
  send({ kind: "domain", domain });
  log({ phase: "rust-supervisor-domain", supervisor_pid: process.pid, launcher_pid: launcher.pid });
  const batchTimer = setTimeout(() => {
    const earliest = [...records.values()].sort((left, right) => left.execution - right.execution)[0];
    if (earliest !== undefined) void expire(earliest);
    else void fail(new Error("Rust supervisor exhausted the shared execute deadline"));
  }, Math.max(0, input.batchDeadline - monotonicMs() - 1000));
  launcher.on("error", (error) => { void fail(error); });
  launcher.on("close", (code, signal) => {
    launcherClosedResolve();
    if ((completed === undefined || code !== 0) && terminal === undefined) void fail(new Error(`Rust launcher unexpectedly closed with ${code ?? signal}`));
  });
  launcher.on("message", (message) => {
    if (terminal !== undefined) return;
    if (message.kind === "ready") {
      launcher.send({ kind: "initialize", artifacts: input.artifacts, policy: input.policy, cwd: input.cwd, batchDeadline: input.batchDeadline });
    } else if (message.kind === "arm") {
      if (typeof message.token !== "string" || !message.token.startsWith(`${launcher.pid}:`) || records.has(message.token) ||
          ![message.started, message.execution, message.total, message.timeoutMs].every(Number.isFinite) ||
          message.timeoutMs <= 0 || message.timeoutMs > 10000 || message.started >= message.execution ||
          message.execution >= message.total || message.total > input.batchDeadline || message.total - message.started > message.timeoutMs + 1) {
        void fail(new Error("Rust launcher returned an invalid or duplicate arm token")); return;
      }
      const record = { ...message };
      records.set(record.token, record);
      record.executionTimer = setTimeout(() => { void expire(record); }, Math.max(0, record.execution - monotonicMs()));
      record.totalTimer = setTimeout(() => { void fail(new Error(`${record.context} exceeded total deadline; termination remains unconfirmed`), record); }, Math.max(0, record.total - monotonicMs()));
      if (monotonicMs() >= record.execution) { void expire(record); return; }
      launcher.send({ kind: "armed", token: record.token });
    } else if (message.kind === "spawned") {
      const record = records.get(message.token);
      if (record !== undefined && record.pid === undefined && Number.isSafeInteger(message.pid) && message.pid > 0) {
        record.pid = message.pid;
        log({ phase: "rust-case-spawned", token: record.token, context: record.context, pid: message.pid, spawned_ms: monotonicMs(), spawn_wall_ms: message.spawn_wall_ms });
      }
    } else if (message.kind === "finished") {
      void complete(message);
    } else if (message.kind === "log") log(message.value);
    else if (message.kind === "result") {
      if (records.size !== 0) { void fail(new Error("Rust launcher returned a result with unconfirmed active cases")); return; }
      completed = message.value;
      launcher.send({ kind: "shutdown" });
      void launcherClosed.then(async () => {
        if (terminal !== undefined) return;
        clearTimeout(batchTimer);
        loopDelay.disable();
        log({ phase: "rust-supervisor-complete", memory: process.memoryUsage(), event_loop_delay_max_ms: loopDelay.max / 1e6 });
        send({ kind: "result", domain_closed: true, value: completed });
        if (process.connected) process.disconnect();
      }).catch((error) => { void fail(error); });
    } else if (message.kind === "failure") void fail(decodedError(message.error));
    else void fail(new Error(`Rust launcher returned an unknown protocol message: ${message.kind}`));
  });
  process.on("disconnect", () => { if (completed === undefined && terminal === undefined) void fail(new Error("Rust supervisor lost its parent")); });
  await launcherClosed;
  clearTimeout(batchTimer);
  if (stopping !== undefined) await stopping;
}

async function launch() {
  const requests = new Map();
  let nextToken = 0;
  let initialized = false;
  const request = (kind, token, data) => new Promise((resolve, reject) => {
    requests.set(`${kind}:${token}`, { resolve, reject });
    process.send({ kind: kind === "armed" ? "arm" : "finished", token, ...data });
  });
  process.on("disconnect", () => {
    for (const pending of requests.values()) pending.reject(new Error("Rust launcher lost independent watchdog"));
  });
  process.on("message", async (message) => {
    if (message.kind === "armed" || message.kind === "completed") {
      const key = `${message.kind}:${message.token}`;
      const pending = requests.get(key);
      if (pending === undefined) { process.send({ kind: "failure", error: encodedError(new Error("unexpected Rust watchdog token")) }); return; }
      requests.delete(key); pending.resolve(); return;
    }
    if (message.kind === "shutdown") { if (process.connected) process.disconnect(); return; }
    if (message.kind !== "initialize" || initialized) return;
    initialized = true;
    try {
      process.send({ kind: "log", value: { phase: "rust-launcher-ready", memory: process.memoryUsage() } });
      const { executeRustTestBinaries } = await import("./run-full-regression.mjs");
      const labels = new Map(message.artifacts.map((artifact) => [artifact.executable, artifact.label]));
      const value = await executeRustTestBinaries({ artifacts: message.artifacts, policy: message.policy, cwd: message.cwd,
        env: process.env, now: monotonicMs, remainingMs: () => Math.floor(message.batchDeadline - monotonicMs()),
        log: (line) => process.send({ kind: "log", value: JSON.parse(line) }),
        run: async (options) => {
          const token = `${process.pid}:${++nextToken}`;
          const started = monotonicMs();
          const total = Math.min(message.batchDeadline, started + options.timeoutMs);
          const execution = total - options.cleanupReserveMs;
          const context = options.args.includes("--list") ? `${labels.get(options.command)} listing` : `${labels.get(options.command)} case ${options.args[0]}`;
          await request("armed", token, { started, total, execution, context, timeoutMs: options.timeoutMs });
          let output;
          let error;
          try {
            const remaining = Math.floor(total - monotonicMs());
            if (remaining <= options.cleanupReserveMs) throw new Error(`${context} exhausted deadline before spawn`);
            output = await runBoundedCommand({ ...options, timeoutMs: remaining, captureOutput: true,
              onStdout: options.captureOutput ? options.onStdout : (chunk) => process.stdout.write(chunk),
              onStderr: options.captureOutput ? options.onStderr : (chunk) => process.stderr.write(chunk),
              env: { ...options.env, STOCK_GAME_RUST_CASE_TOKEN: token },
              spawnProcess: (command, args, spawnOptions) => {
                const spawnStarted = monotonicMs();
                const child = spawn(command, args, { ...spawnOptions, detached: false });
                process.send({ kind: "spawned", token, pid: child.pid, spawn_wall_ms: monotonicMs() - spawnStarted });
                return child;
              },
            });
          } catch (failure) { error = failure; }
          await request("completed", token, { ...(error === undefined ? {} : { error: encodedError(error) }) });
          if (error !== undefined) throw error;
          return output;
        },
      });
      process.send({ kind: "result", value });
    } catch (error) {
      if (process.connected) process.send({ kind: "failure", error: encodedError(error) });
      else { console.error(error.message); process.exitCode = 1; }
    }
  });
  process.send({ kind: "ready" });
}

if (process.argv[1] && path.resolve(process.argv[1]) === SELF) {
  if (process.argv[2] === "--launcher") await launch();
  else if (process.argv[2] === "--watchdog") {
    process.once("message", (input) => {
      supervise(input).catch((error) => {
        if (process.connected) { process.send({ kind: "failure", error: encodedError(error) }); process.disconnect(); }
        console.error(error.message); process.exitCode = 1;
      });
    });
  } else throw new Error("Rust case supervisor requires its internal process mode");
}
