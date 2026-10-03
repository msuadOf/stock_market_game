#!/usr/bin/env node
import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const ORDINARY_TEST_MAX_MS = 10_000;
export const LONG_VALIDATION_MAX_MS = 300_000;
export const COMMAND_CLEANUP_RESERVE_MAX_MS = 1_000;

function terminateTree(child) {
  if (child.pid === undefined) return;
  if (process.platform === "win32") {
    const killer = spawn("taskkill", ["/pid", String(child.pid), "/T", "/F"], {
      windowsHide: true,
      stdio: "ignore",
    });
    killer.unref();
    return;
  }
  try {
    process.kill(-child.pid, "SIGKILL");
  } catch (error) {
    if (error?.code !== "ESRCH") throw error;
  }
}

export function runBoundedCommand({ command, args = [], timeoutMs, cwd = process.cwd(), env = process.env, spawnProcess = spawn, captureOutput = false, onStdout, onStderr, signal, cleanupReserveMs = Math.min(COMMAND_CLEANUP_RESERVE_MAX_MS, Math.max(1, Math.floor(timeoutMs / 5))) }) {
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
  return new BoundedCommandRun({ command, args, timeoutMs, cwd, env, spawnProcess, captureOutput, onStdout, onStderr, signal, cleanupReserveMs }).start();
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
      this.#child = spawnProcess(command, args, {
        cwd,
        env,
        stdio: captureOutput ? ["ignore", "pipe", "pipe"] : "inherit",
        windowsHide: true,
        detached: process.platform !== "win32",
      });
      if (captureOutput) {
        this.#child.stdout?.on("data", (chunk) => this.#onOutput("stdout", chunk));
        this.#child.stderr?.on("data", (chunk) => this.#onOutput("stderr", chunk));
      }
      this.#executionTimer = setTimeout(() => {
        this.#timedOut = true;
        terminateTree(this.#child);
      }, timeoutMs - cleanupReserveMs);
      this.#hardTimer = setTimeout(() => this.#settleOnce(() => {
        reject(new Error(`bounded command exceeded its total ${timeoutMs}ms deadline; its process tree was terminated but close did not settle within the ${cleanupReserveMs}ms cleanup reserve`));
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
      terminateTree(this.#child);
    }
  }

  #abort() {
    const { command, signal } = this.#options;
    this.#abortReason = signal.reason instanceof Error ? signal.reason : new Error(`bounded command ${command} was aborted`);
    terminateTree(this.#child);
  }

  #onError(error) {
    this.#settleOnce(() => {
      this.#reject(new Error(`cannot start ordinary test command ${this.#options.command}: ${error.message}`));
    });
  }

  #onClose(code, signal) {
    this.#settleOnce(() => {
      const { command, timeoutMs, captureOutput } = this.#options;
      if (this.#outputCallbackError !== undefined) {
        this.#reject(this.#outputCallbackError);
      } else if (this.#abortReason !== undefined) {
        this.#reject(new Error(`bounded command ${command} was aborted because a sibling command failed: ${this.#abortReason.message}`, { cause: this.#abortReason }));
      } else if (this.#timedOut) {
        this.#reject(new Error(`bounded command exhausted its execution budget within the total ${timeoutMs}ms deadline and its process tree was terminated`));
      } else if (code !== 0) {
        const detail = captureOutput && this.#stderr.trim().length > 0 ? `: ${this.#stderr.trim()}` : "";
        this.#reject(new Error(`bounded command exited with ${code ?? `signal ${signal}`}${detail}`));
      } else {
        this.#resolve({ stdout: this.#stdout, stderr: this.#stderr });
      }
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
