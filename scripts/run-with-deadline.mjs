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

export function runBoundedCommand({ command, args = [], timeoutMs, cwd = process.cwd(), env = process.env, spawnProcess = spawn, captureOutput = false, signal, cleanupReserveMs = Math.min(COMMAND_CLEANUP_RESERVE_MAX_MS, Math.max(1, Math.floor(timeoutMs / 5))) }) {
  if (typeof command !== "string" || command.length === 0) throw new Error("deadline command must be non-empty");
  if (!Number.isInteger(timeoutMs) || timeoutMs <= 0 || timeoutMs > LONG_VALIDATION_MAX_MS) {
    throw new Error(`bounded command deadline must be within ${LONG_VALIDATION_MAX_MS}ms; got ${timeoutMs}`);
  }
  if (!Number.isInteger(cleanupReserveMs) || cleanupReserveMs <= 0 || cleanupReserveMs >= timeoutMs) {
    throw new Error(`bounded command cleanup reserve must be a positive integer smaller than ${timeoutMs}ms; got ${cleanupReserveMs}`);
  }
  if (signal?.aborted) return Promise.reject(signal.reason instanceof Error ? signal.reason : new Error(`bounded command ${command} was aborted before start`));
  return new Promise((resolve, reject) => {
    const child = spawnProcess(command, args, {
      cwd,
      env,
      stdio: captureOutput ? ["ignore", "pipe", "pipe"] : "inherit",
      windowsHide: true,
      detached: process.platform !== "win32",
    });
    let stdout = "";
    let stderr = "";
    if (captureOutput) {
      child.stdout?.on("data", (chunk) => { stdout += chunk; });
      child.stderr?.on("data", (chunk) => { stderr += chunk; });
    }
    let timedOut = false;
    let abortReason;
    let settled = false;
    const cleanup = () => {
      clearTimeout(executionTimer);
      clearTimeout(hardTimer);
      signal?.removeEventListener("abort", abortChild);
    };
    const abortChild = () => {
      abortReason = signal.reason instanceof Error ? signal.reason : new Error(`bounded command ${command} was aborted`);
      terminateTree(child);
    };
    const executionTimer = setTimeout(() => {
      timedOut = true;
      terminateTree(child);
    }, timeoutMs - cleanupReserveMs);
    const hardTimer = setTimeout(() => {
      if (settled) return;
      settled = true;
      cleanup();
      reject(new Error(`bounded command exceeded its total ${timeoutMs}ms deadline; its process tree was terminated but close did not settle within the ${cleanupReserveMs}ms cleanup reserve`));
    }, timeoutMs);
    signal?.addEventListener("abort", abortChild, { once: true });
    child.on("error", (error) => {
      if (settled) return;
      settled = true;
      cleanup();
      reject(new Error(`cannot start ordinary test command ${command}: ${error.message}`));
    });
    child.on("close", (code, signal) => {
      if (settled) return;
      settled = true;
      cleanup();
      if (abortReason !== undefined) {
        reject(new Error(`bounded command ${command} was aborted because a sibling command failed: ${abortReason.message}`, { cause: abortReason }));
        return;
      }
      if (timedOut) {
        reject(new Error(`bounded command exhausted its execution budget within the total ${timeoutMs}ms deadline and its process tree was terminated`));
        return;
      }
      if (code !== 0) {
        const detail = captureOutput && stderr.trim().length > 0 ? `: ${stderr.trim()}` : "";
        reject(new Error(`bounded command exited with ${code ?? `signal ${signal}`}${detail}`));
        return;
      }
      resolve({ stdout, stderr });
    });
  });
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
