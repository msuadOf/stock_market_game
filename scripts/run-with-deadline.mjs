#!/usr/bin/env node
import { ChildProcess, spawn, spawnSync } from "node:child_process";
import { readdir, readFile } from "node:fs/promises";
import path from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";

export const ORDINARY_TEST_MAX_MS = 10_000;
export const LONG_VALIDATION_MAX_MS = 300_000;
export const COMMAND_CLEANUP_RESERVE_MAX_MS = 1_000;

function cleanupDeadline(deadline) {
  return typeof deadline === "function" ? deadline() : deadline;
}

function checkCleanupBudget(deadline, now) {
  if (now() >= cleanupDeadline(deadline)) throw new Error("owned process tree exit was not confirmed within the cleanup deadline");
}

async function processIdentity(pid, deadline, now) {
  checkCleanupBudget(deadline, now);
  try {
    const stat = await readFile(`/proc/${pid}/stat`, "utf8");
    checkCleanupBudget(deadline, now);
    const fields = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
    if (fields.length < 20 || !/^\d+$/.test(fields[19])) throw new Error(`cannot confirm process ${pid}: malformed stat identity`);
    return { pid, ppid: Number(fields[1]), group: Number(fields[2]), state: fields[0], birth: fields[19] };
  } catch (error) {
    if (error.code === "ENOENT" || error.code === "ESRCH") return undefined;
    throw error;
  }
}

async function inBatches(values, action, deadline, now) {
  checkCleanupBudget(deadline, now);
  const output = [];
  for (let offset = 0; offset < values.length; offset += 64) {
    checkCleanupBudget(deadline, now);
    output.push(...await Promise.all(values.slice(offset, offset + 64).map(action)));
    checkCleanupBudget(deadline, now);
    await new Promise((resolve) => setImmediate(resolve));
  }
  checkCleanupBudget(deadline, now);
  return output;
}

async function tracedChildren(root, deadline, now) {
  const known = new Map();
  const first = await processIdentity(root.pid, deadline, now);
  if (first === undefined || first.state === "Z" || first.state === "X") return [];
  if (root.birth !== undefined && first.birth !== root.birth) throw new Error(`owned process PID identity changed: ${root.pid}`);
  known.set(first.pid, first);
  const queue = [first];
  for (let cursor = 0; cursor < queue.length; cursor += 1) {
    const parent = queue[cursor];
    checkCleanupBudget(deadline, now);
    let tids;
    try { tids = await readdir(`/proc/${parent.pid}/task`); }
    catch (error) { if (error.code === "ENOENT" || error.code === "ESRCH") continue; throw error; }
    const lists = await inBatches(tids, async (tid) => {
      checkCleanupBudget(deadline, now);
      try { return await readFile(`/proc/${parent.pid}/task/${tid}/children`, "utf8"); }
      catch (error) { if (error.code === "ENOENT" || error.code === "ESRCH") return ""; throw error; }
    }, deadline, now);
    const children = [...new Set(lists.flatMap((value) => value.trim().length === 0 ? [] : value.trim().split(/\s+/).map(Number)))];
    const identities = await inBatches(children, (pid) => processIdentity(pid, deadline, now), deadline, now);
    const currentParent = await processIdentity(parent.pid, deadline, now);
    if (currentParent === undefined && identities.some((entry) => entry !== undefined && !known.has(entry.pid))) throw new Error(`owned parent identity disappeared: ${parent.pid}`);
    if (currentParent !== undefined && currentParent.birth !== parent.birth) throw new Error(`owned process PID identity changed: ${parent.pid}`);
    for (const entry of identities) {
      if (entry === undefined || known.has(entry.pid)) continue;
      if (entry.ppid !== parent.pid) throw new Error(`owned child identity changed while tracing parent ${parent.pid}: ${entry.pid}`);
      known.set(entry.pid, entry); queue.push(entry);
    }
  }
  return [...known.values()];
}

async function sameIdentity(entry, deadline, now) {
  const current = await processIdentity(entry.pid, deadline, now);
  if (current !== undefined && current.birth !== entry.birth) throw new Error(`owned process PID identity changed: ${entry.pid}`);
  return current;
}

export async function terminateTree(child, detached = false, deadline = Date.now() + COMMAND_CLEANUP_RESERVE_MAX_MS, now = Date.now, effectiveDeadline) {
  if (child.pid === undefined) return;
  if (effectiveDeadline !== undefined) {
    const initialDeadline = deadline;
    // 仅 builtin cleanup 内部收紧共享界；now 保持真实时钟，原调用和 custom hook 契约不变。
    deadline = () => Math.min(initialDeadline, effectiveDeadline());
  }
  checkCleanupBudget(deadline, now);
  if (process.platform === "linux" && child instanceof ChildProcess) {
    if (child.exitCode !== null || child.signalCode !== null) throw new Error(`owned root identity unavailable after exit: ${child.pid}`);
    const identity = await processIdentity(child.pid, deadline, now);
    if (identity === undefined || child.exitCode !== null || child.signalCode !== null) throw new Error(`owned root identity unavailable during binding: ${child.pid}`);
    child = { pid: child.pid, birth: identity.birth };
  }
  if (process.platform === "linux" && (typeof child.birth !== "string" || !/^\d+$/.test(child.birth))) {
    throw new Error(`owned root identity unavailable without a bound birth: ${child.pid}`);
  }
  if (process.platform === "win32") {
    await new Promise((resolve, reject) => {
      const killer = spawn("taskkill", ["/pid", String(child.pid), "/T", "/F"], {
        windowsHide: true, stdio: "ignore", timeout: Math.max(1, Math.floor(cleanupDeadline(deadline) - now())),
      });
      killer.on("error", reject);
      killer.on("close", (code) => code === 0 ? resolve() : reject(new Error(`taskkill did not confirm process tree termination: exit ${code}`)));
    });
    checkCleanupBudget(deadline, now);
    return;
  }
  // 先保存并终止仍挂在 root 下的子链；全表扫描不能让 detached 叶子等到 root 最后信号后才失去归属。
  const traced = process.platform === "linux" ? await tracedChildren(child, deadline, now) : [];
  await inBatches(traced, (entry) => sameIdentity(entry, deadline, now), deadline, now);
  for (const entry of [...traced].reverse()) {
    if (entry.pid === child.pid) continue;
    const current = await sameIdentity(entry, deadline, now);
    if (current === undefined || current.state === "Z" || current.state === "X") continue;
    checkCleanupBudget(deadline, now);
    try { process.kill(entry.pid, "SIGKILL"); } catch (error) { if (error.code !== "ESRCH") throw error; }
  }
  const processes = process.platform === "linux" ? await linuxProcesses(deadline, now) : posixProcesses(deadline, now);
  const byPid = new Map(traced.map((entry) => [entry.pid, entry]));
  for (const entry of processes) {
    const previous = byPid.get(entry.pid);
    if (previous !== undefined && previous.birth !== entry.birth) throw new Error(`owned process PID identity changed: ${entry.pid}`);
    byPid.set(entry.pid, entry);
  }
  const root = byPid.get(child.pid);
  if (root !== undefined && child.birth !== undefined && root.birth !== child.birth) throw new Error(`owned process PID identity changed: ${child.pid}`);
  const children = new Map();
  await inBatches(processes, async (entry) => {
    const siblings = children.get(entry.ppid);
    if (siblings === undefined) children.set(entry.ppid, [entry.pid]);
    else siblings.push(entry.pid);
  }, deadline, now);
  const descendants = new Set([child.pid, ...traced.map((entry) => entry.pid)]);
  if (detached) {
    const members = processes.filter((entry) => entry.group === child.pid);
    if (process.platform === "linux" && members.length > 0) {
      // PGID 没有 generation；同 birth root（含 Z）或先前已认领的活成员才能授权新 snapshot。
      const currentRoot = await sameIdentity(child, deadline, now);
      let anchored = currentRoot !== undefined;
      for (const entry of traced) {
        if (anchored) break;
        if (entry.group !== child.pid) continue;
        const current = await sameIdentity(entry, deadline, now);
        anchored = current !== undefined && current.state !== "Z" && current.state !== "X" && current.group === child.pid;
      }
      if (!anchored) throw new Error(`owned process group identity unavailable after root exit: ${child.pid}`);
    }
    // 先确认旧 group 身份，再沿 PPID 找 detached 子孙。
    await inBatches(members, async (entry) => { descendants.add(entry.pid); }, deadline, now);
  }
  const queue = [...descendants];
  for (let cursor = 0; cursor < queue.length; cursor += 1) {
    checkCleanupBudget(deadline, now);
    for (const pid of children.get(queue[cursor]) ?? []) {
      if (descendants.has(pid)) continue;
      if (process.platform === "linux") {
        const parent = byPid.get(queue[cursor]);
        const entry = byPid.get(pid);
        if (parent === undefined || entry === undefined) throw new Error(`owned parent identity disappeared: ${queue[cursor]}`);
        const before = await sameIdentity(parent, deadline, now);
        const current = await sameIdentity(entry, deadline, now);
        const after = await sameIdentity(parent, deadline, now);
        if (before === undefined || after === undefined) throw new Error(`owned parent identity disappeared: ${parent.pid}`);
        if (current === undefined) continue;
        if (current.ppid !== parent.pid) throw new Error(`owned child identity changed while tracing parent ${parent.pid}: ${entry.pid}`);
      }
      descendants.add(pid); queue.push(pid);
    }
    if (cursor % 64 === 63) await new Promise((resolve) => setImmediate(resolve));
  }
  const identities = queue.map((pid) => byPid.get(pid)).filter((entry) => entry !== undefined).reverse();
  const currentIdentity = (entry) => sameIdentity(entry, deadline, now);
  // 先核整轮身份，再发信号；不能把 snapshot 的旧 PID 借给新进程。
  if (process.platform === "linux") await inBatches(identities, currentIdentity, deadline, now);
  if (detached) {
    // snapshot 已保存 detached 后代；只有同 birth 的活 group 成员才能授权 group 信号。
    for (const entry of identities.filter((entry) => entry.group === child.pid)) {
      const current = process.platform === "linux" ? await currentIdentity(entry) : entry;
      if (current === undefined || current.state === "Z" || current.state === "X") continue;
      if (current.group !== child.pid) continue;
      checkCleanupBudget(deadline, now);
      try { process.kill(-child.pid, "SIGKILL"); } catch (error) { if (error.code !== "ESRCH") throw error; }
      break;
    }
  }
  for (const entry of identities) {
    checkCleanupBudget(deadline, now);
    const current = process.platform === "linux" ? await currentIdentity(entry) : entry;
    if (current === undefined || current.state === "Z" || current.state === "X") continue;
    checkCleanupBudget(deadline, now);
    try { process.kill(entry.pid, "SIGKILL"); } catch (error) { if (error.code !== "ESRCH") throw error; }
  }
  if (process.platform === "linux") {
    while (true) {
      const current = await inBatches(identities, currentIdentity, deadline, now);
      if (current.every((entry) => entry === undefined || entry.state === "Z" || entry.state === "X")) return;
      checkCleanupBudget(deadline, now);
      await new Promise((resolve) => setTimeout(resolve, Math.min(5, Math.max(1, cleanupDeadline(deadline) - now()))));
    }
  }
}

async function linuxProcesses(deadline, now) {
  checkCleanupBudget(deadline, now);
  const entries = await readdir("/proc");
  checkCleanupBudget(deadline, now);
  return (await inBatches(entries.filter((entry) => /^\d+$/.test(entry)), (entry) => processIdentity(Number(entry), deadline, now), deadline, now))
    .filter((entry) => entry !== undefined);
}

function posixProcesses(deadline, now) {
  checkCleanupBudget(deadline, now);
  const listing = spawnSync("ps", ["-axo", "pid=,ppid=,pgid="], { encoding: "utf8", timeout: Math.max(1, Math.min(100, Math.floor(cleanupDeadline(deadline) - now()))), maxBuffer: 4 * 1024 * 1024 });
  checkCleanupBudget(deadline, now);
  if (listing.error || listing.status !== 0) throw new Error(`cannot enumerate owned process tree: ${listing.error?.message ?? listing.stderr}`);
  return listing.stdout.trim().split("\n").map((line) => {
    const [pid, ppid, group] = line.trim().split(/\s+/).map(Number);
    return { pid, ppid, group };
  });
}

export function runBoundedCommand({ command, args = [], timeoutMs, cwd = process.cwd(), env = process.env, spawnProcess = spawn, terminateProcessTree = terminateTree, captureOutput = false, onStdout, onStderr, signal, cleanupReserveMs = Math.min(COMMAND_CLEANUP_RESERVE_MAX_MS, Math.max(1, Math.floor(timeoutMs / 5))) }) {
  if (typeof command !== "string" || command.length === 0) throw new Error("deadline command must be non-empty");
  if (!Number.isInteger(timeoutMs) || timeoutMs <= 0 || timeoutMs > LONG_VALIDATION_MAX_MS) {
    throw new Error(`bounded command deadline must be within ${LONG_VALIDATION_MAX_MS}ms; got ${timeoutMs}`);
  }
  if (!Number.isInteger(cleanupReserveMs) || cleanupReserveMs <= 0 || cleanupReserveMs >= timeoutMs) {
    throw new Error(`bounded command cleanup reserve must be a positive integer smaller than ${timeoutMs}ms; got ${cleanupReserveMs}`);
  }
  if ((onStdout !== undefined || onStderr !== undefined) && !captureOutput) {
    throw new Error("bounded command output callbacks require captureOutput");
  }
  if (onStdout !== undefined && typeof onStdout !== "function") throw new Error("onStdout must be a function");
  if (onStderr !== undefined && typeof onStderr !== "function") throw new Error("onStderr must be a function");
  if (signal?.aborted) return Promise.reject(signal.reason instanceof Error ? signal.reason : new Error(`bounded command ${command} was aborted before start`));
  if (typeof terminateProcessTree !== "function") throw new Error("terminateProcessTree must be a function");
  return new BoundedCommandRun({ command, args, timeoutMs, cwd, env, spawnProcess, terminateProcessTree, captureOutput, onStdout, onStderr, signal, cleanupReserveMs }).start();
}

class BoundedCommandRun {
  #options;
  #child;
  #stdout = "";
  #stderr = "";
  #outputCallbackError;
  #timedOut = false;
  #abortReason;
  #settled = false;
  #executionTimer;
  #hardTimer;
  #finalSignalTimer;
  #abortListener;
  #detached;
  #termination;
  #terminationError;
  #deadline;
  #executionDeadline;
  #recordedExit;
  #rootIdentity;
  #resolve;
  #reject;

  constructor(options) {
    this.#options = options;
    this.#abortListener = () => this.#abort();
  }

  start() {
    return new Promise((resolve, reject) => {
      this.#resolve = resolve;
      this.#reject = reject;
      const { command, args, cwd, env, spawnProcess, captureOutput, timeoutMs, cleanupReserveMs, signal } = this.#options;
      this.#deadline = performance.now() + timeoutMs;
      this.#executionDeadline = this.#deadline - cleanupReserveMs;
      this.#detached = process.platform !== "win32" && env.STOCK_GAME_DEADLINE_GROUP !== "owned";
      this.#child = spawnProcess(command, args, {
        cwd,
        env: { ...env, STOCK_GAME_DEADLINE_GROUP: "owned" },
        stdio: captureOutput ? ["ignore", "pipe", "pipe"] : "inherit",
        windowsHide: true,
        detached: this.#detached,
      });
      if (process.platform === "linux" && this.#child instanceof ChildProcess && this.#child.pid !== undefined) {
        // 活 handle 时预先绑定 birth；退出后不能把裸 PID 的新一代认作 owned。
        this.#rootIdentity = processIdentity(this.#child.pid, this.#deadline, () => performance.now())
          .then((identity) => ({ identity: this.#child.exitCode === null && this.#child.signalCode === null ? identity : undefined }),
            (error) => ({ error }));
      }
      if (captureOutput) {
        this.#child.stdout?.on("data", (chunk) => this.#onOutput("stdout", chunk));
        this.#child.stderr?.on("data", (chunk) => this.#onOutput("stderr", chunk));
      }
      this.#executionTimer = setTimeout(() => {
        this.#timedOut = true;
        this.#terminate();
      }, Math.max(0, this.#executionDeadline - performance.now()));
      const finalSignalReserve = Math.max(1, Math.min(100, Math.floor(cleanupReserveMs / 2)));
      const finalReturnReserve = Math.max(1, Math.min(50, Math.floor(cleanupReserveMs / 4)));
      this.#finalSignalTimer = setTimeout(() => {
        if (performance.now() >= this.#deadline) {
          this.#recordTerminationError(new Error("final root signal was not started after its cleanup deadline"));
          return;
        }
        // 只使用仍活的真实 ChildProcess handle；不拿释放后的裸 PID 或 inherited PGID 兜底。
        if (this.#child instanceof ChildProcess && this.#child.exitCode === null && this.#child.signalCode === null) {
          try { this.#child.kill("SIGKILL"); }
          catch (error) { this.#recordTerminationError(error); }
        }
      }, Math.max(0, this.#deadline - finalSignalReserve - performance.now()));
      this.#hardTimer = setTimeout(() => this.#settleOnce(() => {
        const cutoff = new Error(`bounded command reached its final cleanup cutoff within the total ${timeoutMs}ms deadline; process tree termination remains unconfirmed; close did not settle within the ${cleanupReserveMs}ms cleanup reserve`);
        const original = this.#commandFailure(this.#recordedExit?.code, this.#recordedExit?.signal, false);
        reject(original === undefined ? cutoff : new AggregateError([original, cutoff], `${original.message}; ${cutoff.message}`));
      }), Math.max(0, this.#deadline - finalReturnReserve - performance.now()));
      signal?.addEventListener("abort", this.#abortListener, { once: true });
      if (signal?.aborted) this.#abort();
      this.#child.on("error", (error) => this.#onError(error));
      this.#child.on("exit", (code, signal) => { this.#recordedExit = { code, signal }; });
      this.#child.on("close", (code, signal) => this.#onClose(code, signal));
    });
  }

  #onOutput(stream, chunk) {
    if (stream === "stdout") this.#stdout += chunk;
    else this.#stderr += chunk;
    const callback = stream === "stdout" ? this.#options.onStdout : this.#options.onStderr;
    if (callback === undefined || this.#outputCallbackError !== undefined) return;
    try {
      callback(chunk);
    } catch (error) {
      this.#outputCallbackError = new Error(`bounded command ${stream} callback failed: ${error instanceof Error ? error.message : String(error)}`, { cause: error });
      this.#terminate();
    }
  }

  #abort() {
    const { command, signal } = this.#options;
    this.#abortReason = signal.reason instanceof Error ? signal.reason : new Error(`bounded command ${command} was aborted`);
    this.#terminate();
  }

  #terminate() {
    if (this.#termination !== undefined) return;
    this.#termination = Promise.resolve().then(async () => {
      if (this.#options.terminateProcessTree === terminateTree) {
        if (this.#rootIdentity !== undefined) {
          const observed = await this.#rootIdentity;
          if (observed.error !== undefined) throw observed.error;
          if (observed.identity === undefined) throw new Error(`owned root identity unavailable after exit: ${this.#child.pid}`);
          return terminateTree({ pid: this.#child.pid, birth: observed.identity.birth }, this.#detached, this.#deadline, () => performance.now());
        }
        return terminateTree(this.#child, this.#detached, this.#deadline, () => performance.now());
      }
      // 自定义 hook 的第三参保留原 Date 绝对时间契约，不能把 monotonic 域冒充给它。
      return this.#options.terminateProcessTree(this.#child, this.#detached, Date.now() + Math.max(0, this.#deadline - performance.now()));
    }).catch((error) => {
      this.#recordTerminationError(error);
    });
  }

  #recordTerminationError(error) {
    this.#terminationError = this.#terminationError === undefined ? error : new AggregateError(
      [this.#terminationError, error], `${this.#terminationError.message}; ${error.message}`);
  }

  #onError(error) {
    this.#settleOnce(() => {
      this.#reject(new Error(`cannot start ordinary test command ${this.#options.command}: ${error.message}`));
    });
  }

  #onClose(code, signal) {
    this.#recordedExit = { code, signal };
    // close 可能先于已经逾期的 timer 被派发；成功也必须复核真实预算。
    if (performance.now() >= this.#executionDeadline) this.#timedOut = true;
    if (code !== 0 || this.#timedOut) this.#terminate();
    if (this.#termination !== undefined) {
      this.#termination.then(() => this.#completeClose(code, signal));
      return;
    }
    this.#completeClose(code, signal);
  }

  #commandFailure(code, signal, includeTimeout = true) {
    const { command, timeoutMs, captureOutput } = this.#options;
    let failure;
    if (this.#outputCallbackError !== undefined) {
      failure = this.#outputCallbackError;
    } else if (this.#abortReason !== undefined) {
      failure = new Error(`bounded command ${command} was aborted because a sibling command failed: ${this.#abortReason.message}`, { cause: this.#abortReason });
    } else if (code !== undefined && code !== 0 && (!this.#timedOut || code !== null)) {
      const detail = captureOutput && this.#stderr.trim().length > 0 ? `: ${this.#stderr.trim()}` : "";
      failure = new Error(`bounded command exited with ${code ?? `signal ${signal}`}${detail}`);
    }
    if (includeTimeout && this.#timedOut) {
      const timeoutFailure = new Error(`bounded command exhausted its execution budget within the total ${timeoutMs}ms deadline; process tree termination was requested and direct child close confirmed`);
      failure = failure === undefined ? timeoutFailure : new AggregateError([failure, timeoutFailure], `${failure.message}; ${timeoutFailure.message}`);
    }
    if (this.#terminationError !== undefined) {
      const cleanupFailure = new Error(`bounded command process tree termination remains unconfirmed: ${this.#terminationError.message}`, { cause: this.#terminationError });
      failure = failure === undefined ? cleanupFailure : new AggregateError([failure, cleanupFailure], `${failure.message}; ${cleanupFailure.message}`);
    }
    return failure;
  }

  #completeClose(code, signal) {
    this.#settleOnce(() => {
      const failure = this.#commandFailure(code, signal);
      if (failure !== undefined) this.#reject(failure);
      else this.#resolve({ stdout: this.#stdout, stderr: this.#stderr });
    });
  }

  #settleOnce(complete) {
    if (this.#settled) return;
    this.#settled = true;
    this.#cleanup();
    complete();
  }

  #cleanup() {
    clearTimeout(this.#executionTimer);
    clearTimeout(this.#hardTimer);
    clearTimeout(this.#finalSignalTimer);
    this.#options.signal?.removeEventListener("abort", this.#abortListener);
  }
}

export function runWithDeadline(options) {
  const timeoutMs = options.timeoutMs ?? ORDINARY_TEST_MAX_MS;
  if (timeoutMs > ORDINARY_TEST_MAX_MS) {
    throw new Error(`ordinary test deadline must be within ${ORDINARY_TEST_MAX_MS}ms; got ${timeoutMs}`);
  }
  return runBoundedCommand({ ...options, timeoutMs });
}

export function parseArgs(argv) {
  const separator = argv.indexOf("--");
  if (separator !== 1 || argv.length < 3 || !/^\d+$/.test(argv[0])) {
    throw new Error("usage: run-with-deadline.mjs <timeout-ms> -- <command> [args...]");
  }
  return { timeoutMs: Number(argv[0]), command: argv[2], args: argv.slice(3) };
}

export async function main(argv) {
  await runWithDeadline(parseArgs(argv));
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
