#!/usr/bin/env node
import path from "node:path";
import { fileURLToPath } from "node:url";
import { runBoundedCommand } from "./run-with-deadline.mjs";

export const CACHE_BUDGET_BYTES = 10_000_000_000;
const rollingKeys = [
  /^(distribution-native-v1-(?:ubuntu-24\.04|windows-2022|macos-15)-(?:X64|ARM64)-(?:desktop|webui-server|server))-[a-f0-9]{64}-\d+-\d+$/,
  /^(sealed-cargo-v2-no-debug-(?:Linux|Windows|macOS)-(?:X64|ARM64))-[a-f0-9]{64}-\d+-\d+$/,
];
const activeStatuses = ["queued", "in_progress", "waiting", "pending", "requested"];

function validTimestamp(value) {
  if (typeof value !== "string" || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?Z$/.test(value)) return false;
  const milliseconds = Date.parse(value);
  return Number.isFinite(milliseconds) && new Date(milliseconds).toISOString().slice(0, 19) === value.slice(0, 19);
}

function timestampKey(cache) {
  return cache.created_at.replace(/(?:\.(\d{1,9}))?Z$/, (suffix, fraction) => `.${(fraction === undefined ? "" : fraction).padEnd(9, "0")}Z`);
}

export function planCachePrune(caches) {
  if (!Array.isArray(caches)) throw new Error("GitHub caches must be an array");
  const ids = new Set();
  const newest = new Map();
  const ordered = [...caches];
  let beforeBytes = 0;
  for (const cache of ordered) {
    if (!cache || !Number.isSafeInteger(cache.id) || cache.id <= 0 || ids.has(cache.id)
      || typeof cache.key !== "string" || cache.key.length === 0
      || typeof cache.ref !== "string" || cache.ref.length === 0
      || !Number.isSafeInteger(cache.size_in_bytes) || cache.size_in_bytes < 0
      || !validTimestamp(cache.created_at)) {
      throw new Error(`Invalid or duplicate GitHub cache: ${JSON.stringify(cache)}`);
    }
    ids.add(cache.id);
    beforeBytes += cache.size_in_bytes;
  }
  if (!Number.isSafeInteger(beforeBytes)) throw new Error("GitHub cache byte total is unsafe");
  ordered.sort((left, right) => timestampKey(right).localeCompare(timestampKey(left)) || right.id - left.id);
  const remove = [];
  for (const cache of ordered) {
    const series = rollingKeys.map((pattern) => pattern.exec(cache.key)).find((match) => match !== null);
    if (!series) continue;
    const group = JSON.stringify([cache.ref, series[1]]);
    if (newest.has(group)) remove.push(cache);
    else newest.set(group, cache);
  }
  return { beforeBytes, afterBytes: beforeBytes - remove.reduce((sum, cache) => sum + cache.size_in_bytes, 0), remove };
}

function pageEntries(raw, field) {
  const pages = JSON.parse(raw);
  if (!Array.isArray(pages) || pages.length === 0 || pages.some((page) => !page || !Array.isArray(page[field]))) {
    throw new Error(`Invalid paginated GitHub response: expected ${field}`);
  }
  return pages.flatMap((page) => page[field]);
}

async function invokeGh(args) {
  const { stdout } = await runBoundedCommand({ command: "gh", args, timeoutMs: 30_000, captureOutput: true });
  return stdout;
}

export async function main(argv, { gh = invokeGh, env = process.env, log = console.log } = {}) {
  if (argv[0] !== "--repo" || !/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(argv[1])
    || argv[1].split("/").some((part) => part === "." || part === "..")
    || !(argv.length === 2 || (argv.length === 3 && argv[2] === "--apply"))) {
    throw new Error("usage: prune-actions-cache.mjs --repo OWNER/REPO [--apply]");
  }
  if (env.GITHUB_RUN_ID !== undefined && !/^\d+$/.test(env.GITHUB_RUN_ID)) {
    throw new Error("Invalid GITHUB_RUN_ID for cache cleanup");
  }
  const repository = argv[1];
  const api = async (endpoint, field) => pageEntries(await gh(["api", "--paginate", "--slurp", `repos/${repository}/actions/${endpoint}`]), field);
  const readPlan = async () => planCachePrune(await api("caches?per_page=100", "actions_caches"));
  const summarize = (plan, mode) => log(JSON.stringify({ mode, beforeBytes: plan.beforeBytes,
    projectedBytes: plan.afterBytes, budgetBytes: CACHE_BUDGET_BYTES,
    remove: plan.remove.map(({ id, key, ref, size_in_bytes }) => ({ id, key, ref, size_in_bytes })) }));
  const initial = await readPlan();
  summarize(initial, argv[2] === "--apply" ? "apply" : "dry-run");
  if (argv[2] !== "--apply") return initial;
  while (true) {
    const plan = await readPlan();
    if (plan.remove.length === 0) {
      if (plan.beforeBytes > CACHE_BUDGET_BYTES) {
        throw new Error(`GitHub cache usage ${plan.beforeBytes} exceeds ${CACHE_BUDGET_BYTES} bytes; only latest/unknown caches remain, refusing to delete them`);
      }
      summarize(plan, "verified");
      return plan;
    }
    const runs = await api("runs?per_page=100", "workflow_runs");
    if (runs.some((run) => !run || !Number.isSafeInteger(run.id) || run.id <= 0 || ![...activeStatuses, "completed"].includes(run.status))) {
      throw new Error("Invalid active GitHub workflow run response; refusing cache deletion");
    }
    const others = runs.filter((run) => run.status !== "completed" && String(run.id) !== env.GITHUB_RUN_ID);
    if (others.length > 0) {
      log(`Cache cleanup deferred: active workflow runs ${[...new Set(others.map((run) => run.id))].join(", ")}; current bytes=${plan.beforeBytes}, budget=${CACHE_BUDGET_BYTES}`);
      return { ...plan, deferred: true };
    }
    const refreshed = await readPlan();
    if (refreshed.remove.length === 0) continue;
    const candidate = refreshed.remove[0];
    await gh(["cache", "delete", String(candidate.id), "--repo", repository]);
    log(`Deleted superseded cache ${candidate.id} (${candidate.size_in_bytes} bytes): ${candidate.key}`);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(`Actions cache cleanup failed: ${error.message}`);
    process.exitCode = 1;
  });
}
