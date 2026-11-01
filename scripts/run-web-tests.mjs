#!/usr/bin/env node
import fsp from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  COMMAND_CLEANUP_RESERVE_MAX_MS,
  ORDINARY_TEST_MAX_MS,
  LONG_VALIDATION_MAX_MS,
  runBoundedCommand,
} from "./run-with-deadline.mjs";

const DEFAULT_REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const MAX_WEB_TEST_PROCESSES = 8;
const MAX_WEB_FILE_PROCESSES = 8;
export const WEB_TEST_INTERNAL_WORKER_ENV = "STOCK_GAME_WEB_TEST_INTERNAL_WORKER";

export function assertSupportedNodeVersion(version = process.versions.node) {
  const match = /^(\d+)\.(\d+)\.(\d+)/.exec(version);
  if (!match) throw new Error(`cannot parse Node.js version: ${version}`);
  const major = Number(match[1]);
  const minor = Number(match[2]);
  if (major < 24 || (major === 24 && minor < 18)) {
    throw new Error(`Web tests require Node.js >=24.18.0; current version is ${version}`);
  }
}

async function collectTests(directory, files) {
  const entries = await fsp.readdir(directory, { withFileTypes: true });
  entries.sort((left, right) => left.name.localeCompare(right.name));
  for (const entry of entries) {
    const entryPath = path.join(directory, entry.name);
    if (entry.isSymbolicLink()) throw new Error(`Web test source must not contain a symbolic link: ${entryPath}`);
    if (entry.isDirectory()) {
      await collectTests(entryPath, files);
    } else if (entry.isFile() && /\.test\.tsx?$/.test(entry.name)) {
      files.push(entryPath);
    }
  }
}

export async function discoverWebTestFiles(webRoot) {
  const resolvedWebRoot = await fsp.realpath(path.resolve(webRoot));
  const sourceRoot = path.join(resolvedWebRoot, "src");
  const sourceStat = await fsp.lstat(sourceRoot);
  if (!sourceStat.isDirectory() || sourceStat.isSymbolicLink()) {
    throw new Error(`Web test source root must be a real directory: ${sourceRoot}`);
  }
  const files = [];
  await collectTests(sourceRoot, files);
  files.sort();
  return files;
}

export function buildWebTestShardPolicy(availableCpuCount, files) {
  if (!Number.isInteger(availableCpuCount) || availableCpuCount <= 0) {
    throw new Error("Web test sharding requires a positive CPU count");
  }
  if (!Array.isArray(files) || files.length === 0) throw new Error("no Web test files were discovered");
  if (files.some((file) => typeof file !== "string" || file.length === 0)) {
    throw new Error("Web test inventory contains an invalid path");
  }
  if (new Set(files).size !== files.length) throw new Error("Web test inventory contains duplicate paths");
  const shardCount = Math.min(MAX_WEB_TEST_PROCESSES, availableCpuCount, files.length);
  const shards = Array.from({ length: shardCount }, () => []);
  const compilerSuite = shardCount > 1
    ? files.find((file) => /(?:^|[/\\])app[/\\]workspace-grid-types\.test\.ts$/.test(file))
    : undefined;
  const sharedFiles = compilerSuite === undefined ? files : files.filter((file) => file !== compilerSuite);
  const sharedOffset = compilerSuite === undefined ? 0 : 1;
  if (compilerSuite !== undefined) shards[0].push(compilerSuite);
  sharedFiles.forEach((file, index) => shards[sharedOffset + index % (shardCount - sharedOffset)].push(file));
  const perShardFileConcurrency = Math.max(1, Math.floor(Math.min(availableCpuCount, MAX_WEB_FILE_PROCESSES) / shardCount));
  const fileProcessBudget = shardCount * perShardFileConcurrency;
  return {
    available_cpu_count: availableCpuCount,
    file_count: files.length,
    shard_count: shardCount,
    per_shard_file_concurrency: perShardFileConcurrency,
    max_concurrent_processes: fileProcessBudget,
    aggregate_process_budget: fileProcessBudget,
    shards,
  };
}

export async function runWebTestBatch({
  webRoot = path.join(DEFAULT_REPO_ROOT, "apps", "web"),
  cpuCount = os.availableParallelism(),
  env = process.env,
  batchIndex = 0,
  batchCount = 1,
  discover = discoverWebTestFiles,
  run = runBoundedCommand,
  now = Date.now,
  log = () => undefined,
  remainingMs: parentRemainingMs,
  signal,
} = {}) {
  assertSupportedNodeVersion();
  const startedAt = now();
  const inventory = await discover(webRoot);
  if (!Number.isInteger(batchCount) || batchCount < 1 || !Number.isInteger(batchIndex) || batchIndex < 0 || batchIndex >= batchCount || inventory.length < batchCount) {
    throw new Error("invalid Web test batch selection");
  }
  if (new Set(inventory).size !== inventory.length) throw new Error("Web test inventory contains duplicate paths");
  let files;
  if (batchCount === 12 || batchCount === 13) {
    const startup = inventory.filter((file) => /(?:^|[/\\])app[/\\]startup-screen\.test\.ts$/.test(file));
    if (startup.length !== 1) throw new Error("Web test inventory requires exactly one startup-screen test");
    const schema = inventory.filter((file) => /(?:^|[/\\])save[/\\]complete-save-schema\.test\.ts$/.test(file));
    if (schema.length !== 1) throw new Error("Web test inventory requires exactly one complete-save-schema test");
    const schemaExtra = inventory.filter((file) => /(?:^|[/\\])save[/\\]complete-save-schema-extra\.test\.ts$/.test(file));
    if (schemaExtra.length !== 1) throw new Error("Web test inventory requires exactly one complete-save-schema-extra test");
    const browser = inventory.filter((file) => /(?:^|[/\\])mobile[/\\]mobile-color-contrast\.test\.ts$/.test(file));
    if (browser.length !== 1) throw new Error("Web test inventory requires exactly one mobile-color-contrast test");
    const compiler = inventory.filter((file) => /(?:^|[/\\])app[/\\]workspace-grid-types\.test\.ts$/.test(file));
    if (batchCount === 13 && compiler.length !== 1) throw new Error("Web test inventory requires exactly one workspace-grid-types test");
    const rest = inventory.filter((file) => file !== startup[0] && file !== schema[0] && file !== schemaExtra[0] && file !== browser[0] && (batchCount !== 13 || file !== compiler[0]));
    files = batchIndex === 8 ? startup : batchIndex === 9 ? schema : batchIndex === 10 ? schemaExtra : batchIndex === 11 ? browser : batchIndex === 12 ? compiler : rest.filter((_, index) => index % 8 === batchIndex);
  } else {
    files = inventory.filter((_, index) => index % batchCount === batchIndex);
  }
  const policy = buildWebTestShardPolicy(cpuCount, files);
  log(JSON.stringify({ status: "started", available_cpu_count: policy.available_cpu_count,
    max_concurrent_processes: policy.max_concurrent_processes,
    shard_file_counts: policy.shards.map((shard) => shard.length) }));
  const controller = new AbortController();
  const combinedSignal = signal ? AbortSignal.any([signal, controller.signal]) : controller.signal;
  const failures = [];
  const completed = [];
  const remainingMs = (context) => {
    const local = Math.floor(startedAt + LONG_VALIDATION_MAX_MS - now());
    const remaining = parentRemainingMs === undefined ? local : Math.min(local, parentRemainingMs(context));
    if (remaining <= 1) throw new Error(`Web test batch exhausted its deadline before ${context}`);
    return remaining;
  };
  const queue = policy.shards.flat();
  let cursor = 0;
  async function worker() {
    while (!combinedSignal.aborted && cursor < queue.length) {
      await new Promise((resolve) => setImmediate(resolve));
      if (combinedSignal.aborted || cursor >= queue.length) break;
      const file = queue[cursor++];
      const fileStartedAt = now();
      try {
        const timeoutMs = Math.min(ORDINARY_TEST_MAX_MS, remainingMs(file));
        await run({
          command: process.execPath,
          args: ["--test", "--test-timeout=10000", "--test-concurrency=1", "--test-isolation=none", "--test-force-exit", file],
          cwd: webRoot, env, timeoutMs,
          cleanupReserveMs: Math.min(COMMAND_CLEANUP_RESERVE_MAX_MS, timeoutMs - 1),
          signal: combinedSignal,
        });
        completed.push({ file, wall_ms: now() - fileStartedAt });
      } catch (error) {
        failures.push(new Error(`Web test shard failed for ${file}: ${error.message}`, { cause: error }));
        if (!controller.signal.aborted) controller.abort(error);
      }
    }
  }
  await Promise.all(Array.from({ length: policy.max_concurrent_processes }, worker));
  if (signal?.aborted && failures.length === 0) throw signal.reason;
  if (failures.length > 0) throw new AggregateError(failures, failures.map((failure) => failure.message).join("\n"));
  remainingMs("result summary");
  completed.sort((left, right) => left.file.localeCompare(right.file));
  const result = { wall_ms: now() - startedAt, ...policy, files: completed };
  log(JSON.stringify(result));
  return result;
}

export async function runWebTests({
  cwd = DEFAULT_REPO_ROOT,
  run = runBoundedCommand,
  nodeExecutable = process.execPath,
} = {}) {
  return run({
    command: nodeExecutable,
    args: [fileURLToPath(import.meta.url), "--internal-worker"],
    cwd: path.resolve(cwd),
    env: { ...process.env, [WEB_TEST_INTERNAL_WORKER_ENV]: "1" },
    timeoutMs: LONG_VALIDATION_MAX_MS,
    cleanupReserveMs: COMMAND_CLEANUP_RESERVE_MAX_MS,
  });
}

export async function main(argv, env = process.env) {
  if (argv.length === 0) return runWebTests();
  if ((argv.length === 1 || argv.length === 3) && argv[0] === "--internal-worker") {
    if (env[WEB_TEST_INTERNAL_WORKER_ENV] !== "1") {
      throw new Error("Web test internal worker must be started by the external five-minute supervisor");
    }
    const batchIndex = argv.length === 3 ? Number(argv[1]) : 0;
    const batchCount = argv.length === 3 ? Number(argv[2]) : 1;
    const budget = env.STOCK_GAME_WEB_TEST_CPU_BUDGET;
    if (budget !== undefined && (!/^[1-9][0-9]*(?![\s\S])/.test(budget) || !Number.isSafeInteger(Number(budget)) || Number(budget) > os.availableParallelism())) {
      throw new Error("Web test CPU budget is invalid");
    }
    return runWebTestBatch({ batchIndex, batchCount, cpuCount: budget === undefined ? os.availableParallelism() : Number(budget), log: console.log });
  }
  throw new Error("usage: run-web-tests.mjs");
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
