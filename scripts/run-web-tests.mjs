#!/usr/bin/env node
import fsp from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  COMMAND_CLEANUP_RESERVE_MAX_MS,
  ORDINARY_TEST_MAX_MS,
  runBoundedCommand,
} from "./run-with-deadline.mjs";

const DEFAULT_REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const MAX_WEB_TEST_PROCESSES = 8;
const INTERNAL_WORKER_ENV = "STOCK_GAME_WEB_TEST_INTERNAL_WORKER";

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
  files.forEach((file, index) => shards[index % shardCount].push(file));
  return {
    available_cpu_count: availableCpuCount,
    file_count: files.length,
    shard_count: shardCount,
    max_concurrent_processes: shardCount,
    aggregate_process_budget: shardCount,
    shards,
  };
}

export async function runWebTestBatch({
  webRoot = path.join(DEFAULT_REPO_ROOT, "apps", "web"),
  cpuCount = os.availableParallelism(),
  discover = discoverWebTestFiles,
  run = runBoundedCommand,
  now = Date.now,
  log = () => undefined,
} = {}) {
  assertSupportedNodeVersion();
  const startedAt = now();
  const files = await discover(webRoot);
  const policy = buildWebTestShardPolicy(cpuCount, files);
  const controller = new AbortController();
  const failures = [];
  const completed = [];
  const remainingMs = (context) => {
    const remaining = Math.floor(startedAt + ORDINARY_TEST_MAX_MS - now());
    if (remaining <= 1) throw new Error(`Web test batch exhausted its ${ORDINARY_TEST_MAX_MS}ms deadline before ${context}`);
    return remaining;
  };
  await Promise.all(policy.shards.map(async (shard, index) => {
    const label = `Web test shard ${index + 1}/${policy.shard_count}`;
    const shardStartedAt = now();
    try {
      const timeoutMs = Math.min(ORDINARY_TEST_MAX_MS, remainingMs(label));
      await run({
        command: process.execPath,
        args: [
          "--test",
          "--test-timeout=10000",
          "--test-isolation=none",
          "--test-concurrency=1",
          "--test-force-exit",
          ...shard,
        ],
        cwd: webRoot,
        env: process.env,
        timeoutMs,
        cleanupReserveMs: Math.min(COMMAND_CLEANUP_RESERVE_MAX_MS, timeoutMs - 1),
        signal: controller.signal,
      });
      completed.push({ shard: index + 1, file_count: shard.length, wall_ms: now() - shardStartedAt });
    } catch (error) {
      if (!controller.signal.aborted) controller.abort(error);
      failures.push(new Error(`${label} failed: ${error.message}`, { cause: error }));
    }
  }));
  if (failures.length > 0) throw new AggregateError(failures, `${failures.length} Web test shard failed`);
  const wallMs = now() - startedAt;
  if (wallMs > ORDINARY_TEST_MAX_MS) throw new Error(`Web test batch exceeded its ${ORDINARY_TEST_MAX_MS}ms deadline`);
  completed.sort((left, right) => left.shard - right.shard);
  const result = { wall_ms: wallMs, ...policy, shards: completed };
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
    env: { ...process.env, [INTERNAL_WORKER_ENV]: "1" },
    timeoutMs: ORDINARY_TEST_MAX_MS,
    cleanupReserveMs: COMMAND_CLEANUP_RESERVE_MAX_MS,
  });
}

export async function main(argv, env = process.env) {
  if (argv.length === 0) return runWebTests();
  if (argv.length === 1 && argv[0] === "--internal-worker") {
    if (env[INTERNAL_WORKER_ENV] !== "1") {
      throw new Error("Web test internal worker must be started by the external ten-second supervisor");
    }
    return runWebTestBatch({ log: console.log });
  }
  throw new Error("usage: run-web-tests.mjs");
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
