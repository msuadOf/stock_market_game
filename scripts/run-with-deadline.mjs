#!/usr/bin/env node
import { spawn, spawnSync } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const ORDINARY_TEST_MAX_MS = 10_000;
export const LONG_VALIDATION_MAX_MS = 300_000;
export const COMMAND_CLEANUP_RESERVE_MAX_MS = 1_000;

export async function terminateTree(child, detached = false, deadline = Date.now() + COMMAND_CLEANUP_RESERVE_MAX_MS) {
  if (child.pid === undefined) return;
  if (process.platform === "win32") {
    await new Promise((resolve, reject) => {
      const killer = spawn("taskkill", ["/pid", String(child.pid), "/T", "/F"], {
        windowsHide: true,
        stdio: "ignore",
        timeout: Math.max(1, deadline - Date.now()),
      });
      killer.on("error", reject);
      killer.on("close", (code) => code === 0 ? resolve() : reject(new Error(`taskkill did not confirm process tree termination: exit ${code}`)));
    });
    return;
  }
  const processes = process.platform === "linux" ? linuxProcesses() : posixProcesses();
  const descendants = new Set([child.pid]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const [pid, parentPid] of processes) {
      if (descendants.has(parentPid) && !descendants.has(pid)) { descendants.add(pid); changed = true; }
    }
  }
  for (const pid of [...descendants].reverse()) {
    try { process.kill(pid, "SIGKILL"); } catch (error) { if (error?.code !== "ESRCH") throw error; }
  }
  if (detached) {
    try { process.kill(-child.pid, "SIGKILL"); } catch (error) { if (error?.code !== "ESRCH") throw error; }
  }
  if (process.platform === "linux") {
    while ([...descendants].some((pid) => linuxProcessIsLive(pid))) {
      if (Date.now() >= deadline) throw new Error("owned process tree exit was not confirmed within the cleanup deadline");
      await new Promise((resolve) => setTimeout(resolve, Math.min(5, Math.max(1, deadline - Date.now()))));
    }
  }
}

function linuxProcessIsLive(pid) {
  try {
    const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
    const state = stat.slice(stat.lastIndexOf(")") + 2).split(" ")[0];
    return state !== "Z" && state !== "X";
  } catch (error) {
    if (error.code === "ENOENT" || error.code === "ESRCH") return false;
    throw error;
  }
}

function linuxProcesses() {
  const processes = [];
  for (const entry of readdirSync("/proc")) {
    if (!/^\d+$/.test(entry)) continue;
    try {
      const stat = readFileSync(`/proc/${entry}/stat`, "utf8");
      const fields = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
      processes.push([Number(entry), Number(fields[1])]);
    } catch (error) {
      if (error.code !== "ENOENT" && error.code !== "ESRCH") throw error;
    }
  }
  return processes;
}

function posixProcesses() {
  const listing = spawnSync("ps", ["-axo", "pid=,ppid="], { encoding: "utf8", timeout: 100, maxBuffer: 4 * 1024 * 1024 });
  if (listing.error || listing.status !== 0) throw new Error(`cannot enumerate owned process tree: ${listing.error?.message ?? listing.stderr}`);
  return listing.stdout.trim().split("\n").map((line) => line.trim().split(/\s+/).map(Number));
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
  #abortListener;
  #detached;
  #termination;
  #terminationError;
  #deadline;
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
      this.#deadline = Date.now() + timeoutMs;
      this.#detached = process.platform !== "win32" && env.STOCK_GAME_DEADLINE_GROUP !== "owned";
      this.#child = spawnProcess(command, args, {
        cwd,
        env: { ...env, STOCK_GAME_DEADLINE_GROUP: "owned" },
        stdio: captureOutput ? ["ignore", "pipe", "pipe"] : "inherit",
        windowsHide: true,
        detached: this.#detached,
      });
      if (captureOutput) {
        this.#child.stdout?.on("data", (chunk) => this.#onOutput("stdout", chunk));
        this.#child.stderr?.on("data", (chunk) => this.#onOutput("stderr", chunk));
      }
      this.#executionTimer = setTimeout(() => {
        this.#timedOut = true;
        this.#terminate();
      }, timeoutMs - cleanupReserveMs);
      this.#hardTimer = setTimeout(() => this.#settleOnce(() => {
        reject(new Error(`bounded command exceeded its total ${timeoutMs}ms deadline; process tree termination remains unconfirmed; close did not settle within the ${cleanupReserveMs}ms cleanup reserve${this.#terminationError ? `: ${this.#terminationError.message}` : ""}`));
      }), timeoutMs);
      signal?.addEventListener("abort", this.#abortListener, { once: true });
      this.#child.on("error", (error) => this.#onError(error));
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
    this.#termination = Promise.resolve().then(() => this.#options.terminateProcessTree(this.#child, this.#detached, this.#deadline)).catch((error) => {
      this.#terminationError = error;
    });
  }

  #onError(error) {
    this.#settleOnce(() => {
      this.#reject(new Error(`cannot start ordinary test command ${this.#options.command}: ${error.message}`));
    });
  }

  #onClose(code, signal) {
    if (code !== 0) this.#terminate();
    if (this.#termination !== undefined) {
      this.#termination.then(() => this.#completeClose(code, signal));
      return;
    }
    this.#completeClose(code, signal);
  }

  #completeClose(code, signal) {
    this.#settleOnce(() => {
      const { command, timeoutMs, captureOutput } = this.#options;
      let failure;
      if (this.#outputCallbackError !== undefined) {
        failure = this.#outputCallbackError;
      } else if (this.#abortReason !== undefined) {
        failure = new Error(`bounded command ${command} was aborted because a sibling command failed: ${this.#abortReason.message}`, { cause: this.#abortReason });
      } else if (this.#timedOut) {
        failure = new Error(`bounded command exhausted its execution budget within the total ${timeoutMs}ms deadline; process tree termination was requested and direct child close confirmed`);
      } else if (code !== 0) {
        const detail = captureOutput && this.#stderr.trim().length > 0 ? `: ${this.#stderr.trim()}` : "";
        failure = new Error(`bounded command exited with ${code ?? `signal ${signal}`}${detail}`);
      }
      if (this.#terminationError !== undefined) {
        const cleanupFailure = new Error(`bounded command process tree termination remains unconfirmed: ${this.#terminationError.message}`, { cause: this.#terminationError });
        failure = failure === undefined ? cleanupFailure : new AggregateError([failure, cleanupFailure], `${failure.message}; ${cleanupFailure.message}`);
      }
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
