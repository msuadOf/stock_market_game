#!/usr/bin/env node
import fsp from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { COMMAND_CLEANUP_RESERVE_MAX_MS, LONG_VALIDATION_MAX_MS, ORDINARY_TEST_MAX_MS, runBoundedCommand } from "./run-with-deadline.mjs";
import { assertSupportedNodeVersion } from "./run-web-tests.mjs";

const DEFAULT_REPO_ROOT = path.resolve(import.meta.dirname, "..");
const MAX_SCRIPT_TEST_PROCESSES = 4;
export const SCRIPT_TEST_INTERNAL_WORKER_ENV = "STOCK_GAME_SCRIPT_TEST_INTERNAL_WORKER";

export async function discoverScriptTestFiles(repoRoot = DEFAULT_REPO_ROOT) {
  const directory = path.join(await fsp.realpath(repoRoot), "scripts");
  const files = [];
  async function discover(current) {
    const stat = await fsp.lstat(current);
    if (!stat.isDirectory() || stat.isSymbolicLink()) throw new Error(`scripts 测试目录必须是真实目录：${current}`);
    const entries = await fsp.readdir(current, { withFileTypes: true });
    entries.sort((left, right) => left.name.localeCompare(right.name));
    for (const entry of entries) {
      const filename = path.join(current, entry.name);
      if (entry.isSymbolicLink()) throw new Error(`scripts 测试源禁止 symbolic link：${filename}`);
      if (entry.isDirectory()) await discover(filename);
      else if (entry.isFile() && entry.name.endsWith(".test.mjs")) files.push(filename);
    }
  }
  await discover(directory);
  return files.sort();
}

export function buildScriptTestPolicy(cpuCount, files) {
  if (!Number.isInteger(cpuCount) || cpuCount <= 0) throw new Error("scripts 测试需要正整数 CPU 数");
  if (!Array.isArray(files) || files.length === 0) throw new Error("未发现 scripts 测试文件");
  if (files.some((file) => typeof file !== "string" || !file.endsWith(".test.mjs"))) throw new Error("scripts 测试清单存在非法路径");
  if (new Set(files).size !== files.length) throw new Error("scripts 测试清单存在重复路径");
  return { available_cpu_count: cpuCount, file_count: files.length,
    concurrent_file_workers: Math.min(cpuCount, files.length, MAX_SCRIPT_TEST_PROCESSES),
    case_timeout_ms: ORDINARY_TEST_MAX_MS, external_file_deadline_ms: ORDINARY_TEST_MAX_MS };
}

export async function runScriptTestBatch({
  cwd = DEFAULT_REPO_ROOT, env = process.env, cpuCount = os.availableParallelism(),
  discover = discoverScriptTestFiles, run = runBoundedCommand, now = Date.now,
  remainingMs, log = () => undefined,
} = {}) {
  assertSupportedNodeVersion();
  const startedAt = now();
  const remaining = (label) => {
    const localRemaining = Math.floor(startedAt + LONG_VALIDATION_MAX_MS - now());
    const budget = remainingMs === undefined ? localRemaining : Math.min(localRemaining, remainingMs(label));
    if (budget <= 1) throw new Error(`scripts 测试批次期限耗尽：${label}`);
    return budget;
  };
  const files = await discover(cwd);
  const policy = buildScriptTestPolicy(cpuCount, files);
  remaining("测试发现");
  log(JSON.stringify({ phase: "scripts-start", ...policy }));
  const controller = new AbortController();
  const failures = [];
  const completed = [];
  let cursor = 0;
  async function worker() {
    while (!controller.signal.aborted && cursor < files.length) {
      const filename = files[cursor++];
      const fileStartedAt = now();
      try {
        const timeoutMs = Math.min(ORDINARY_TEST_MAX_MS, remaining(filename));
        await run({ command: process.execPath,
          args: ["--test", "--test-timeout=10000", "--test-isolation=none", "--test-concurrency=1", filename],
          cwd, env, timeoutMs, signal: controller.signal,
          cleanupReserveMs: Math.min(COMMAND_CLEANUP_RESERVE_MAX_MS, timeoutMs - 1),
        });
        completed.push({ file: filename, wall_ms: now() - fileStartedAt });
      } catch (error) {
        failures.push(new Error(`scripts 测试失败 ${filename}：${error.message}`, { cause: error }));
        if (!controller.signal.aborted) controller.abort(error);
      }
    }
  }
  await Promise.all(Array.from({ length: policy.concurrent_file_workers }, worker));
  if (failures.length > 0) throw new AggregateError(failures, failures.map((failure) => failure.message).join("\n"));
  remaining("结果汇总");
  completed.sort((left, right) => left.file.localeCompare(right.file));
  const result = { phase: "scripts-completed", ...policy, wall_ms: now() - startedAt, files: completed };
  log(JSON.stringify(result));
  return result;
}

export async function runScriptTests({ cwd = DEFAULT_REPO_ROOT, run = runBoundedCommand } = {}) {
  return run({ command: process.execPath, args: [fileURLToPath(import.meta.url), "--internal-worker"],
    cwd, env: { ...process.env, [SCRIPT_TEST_INTERNAL_WORKER_ENV]: "1" },
    timeoutMs: LONG_VALIDATION_MAX_MS, cleanupReserveMs: COMMAND_CLEANUP_RESERVE_MAX_MS });
}

export async function main(argv, env = process.env) {
  if (argv.length === 0) return runScriptTests();
  if (argv.length === 1 && argv[0] === "--internal-worker" && env[SCRIPT_TEST_INTERNAL_WORKER_ENV] === "1") {
    return runScriptTestBatch({ log: console.log });
  }
  throw new Error("scripts internal worker 必须由进程外五分钟 supervisor 启动；用法：run-script-tests.mjs");
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((error) => { console.error(error.message); process.exitCode = 1; });
}
