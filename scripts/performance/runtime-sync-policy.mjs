#!/usr/bin/env node
/** 用固定市场输入配对测量 runtime sync policy 的成本。 */
import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { isDeepStrictEqual } from "node:util";
import { fileURLToPath } from "node:url";
import { runProcessSample } from "../simulation/escrow-performance-harness.mjs";
import { terminateTree } from "../run-with-deadline.mjs";

const CONFIG_SCHEMA = "runtime-sync-policy-config";
const SAMPLE_SCHEMA = "runtime-sync-policy-sample";
const CLEANUP_RESERVE_MS = 1000;
const MAX_DEADLINE_MS = 300000;
const MAX_U64 = (1n << 64n) - 1n;

function fail(message) { throw new Error(message); }
function isRecord(value) { return value !== null && typeof value === "object" && !Array.isArray(value); }
function exactKeys(value, keys, label) {
  if (!isRecord(value)) fail(`${label} must be an object`);
  const actual = Object.keys(value).sort();
  if (JSON.stringify(actual) !== JSON.stringify([...keys].sort())) fail(`${label} keys mismatch`);
}
function canonicalU64(value, label) {
  if (typeof value !== "string" || !/^(0|[1-9][0-9]*)$/.test(value)) fail(`${label} must be a canonical u64 decimal string`);
  const parsed = BigInt(value);
  if (parsed > MAX_U64) fail(`${label} exceeds u64`);
  return parsed;
}
function digest(bytes) { return createHash("sha256").update(bytes).digest("hex"); }
function hashIdentity(file, bytes, identity) { return `${identity}:${digest(bytes)}`; }
function samplePrefix(outputDirectory, batchIndex, item) {
  return path.join(outputDirectory, "samples", `${String(batchIndex).padStart(2, "0")}-${item.phase}-${String(item.round).padStart(2, "02")}-${String(item.position).padStart(2, "02")}-${item.policy}`);
}
async function persistSample(outputDirectory, batchIndex, item, writeFile) {
  const prefix = samplePrefix(outputDirectory, batchIndex, item);
  const metadata = { phase: item.phase, round: item.round, position: item.position, policy: item.policy, wall_ns: item.wall_ns, peak_process_tree_rss_bytes: item.peak_process_tree_rss_bytes, process_tree_thread_state: item.process_tree_thread_state, run_wall_ns: item.run_wall_ns, intraday_wall_ns: item.intraday_wall_ns, boundary_wall_ns: item.boundary_wall_ns };
  await writeFile(`${prefix}.json`, `${JSON.stringify(metadata, null, 2)}\n`, { flag: "wx" });
}
async function persistFailure(outputDirectory, batchIndex, item, writeFile, capture = { stdout: "", stderr: "" }, includeRaw = true) {
  const prefix = samplePrefix(outputDirectory, batchIndex, item);
  if (includeRaw) {
    await writeFile(`${prefix}.stdout`, capture.stdout, { flag: "wx" });
    await writeFile(`${prefix}.stderr`, capture.stderr, { flag: "wx" });
  }
  await writeFile(`${prefix}.failure.json`, `${JSON.stringify({ phase: item.phase, round: item.round, position: item.position, policy: item.policy, error: item.error.message }, null, 2)}\n`, { flag: "wx" });
}
function stableJson(value) {
  if (Array.isArray(value)) return `[${value.map(stableJson).join(",")}]`;
  if (isRecord(value)) return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableJson(value[key])}`).join(",")}}`;
  return JSON.stringify(value);
}

export function validateRuntimeSyncConfig(config) {
  if (!isRecord(config)) fail("runtime sync config must be an object");
  const required = ["schema", "schema_version", "binary", "cwd", "setup_file", "seed", "trading_days", "cpu_affinity", "source_revision"];
  for (const key of required) if (!(key in config)) fail(`runtime sync config is missing ${key}`);
  const allowed = [...required, "rounds", "warmup_rounds", "sample_interval_ms", "deadline_ms", "output_directory"];
  for (const key of Object.keys(config)) if (!allowed.includes(key)) fail(`runtime sync config has unknown key ${key}`);
  if (config.schema !== CONFIG_SCHEMA || config.schema_version !== 1) fail("runtime sync config schema is unsupported");
  for (const key of ["binary", "cwd", "setup_file"]) {
    if (typeof config[key] !== "string" || !path.isAbsolute(config[key])) fail(`${key} must be absolute`);
  }
  canonicalU64(config.seed, "seed");
  for (const [key, value, minimum] of [["trading_days", config.trading_days, 1], ["rounds", config.rounds ?? 5, 5], ["warmup_rounds", config.warmup_rounds ?? 1, 1], ["sample_interval_ms", config.sample_interval_ms ?? 5, 1]]) {
    if (!Number.isSafeInteger(value) || value < minimum) fail(`${key} must be an integer >= ${minimum}`);
  }
  if (config.rounds !== undefined && config.rounds !== 5) fail("rounds must declare the five-batch campaign");
  if (config.warmup_rounds !== undefined && config.warmup_rounds !== 1) fail("warmup_rounds must be one per batch");
  if (config.output_directory !== undefined && (typeof config.output_directory !== "string" || !path.isAbsolute(config.output_directory))) fail("output_directory must be absolute");
  if (!Number.isSafeInteger(config.deadline_ms ?? MAX_DEADLINE_MS) || (config.deadline_ms ?? MAX_DEADLINE_MS) <= CLEANUP_RESERVE_MS || (config.deadline_ms ?? MAX_DEADLINE_MS) > MAX_DEADLINE_MS) {
    fail(`deadline_ms must be > ${CLEANUP_RESERVE_MS} and <= ${MAX_DEADLINE_MS}`);
  }
  if (!Array.isArray(config.cpu_affinity) || config.cpu_affinity.length === 0
    || !config.cpu_affinity.every((cpu) => Number.isSafeInteger(cpu) && cpu >= 0)
    || new Set(config.cpu_affinity).size !== config.cpu_affinity.length) fail("cpu_affinity must be a non-empty list of unique Linux CPU numbers");
  if (typeof config.source_revision !== "string" || !/^[a-f0-9]{40}$/.test(config.source_revision)) fail("source_revision must be a full lowercase 40-hex revision");
  return {
    ...config,
    rounds: config.rounds ?? 5,
    warmup_rounds: config.warmup_rounds ?? 1,
    sample_interval_ms: config.sample_interval_ms ?? 5,
    deadline_ms: config.deadline_ms ?? MAX_DEADLINE_MS,
  };
}

function normalizeSetup(setup, label) {
  if (!isRecord(setup)) fail(`${label} setup must be an object`);
  const normalized = structuredClone(setup);
  delete normalized.runtime_sync_policy;
  return normalized;
}
function isDate(value) {
  if (typeof value !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(value)) return false;
  const date = new Date(`${value}T00:00:00.000Z`);
  return Number.isFinite(date.getTime()) && date.toISOString().slice(0, 10) === value;
}
function positiveSafeInteger(value, label) {
  if (!Number.isSafeInteger(value) || value <= 0) fail(`${label} must be a positive integer`);
}
function positiveDecimal(value, label, allowZero = false) {
  if (typeof value !== "string" || !/^(0|[1-9][0-9]*)$/.test(value) || (!allowZero && BigInt(value) === 0n)) fail(`${label} must be ${allowZero ? "a" : "a positive"} decimal string`);
  return BigInt(value);
}
function validateThreadState(value, label) {
  if (!isRecord(value) || value.schema !== "linux-process-tree-thread-state" || value.schema_version !== 1
    || value.sampled_state !== "R (running or runnable)") fail(`${label} thread-state sampling contract is invalid`);
  positiveSafeInteger(value.sample_count, `${label} thread sample_count`);
  positiveSafeInteger(value.sample_interval_ms, `${label} thread sample interval`);
  for (const field of ["process_count", "total_threads"]) {
    const range = value[field];
    if (!isRecord(range) || !Number.isSafeInteger(range.minimum) || range.minimum < 1
      || !Number.isSafeInteger(range.maximum) || range.maximum < range.minimum) fail(`${label} thread ${field} range is invalid`);
  }
  const runnable = value.runnable_threads;
  if (!isRecord(runnable) || !Number.isSafeInteger(runnable.minimum) || runnable.minimum < 0
    || !Number.isSafeInteger(runnable.maximum) || runnable.maximum < runnable.minimum
    || !Number.isFinite(runnable.mean) || runnable.mean < 0 || !isRecord(runnable.histogram)) fail(`${label} runnable thread evidence is missing`);
  let count = 0;
  let total = 0;
  for (const [threads, samples] of Object.entries(runnable.histogram)) {
    if (!/^(0|[1-9][0-9]*)$/.test(threads) || !Number.isSafeInteger(samples) || samples <= 0) fail(`${label} runnable thread histogram is invalid`);
    count += samples;
    total += Number(threads) * samples;
  }
  if (count !== value.sample_count || Math.abs(total / count - runnable.mean) > Number.EPSILON) fail(`${label} runnable histogram does not reproduce its summary`);
  if (runnable.maximum > value.total_threads.maximum) fail(`${label} runnable threads exceed total threads`);
}

export function validateRuntimeSyncSample(measured, config, expectedPolicy, label = "runtime sync sample") {
  exactKeys(measured, ["wall_ns", "peak_process_tree_rss_bytes", "process_tree_thread_state", "stdout", "stderr"], `${label} measurement`);
  positiveDecimal(measured.wall_ns, `${label} measured wall_ns`);
  positiveSafeInteger(measured.peak_process_tree_rss_bytes, `${label} peak RSS`);
  validateThreadState(measured.process_tree_thread_state, label);
  if (typeof measured.stdout !== "string" || typeof measured.stderr !== "string") fail(`${label} stdout/stderr must be strings`);
  let sample;
  try { sample = JSON.parse(measured.stdout); }
  catch (error) { fail(`${label} stdout is not sample JSON: ${error.message}`); }
  if (!isRecord(sample) || sample.schema !== SAMPLE_SCHEMA || sample.schema_version !== 1) fail(`${label} sample schema is unsupported`);
  if (sample.policy !== expectedPolicy || !["every_tick", "day_end"].includes(sample.policy)) fail(`${label} policy mismatch`);
  if (sample.seed !== config.seed || sample.trading_days !== config.trading_days) fail(`${label} seed/trading_days mismatch`);
  if (!isRecord(sample.setup) || sample.setup.runtime_sync_policy !== expectedPolicy) fail(`${label} setup policy mismatch`);
  const configuredSetup = JSON.parse(config.setup_json ?? config.setup);
  if (!isDeepStrictEqual(normalizeSetup(sample.setup, label), normalizeSetup(configuredSetup, "configured"))) fail(`${label} setup differs outside runtime_sync_policy`);
  positiveSafeInteger(sample.completed_trading_days, `${label} completed_trading_days`);
  if (sample.completed_trading_days !== config.trading_days) fail(`${label} completed trading days mismatch`);
  const ticksPerDay = sample.setup.ticks_per_day;
  positiveSafeInteger(ticksPerDay, `${label} setup.ticks_per_day`);
  const expectedTicks = BigInt(ticksPerDay) * BigInt(config.trading_days) + 1n;
  for (const key of ["completed_ticks", "expected_ticks", "last_tick"]) {
    if (!Number.isSafeInteger(sample[key]) || BigInt(sample[key]) !== expectedTicks) fail(`${label} ${key} does not equal exact stop tick ${expectedTicks}`);
  }
  positiveSafeInteger(sample.civil_updates, `${label} civil_updates`);
  if (!Number.isSafeInteger(sample.civil_updates) || sample.civil_updates < sample.completed_trading_days || !Number.isSafeInteger(sample.closed_civil_days) || sample.closed_civil_days < 0
    || sample.civil_updates !== sample.completed_trading_days + sample.closed_civil_days) fail(`${label} civil update count does not match trading and closed civil days`);
  if (!Array.isArray(sample.trading_day_boundaries) || sample.trading_day_boundaries.length !== config.trading_days) fail(`${label} trading-day boundary count mismatch`);
  if (!Array.isArray(sample.boundary_wall_ns) || sample.boundary_wall_ns.length !== config.trading_days) fail(`${label} boundary wall count mismatch`);
  for (const [index, boundary] of sample.trading_day_boundaries.entries()) {
    if (!isRecord(boundary) || !isRecord(boundary.mirror_origin) || typeof boundary.settled_date !== "string" || typeof boundary.next_date !== "string"
      || !isDate(boundary.settled_date) || !isDate(boundary.next_date)
      || !(boundary.settled_date < boundary.next_date) || (index > 0 && !(sample.trading_day_boundaries[index - 1].settled_date < boundary.settled_date))
      || (index > 0 && sample.trading_day_boundaries[index - 1].next_date > boundary.settled_date)) fail(`${label} trading-day boundary ${index} dates are invalid or unordered`);
    const origin = boundary.mirror_origin;
    if (origin.source_tick !== boundary.tick || !Number.isSafeInteger(origin.install_sequence) || origin.install_sequence <= 0
      || !Number.isSafeInteger(origin.authority_epoch) || !Number.isSafeInteger(origin.source_generation)
      || !Number.isSafeInteger(origin.non_tick_revision) || origin.day_end_stage !== "retained_published"
      || !["CallAuction", "PreOpen", "ClosingAuction", "Continuous"].includes(origin.mirror_phase)) fail(`${label} boundary mirror origin is invalid`);
    const combinedBoundaryWall = positiveDecimal(boundary.combined_boundary_wall_ns, `${label} combined boundary wall`, true);
    if (positiveDecimal(sample.boundary_wall_ns[index], `${label} boundary_wall_ns`, true) !== combinedBoundaryWall) fail(`${label} boundary wall sample does not match boundary receipt`);
    if (!Number.isSafeInteger(boundary.tick) || boundary.tick !== ticksPerDay * (index + 1)
      || boundary.save_candidate_tick !== boundary.tick || !Number.isSafeInteger(boundary.save_candidate_seq) || boundary.save_candidate_seq < 0) fail(`${label} trading-day boundary ${index} tick/save candidate mismatch`);
  }
  for (const field of ["run_wall_ns", "intraday_wall_ns", "initialization_wall_ns", "closed_civil_wall_ns"]) positiveDecimal(sample[field], `${label} ${field}`, field === "closed_civil_wall_ns" || field === "intraday_wall_ns");
  sample.boundary_wall_ns.forEach((value) => positiveDecimal(value, `${label} boundary_wall_ns`, true));
  if (sample.pure_flush_wall_ns !== null || sample.pure_flush_availability !== "unavailable") fail(`${label} pure_flush must be unavailable`);
  if (sample.shutdown_confirmed !== true || !isRecord(sample.shutdown) || !isRecord(sample.shutdown.runtime) || !isRecord(sample.topology)) fail(`${label} shutdown/topology evidence is invalid`);
  positiveSafeInteger(sample.topology.groups, `${label} topology.groups`);
  positiveSafeInteger(sample.topology.desks, `${label} topology.desks`);
  positiveSafeInteger(sample.stocks, `${label} stocks`);
  if (sample.topology.groups !== 1 || sample.topology.desks !== sample.stocks) fail(`${label} topology must be one production group and one desk per stock`);
  const shutdown = sample.shutdown.runtime;
  const ownedWorkers = sample.topology.groups + sample.topology.desks;
  if (shutdown.owned_workers !== ownedWorkers || shutdown.joined_workers !== ownedWorkers
    || shutdown.group_finals !== sample.topology.groups || shutdown.desk_finals !== sample.topology.desks
    || !Array.isArray(shutdown.worker_results) || shutdown.worker_results.length !== ownedWorkers
    || shutdown.worker_results.some((entry) => !isRecord(entry) || !isRecord(entry.result)
      || Object.keys(entry.result).length !== 1 || entry.result.Ok !== null)) fail(`${label} worker shutdown/join receipts are incomplete or failed`);
  positiveSafeInteger(sample.npc_count, `${label} npc_count`);
  if (!Number.isSafeInteger(sample.events) || sample.events < 0 || !Number.isSafeInteger(sample.final_seq) || sample.final_seq < 0
    || !isRecord(sample.final_mirror_origin) || typeof sample.final_civil_date !== "string"
    || sample.final_phase !== "intraday_trading" || sample.next_day_first_tick_confirmed !== true) fail(`${label} final state/next trading-day evidence is invalid`);
  const finalOrigin = sample.final_mirror_origin;
  if (!Number.isSafeInteger(finalOrigin.install_sequence) || finalOrigin.install_sequence <= 0
    || !Number.isSafeInteger(finalOrigin.source_tick) || finalOrigin.source_tick > sample.last_tick
    || !Number.isSafeInteger(finalOrigin.authority_epoch) || !Number.isSafeInteger(finalOrigin.source_generation)
    || !Number.isSafeInteger(finalOrigin.non_tick_revision)
    || !["tick", "day_end_committed", "retained_published"].includes(finalOrigin.day_end_stage)
    || !["CallAuction", "PreOpen", "ClosingAuction", "Continuous"].includes(finalOrigin.mirror_phase)) fail(`${label} final mirror origin is invalid`);
  if (expectedPolicy === "every_tick" ? finalOrigin.source_tick !== sample.last_tick || finalOrigin.day_end_stage !== "tick" : finalOrigin.source_tick !== sample.last_tick - 1 || finalOrigin.day_end_stage !== "retained_published") fail(`${label} final mirror source tick does not match policy`);
  const lastBoundary = sample.trading_day_boundaries.at(-1);
  if (expectedPolicy === "day_end" && finalOrigin.source_tick !== lastBoundary.tick) fail(`${label} day_end final mirror source tick differs from final trading boundary`);
  if (!isDate(sample.final_civil_date) || sample.final_civil_date < lastBoundary.next_date) fail(`${label} final civil date precedes the final trading-day boundary`);
  if (sample.npc_count !== sample.setup.npcs?.retail_count + sample.setup.npcs?.inst_count + sample.setup.npcs?.hot_count || sample.stocks !== sample.setup.stocks?.length) fail(`${label} npc/stocks counts do not match setup`);
  const wallNs = positiveDecimal(measured.wall_ns, `${label} measured wall_ns`);
  const boundaryTotal = sample.boundary_wall_ns.reduce((sum, value) => sum + BigInt(value), 0n);
  if (boundaryTotal > wallNs) fail(`${label} boundary time exceeds measured process wall time`);
  return { sample, measurement: measured };
}

function stats(values) {
  if (values.length === 0) fail("cannot summarize an empty sample set");
  const sorted = [...values].sort((a, b) => a < b ? -1 : a > b ? 1 : 0);
  const sum = values.reduce((total, value) => total + value, 0n);
  const percentile = (rank) => String(sorted[Math.max(0, Math.ceil(rank * sorted.length) - 1)]);
  const middle = values.length % 2 === 1 ? String(sorted[Math.floor(values.length / 2)]) : (() => { const pair = sorted[values.length / 2 - 1] + sorted[values.length / 2]; return pair % 2n === 0n ? String(pair / 2n) : `${pair / 2n}.5`; })();
  return { sample_count: values.length, minimum: String(sorted[0]), median: middle, p50: percentile(0.5), p95: percentile(0.95), maximum: String(sorted.at(-1)), mean: String(sum / BigInt(values.length)), percentile_method: "nearest-rank" };
}
function defaultLoad() { return process.platform === "linux" ? os.loadavg() : null; }
function numberStats(values) {
  if (values.length === 0) fail("cannot summarize an empty numeric sample set");
  const sorted = [...values].sort((a, b) => a - b);
  return { sample_count: values.length, minimum: sorted[0], median: sorted.length % 2 ? sorted[Math.floor(sorted.length / 2)] : (sorted[sorted.length / 2 - 1] + sorted[sorted.length / 2]) / 2, p50: sorted[Math.max(0, Math.ceil(0.5 * sorted.length) - 1)], p95: sorted[Math.max(0, Math.ceil(0.95 * sorted.length) - 1)], maximum: sorted.at(-1), mean: values.reduce((sum, value) => sum + value, 0) / values.length, percentile_method: "nearest-rank" };
}

export async function runRuntimeSyncPolicy(inputConfig, {
  runSample = runProcessSample,
  clock = () => performance.now(),
  identity = async (file) => file,
  readFile = (file) => fs.readFile(file),
  writeFile = (...args) => fs.writeFile(...args),
  openFile = (...args) => fs.open(...args),
  mkdir = (...args) => fs.mkdir(...args),
  renameFile = (...args) => fs.rename(...args),
  unlinkFile = (...args) => fs.unlink(...args),
  load = defaultLoad,
  spawnProcess = spawn,
  terminate = terminateTree,
  setDeadlineTimer = setTimeout,
  clearDeadlineTimer = clearTimeout,
  batchIndex,
} = {}) {
  if (!Number.isSafeInteger(batchIndex) || batchIndex < 1 || batchIndex > 5) fail("batch_index must be a pre-registered integer from 1 through 5");
  if (process.platform !== "linux") fail("runtime sync policy runner requires Linux taskset and /proc sampling");
  const childEnvironmentSnapshot = { ...process.env };
  const outputDirectory = inputConfig.output_directory;
  const config = validateRuntimeSyncConfig(inputConfig);
  const startedAt = clock();
  const deadlineAt = startedAt + config.deadline_ms;
  const executionDeadlineAt = deadlineAt - CLEANUP_RESERVE_MS;
  if (outputDirectory !== undefined) {
    await mkdir(outputDirectory, { recursive: false });
    await mkdir(path.join(outputDirectory, "samples"), { recursive: false });
  }
  const runtimeEnvironment = { ENGINE_STEP_RUNTIME: childEnvironmentSnapshot.ENGINE_STEP_RUNTIME ?? "1", ENGINE_SPIN_US: childEnvironmentSnapshot.ENGINE_SPIN_US ?? null, ENGINE_SPIN_DISABLE: childEnvironmentSnapshot.ENGINE_SPIN_DISABLE ?? null, STEP_PROFILE: childEnvironmentSnapshot.STEP_PROFILE ?? null };
  if (runtimeEnvironment.ENGINE_STEP_RUNTIME !== "1") fail("ENGINE_STEP_RUNTIME 必须为 1 或未设置，以确保 native runtime 实际启用");
  const writeBeforeDeadline = async (...args) => {
    if (clock() >= executionDeadlineAt) fail("runtime sync policy batch exhausted deadline before metadata write");
    await writeFile(...args);
    if (clock() >= executionDeadlineAt) fail("runtime sync policy batch exhausted deadline during metadata write");
  };
  const [setupBytes, binaryBytes] = await Promise.all([readFile(config.setup_file), readFile(config.binary)]);
  const setupText = Buffer.isBuffer(setupBytes) ? setupBytes.toString("utf8") : String(setupBytes);
  const configuredSetup = JSON.parse(setupText);
  normalizeSetup(configuredSetup, "configured");
  config.setup_json = JSON.stringify(configuredSetup);
  const beforeIdentity = {
    binary: hashIdentity(config.binary, binaryBytes, await identity(config.binary)),
    setup: hashIdentity(config.setup_file, setupBytes, await identity(config.setup_file)),
  };
  const binarySha256 = digest(binaryBytes);
  const setupSha256 = digest(setupBytes);
  const configForHash = { ...config, runtime_environment: runtimeEnvironment };
  delete configForHash.setup_json;
  delete configForHash.output_directory;
  const configSha256 = digest(stableJson(configForHash));
  if (clock() >= executionDeadlineAt) fail("runtime sync policy batch exhausted deadline during input fingerprinting");
  const loadBefore = load();
  let activeController;
  let activeChild;
  let activeCapture = { stdout: "", stderr: "" };
  let terminated = Promise.resolve();
  let timedOut = false;
  const abortActive = () => {
    timedOut = true;
    if (activeController !== undefined && !activeController.signal.aborted) {
      activeController.abort(new Error("runtime sync policy batch reached its shared deadline"));
    }
  };
  const timer = setDeadlineTimer(abortActive, Math.max(0, executionDeadlineAt - clock()));
  const samples = [];
  const counts = { warmup_rounds: 0, measured_rounds: 0, warmup_samples: 0, measured_samples: 0 };
  const collect = async (policy, phase, round, position) => {
    if (clock() >= executionDeadlineAt) { abortActive(); fail("runtime sync policy batch exhausted execution deadline"); }
    const controller = new AbortController();
    activeController = controller;
    const cpuList = config.cpu_affinity.join(",");
    const endpoint = {
      command: ["taskset", "-c", cpuList, config.binary, config.setup_file, policy, config.seed, String(config.trading_days)],
      cwd: config.cwd,
      source_fingerprint: digest(binaryBytes),
    };
    let stdoutHandle;
    let stderrHandle;
    let rawStreamed = false;
    let rawWriteError;
    let pendingWrites = Promise.resolve();
    const queueRawWrite = (handle, chunk) => {
      if (handle === undefined) return;
      pendingWrites = pendingWrites.then(async () => {
        if (rawWriteError !== undefined) throw rawWriteError;
        await handle.writeFile(chunk);
      }).catch((error) => { rawWriteError = error; abortActive(); });
    };
    const spawnBound = (command, args, options) => {
      if (controller.signal.aborted) throw controller.signal.reason;
      const childEnvironment = { ...childEnvironmentSnapshot, ENGINE_STEP_RUNTIME: "1" };
      activeChild = spawnProcess(command, args, { ...options, env: childEnvironment });
      activeCapture = { stdout: "", stderr: "" };
      activeChild.stdout?.on("data", (chunk) => { activeCapture.stdout += chunk.toString(); rawStreamed = true; queueRawWrite(stdoutHandle, chunk); });
      activeChild.stderr?.on("data", (chunk) => { activeCapture.stderr += chunk.toString(); rawStreamed = true; queueRawWrite(stderrHandle, chunk); });
      controller.signal.addEventListener("abort", () => {
        terminated = Promise.resolve(terminate(activeChild, false, Date.now() + Math.max(1, deadlineAt - clock())));
      }, { once: true });
      return activeChild;
    };
    let measuredResult;
    let rawPersisted = false;
    try {
      if (outputDirectory !== undefined) {
        const prefix = samplePrefix(outputDirectory, batchIndex, { phase, round, position, policy });
        stdoutHandle = await openFile(`${prefix}.stdout`, "wx");
        try { stderrHandle = await openFile(`${prefix}.stderr`, "wx"); }
        catch (error) { await stdoutHandle.close(); throw error; }
        rawPersisted = true;
      }
      const measured = await runSample(endpoint, {
        rssSampleIntervalMs: config.sample_interval_ms,
        signal: controller.signal,
        spawn: spawnBound,
        terminate: async (child) => terminate(child, false, Date.now() + Math.max(1, deadlineAt - clock())),
      });
      measuredResult = measured;
      if (outputDirectory !== undefined) {
        if (!rawStreamed && clock() < executionDeadlineAt) {
          await Promise.all([stdoutHandle.writeFile(measured.stdout), stderrHandle.writeFile(measured.stderr)]);
        }
        await pendingWrites;
        if (rawWriteError !== undefined) throw rawWriteError;
        await Promise.all([stdoutHandle.close(), stderrHandle.close()]);
        stdoutHandle = undefined; stderrHandle = undefined;
      }
      if (clock() >= executionDeadlineAt) { abortActive(); fail("runtime sync policy sample completed beyond the execution deadline"); }
      if (controller.signal.aborted) {
        await terminated;
        fail("runtime sync policy sample was cancelled by the shared deadline");
      }
      const validated = validateRuntimeSyncSample(measured, config, policy, `${phase} ${policy} ${round}.${position}`);
      const item = {
        phase, round, position, policy,
        wall_ns: measured.wall_ns,
        peak_process_tree_rss_bytes: measured.peak_process_tree_rss_bytes,
        process_tree_thread_state: measured.process_tree_thread_state,
        run_wall_ns: validated.sample.run_wall_ns,
        intraday_wall_ns: validated.sample.intraday_wall_ns,
        initialization_wall_ns: validated.sample.initialization_wall_ns,
        closed_civil_wall_ns: validated.sample.closed_civil_wall_ns,
        boundary_wall_ns: validated.sample.boundary_wall_ns,
        calendar_signature: { trading_day_boundaries: validated.sample.trading_day_boundaries.map(({ settled_date, next_date }) => ({ settled_date, next_date })), final_civil_date: validated.sample.final_civil_date, final_phase: validated.sample.final_phase, next_day_first_tick_confirmed: validated.sample.next_day_first_tick_confirmed },
        stdout: measured.stdout,
        stderr: measured.stderr,
      };
      samples.push(item);
      if (outputDirectory !== undefined) await persistSample(outputDirectory, batchIndex, item, writeBeforeDeadline);
      if (phase === "warmup") counts.warmup_samples += 1;
      else counts.measured_samples += 1;
    } catch (error) {
      const cleanupErrors = [];
      if (controller.signal.aborted) {
        try { await terminated; } catch (terminationError) { cleanupErrors.push(terminationError); }
      }
      await pendingWrites;
      if (rawWriteError !== undefined) cleanupErrors.push(rawWriteError);
      if (stdoutHandle !== undefined || stderrHandle !== undefined) {
        try { await Promise.all([stdoutHandle?.close(), stderrHandle?.close()]); }
        catch (closeError) { cleanupErrors.push(closeError); }
        stdoutHandle = undefined; stderrHandle = undefined;
      }
      if (outputDirectory !== undefined && clock() < executionDeadlineAt && !samples.some((sample) => sample.phase === phase && sample.round === round && sample.position === position)) {
        try { await persistFailure(outputDirectory, batchIndex, { phase, round, position, policy, error }, writeBeforeDeadline, measuredResult === undefined ? activeCapture : measuredResult, !rawPersisted); }
        catch (artifactError) { cleanupErrors.push(artifactError); }
      }
      if (cleanupErrors.length > 0) throw new AggregateError([error, ...cleanupErrors], `样本失败且清理/原始证据写入也失败：${[error, ...cleanupErrors].map((entry) => entry.message).join("；")}`, { cause: error });
      throw error;
    } finally {
      if (activeController === controller) activeController = undefined;
      activeChild = undefined;
      activeCapture = { stdout: "", stderr: "" };
    }
  };
  try {
    await collect("every_tick", "warmup", 0, 1);
    await collect("day_end", "warmup", 0, 2);
    counts.warmup_rounds = 1;
    for (const [position, policy] of ["every_tick", "day_end", "day_end", "every_tick"].entries()) {
      await collect(policy, "measured", 1, position + 1);
    }
    counts.measured_rounds = 1;
    if (timedOut || clock() >= executionDeadlineAt) fail("runtime sync policy batch exhausted execution deadline before validation");
    const [setupAfterBytes, binaryAfterBytes] = await Promise.all([readFile(config.setup_file), readFile(config.binary)]);
    const afterIdentity = {
      binary: hashIdentity(config.binary, binaryAfterBytes, await identity(config.binary)),
      setup: hashIdentity(config.setup_file, setupAfterBytes, await identity(config.setup_file)),
    };
    if (JSON.stringify(beforeIdentity) !== JSON.stringify(afterIdentity)) fail("binary or setup identity/content changed during runtime sync policy batch");
    if (timedOut || clock() >= executionDeadlineAt) fail("runtime sync policy batch exhausted shared deadline during validation");
    const measured = samples.filter((sample) => sample.phase === "measured");
    const allCalendarSignatures = samples.map((sample) => stableJson(sample.calendar_signature));
    if (new Set(allCalendarSignatures).size !== 1) fail("paired runtime sync samples differ in trading calendar boundaries or final civil date");
    const combinedBoundaryStats = (policy) => ({
      ...stats(measured.filter((sample) => sample.policy === policy).flatMap((sample) => sample.boundary_wall_ns.map(BigInt))),
      by_trading_day: Array.from({ length: config.trading_days }, (_, day) => ({
        trading_day: day + 1,
        ...stats(measured.filter((sample) => sample.policy === policy).map((sample) => BigInt(sample.boundary_wall_ns[day]))),
      })),
    });
    const summarizePolicy = (policy) => {
      const selected = measured.filter((sample) => sample.policy === policy);
      const boundaries = selected.flatMap((sample) => sample.boundary_wall_ns.map(BigInt));
      return {
        sample_count: selected.length,
        process_wall_ns: stats(selected.map((sample) => BigInt(sample.wall_ns))),
        run_wall_ns: stats(selected.map((sample) => BigInt(sample.run_wall_ns))),
        intraday_wall_ns: stats(selected.map((sample) => BigInt(sample.intraday_wall_ns))),
        initialization_wall_ns: stats(selected.map((sample) => BigInt(sample.initialization_wall_ns))),
        closed_civil_wall_ns: stats(selected.map((sample) => BigInt(sample.closed_civil_wall_ns))),
        combined_boundary_wall_ns: stats(boundaries),
        peak_process_tree_rss_bytes: stats(selected.map((sample) => BigInt(sample.peak_process_tree_rss_bytes))),
        runnable_threads: numberStats(selected.map((sample) => sample.process_tree_thread_state.runnable_threads.mean)),
        total_ticks_per_second: numberStats(selected.map((sample) => { const ticks = BigInt(config.trading_days) * BigInt(JSON.parse(sample.stdout).setup.ticks_per_day) + 1n; const rate = Number(ticks) * 1e9 / Number(sample.run_wall_ns); if (!Number.isFinite(rate)) fail("total tick throughput is not finite"); return rate; })),
        intraday_steps_per_second: selected.some((sample) => BigInt(sample.intraday_wall_ns) === 0n) ? { availability: "unavailable", value: null } : numberStats(selected.map((sample) => { const probe = JSON.parse(sample.stdout); const steps = BigInt(probe.completed_ticks - config.trading_days); const rate = Number(steps) * 1e9 / Number(sample.intraday_wall_ns); if (!Number.isFinite(rate)) fail("intraday throughput is not finite"); return rate; })),
      };
    };
    const report = {
      schema: "runtime-sync-policy-report", schema_version: 1,
      status: "PASS_BATCH",
      batch_index: batchIndex,
      planned_batches: [1, 2, 3, 4, 5],
      config_sha256: configSha256,
      binary_sha256: binarySha256,
      setup_sha256: setupSha256,
      source_revision: config.source_revision,
      config: { ...config, setup_json: undefined },
      binary_identity: beforeIdentity.binary,
      setup_identity: beforeIdentity.setup,
      setup_input_json: setupText,
      cpu_affinity: config.cpu_affinity,
      runtime_environment: runtimeEnvironment,
      cpu_load: { before: loadBefore, after: load() },
      interference_declaration: "taskset 将两臂限制在同一 CPU 集合；未停止其他负载。报告 load average 与 runnable 线程证据，不据 CPU 百分比宣称提速。",
      pure_flush_wall_ns: null,
      pure_flush_availability: "unavailable",
      contract_metrics_complete: false,
      measurement_gaps: ["N/2N 工作量增长斜率：本campaign固定 trading_days，尚未采集。", "PlanBook/覆盖表与保留事实 serialized bytes（不代表 resident allocation）：尚未采集。", "flush前 driver 覆盖表实载峰计数：尚未采集。", "纯自动 flush 独立 wall：没有公开独立读面，不可用。"],
      throughput_scope: "total_ticks_per_second 使用 completed_ticks/run_wall_ns（含 civil update）；intraday_steps_per_second 使用非交易日终 step 数/intraday_wall_ns。初始化独立报告，不混入run_wall_ns。",
      calendar_signature: JSON.parse(allCalendarSignatures[0]),
      combined_boundary_wall_ns: { per_policy: { every_tick: combinedBoundaryStats("every_tick"), day_end: combinedBoundaryStats("day_end") } },
      planned_round_count: config.rounds,
      warmup_round_count: counts.warmup_rounds,
      warmup_sample_count: counts.warmup_samples,
      round_count: counts.measured_rounds,
      sample_count: counts.measured_samples,
      measured_sample_count: counts.measured_samples,
      campaign_process_count: counts.warmup_samples + counts.measured_samples,
      total_completed_ticks: samples.reduce((total, sample) => total + BigInt(JSON.parse(sample.stdout).completed_ticks), 0n).toString(),
      measured_sample_order: measured.map(({ policy }) => policy),
      policy_summary: { every_tick: summarizePolicy("every_tick"), day_end: summarizePolicy("day_end") },
      samples,
    };
    const reportBytes = `${JSON.stringify(report, null, 2)}\n`;
    if (clock() >= executionDeadlineAt) fail("runtime sync policy batch exhausted deadline before report publication");
    if (typeof outputDirectory === "string") {
      if (!path.isAbsolute(outputDirectory)) fail("output_directory must be absolute");
      if (clock() >= executionDeadlineAt) fail("runtime sync policy batch exhausted deadline before report publication");
      const reportPath = path.join(outputDirectory, "runtime-sync-policy-report.json");
      const partialPath = `${reportPath}.partial`;
      try { await writeBeforeDeadline(partialPath, reportBytes, { flag: "wx" }); }
      catch (error) {
        if (clock() >= executionDeadlineAt) {
          try { await unlinkFile(partialPath); }
          catch (cleanupError) { throw new AggregateError([error, cleanupError], `报告暂存写入越过执行期限且清理失败：${error.message}；${cleanupError.message}`, { cause: error }); }
        }
        throw error;
      }
      if (clock() >= executionDeadlineAt) { await unlinkFile(partialPath); fail("runtime sync policy batch exhausted deadline before report publication"); }
      await renameFile(partialPath, reportPath);
      if (clock() >= executionDeadlineAt) { await unlinkFile(reportPath); fail("runtime sync policy batch exhausted deadline during report publication"); }
    }
    return report;
  } catch (error) {
    abortActive();
    try { await terminated; }
    catch (terminationError) {
      if (error instanceof AggregateError && error.errors.includes(terminationError)) throw error;
      throw new AggregateError([error, terminationError], `batch failed and process-tree termination also failed: ${error.message}; ${terminationError.message}`, { cause: error });
    }
    throw error;
  } finally {
    clearDeadlineTimer(timer);
  }
}

export function aggregateRuntimeSyncPolicy(reports) {
  if (!Array.isArray(reports) || reports.length !== 5) fail("aggregate requires exactly five batch reports");
  const byIndex = new Map();
  const validatedByBatch = new Map();
  for (const report of reports) {
    if (!isRecord(report) || report.schema !== "runtime-sync-policy-report" || report.schema_version !== 1) fail("aggregate contains an unsupported batch report");
    if (report.status !== "PASS_BATCH") fail("aggregate accepts only PASS_BATCH reports");
    if (!Number.isSafeInteger(report.batch_index) || report.batch_index < 1 || report.batch_index > 5 || byIndex.has(report.batch_index)) fail("aggregate batch indices must be unique values from 1 through 5");
    const expectedSampleOrder = ["every_tick", "day_end", "every_tick", "day_end", "day_end", "every_tick"];
    if (!isDeepStrictEqual(report.planned_batches, [1, 2, 3, 4, 5]) || report.planned_round_count !== 5
      || report.warmup_round_count !== 1 || report.warmup_sample_count !== 2 || report.round_count !== 1 || report.sample_count !== 4
      || !isDeepStrictEqual(report.measured_sample_order, expectedSampleOrder.slice(2))
      || !Array.isArray(report.samples) || report.samples.length !== 6
      || report.samples.some((sample, index) => sample.phase !== (index < 2 ? "warmup" : "measured") || sample.policy !== expectedSampleOrder[index])) fail("aggregate batch sample counts/order do not match the five-batch campaign");
    for (const field of ["config_sha256", "binary_sha256", "setup_sha256"]) {
      if (typeof report[field] !== "string" || !/^[a-f0-9]{64}$/.test(report[field])) fail(`aggregate ${field} must be a SHA-256 digest`);
    }
    if (typeof report.calendar_signature !== "object" || report.calendar_signature === null) fail("aggregate calendar signature is missing");
    if (typeof report.setup_input_json !== "string" || digest(Buffer.from(report.setup_input_json)) !== report.setup_sha256) fail("aggregate setup input does not match setup SHA-256");
    const config = { ...report.config, setup_json: report.setup_input_json };
    const expectedCalendar = [];
    const validatedSamples = [];
    for (const [index, sample] of report.samples.entries()) {
      const measurement = {
        wall_ns: sample.wall_ns,
        peak_process_tree_rss_bytes: sample.peak_process_tree_rss_bytes,
        process_tree_thread_state: sample.process_tree_thread_state,
        stdout: sample.stdout,
        stderr: sample.stderr,
      };
      const validated = validateRuntimeSyncSample(measurement, config, expectedSampleOrder[index], `aggregate batch ${report.batch_index} sample ${index + 1}`);
      for (const field of ["run_wall_ns", "intraday_wall_ns", "initialization_wall_ns", "closed_civil_wall_ns"]) {
        if (sample[field] !== validated.sample[field]) fail(`aggregate sample ${field} differs from raw sample JSON`);
      }
      if (!isDeepStrictEqual(sample.boundary_wall_ns, validated.sample.boundary_wall_ns)) fail("aggregate boundary wall metadata differs from raw sample JSON");
      validatedSamples.push({ phase: sample.phase, round: sample.round, position: sample.position, policy: sample.policy, measurement, sample: validated.sample });
      const signature = { trading_day_boundaries: validated.sample.trading_day_boundaries.map(({ settled_date, next_date }) => ({ settled_date, next_date })), final_civil_date: validated.sample.final_civil_date, final_phase: validated.sample.final_phase, next_day_first_tick_confirmed: validated.sample.next_day_first_tick_confirmed };
      expectedCalendar.push(stableJson(signature));
      if (!isDeepStrictEqual(sample.calendar_signature, signature)) fail("aggregate calendar signature differs from raw sample JSON");
    }
    if (new Set(expectedCalendar).size !== 1 || !isDeepStrictEqual(report.calendar_signature, JSON.parse(expectedCalendar[0]))) fail("aggregate batch raw sample calendar signatures differ");
    exactKeys(report.runtime_environment, ["ENGINE_STEP_RUNTIME", "ENGINE_SPIN_US", "ENGINE_SPIN_DISABLE", "STEP_PROFILE"], "aggregate runtime_environment");
    if (report.runtime_environment.ENGINE_STEP_RUNTIME !== "1" || ["ENGINE_SPIN_US", "ENGINE_SPIN_DISABLE", "STEP_PROFILE"].some((key) => report.runtime_environment[key] !== null && typeof report.runtime_environment[key] !== "string")) fail("aggregate runtime_environment is invalid");
    const configForHash = { ...config, runtime_environment: report.runtime_environment };
    delete configForHash.setup_json;
    delete configForHash.output_directory;
    if (digest(stableJson(configForHash)) !== report.config_sha256) fail("aggregate config SHA-256 does not match the reported config and runtime environment");
    byIndex.set(report.batch_index, report);
    validatedByBatch.set(report.batch_index, validatedSamples);
  }
  if ([1, 2, 3, 4, 5].some((index) => !byIndex.has(index))) fail("aggregate requires exactly five unique batches");
  const ordered = [1, 2, 3, 4, 5].map((index) => byIndex.get(index));
  const rawWindows = [1, 2, 3, 4, 5].map((index) => validatedByBatch.get(index));
  const baseline = ordered[0];
  const sameFields = ["config_sha256", "binary_sha256", "setup_sha256", "source_revision"];
  for (const report of ordered.slice(1)) {
    if (sameFields.some((field) => report[field] !== baseline[field])
      || !isDeepStrictEqual(report.cpu_affinity, baseline.cpu_affinity)
      || report.config.seed !== baseline.config.seed || report.config.trading_days !== baseline.config.trading_days
      || report.binary_identity !== baseline.binary_identity || report.setup_identity !== baseline.setup_identity
      || !isDeepStrictEqual(report.calendar_signature, baseline.calendar_signature)
      || !isDeepStrictEqual(report.runtime_environment, baseline.runtime_environment)) fail("aggregate batches do not share the same campaign, source, binary, setup, CPU, runtime environment, seed, trading days and calendar");
  }
  return {
    schema: "runtime-sync-policy-aggregate", schema_version: 1,
    status: "COMPLETE_PAIRED_DATA",
    completed_batches: [1, 2, 3, 4, 5],
    warmup_sample_count: 10,
    measured_sample_count: 20,
    campaign_process_count: 30,
    total_completed_ticks: rawWindows.flatMap((samples) => samples).reduce((total, item) => total + BigInt(item.sample.completed_ticks), 0n).toString(),
    planned_batches: [1, 2, 3, 4, 5],
    config_sha256: baseline.config_sha256,
    binary_sha256: baseline.binary_sha256,
    setup_sha256: baseline.setup_sha256,
    source_revision: baseline.source_revision,
    calendar_signature: baseline.calendar_signature,
    combined_boundary_wall_ns: { per_policy: Object.fromEntries(["every_tick", "day_end"].map((policy) => [policy, {
      ...stats(rawWindows.flatMap((samples) => samples.filter((entry) => entry.phase === "measured" && entry.policy === policy).flatMap((entry) => entry.sample.boundary_wall_ns.map(BigInt)))),
      by_trading_day: Array.from({ length: baseline.config.trading_days }, (_, day) => ({
        trading_day: day + 1,
        ...stats(ordered.flatMap((report) => report.samples.filter((sample) => sample.phase === "measured" && sample.policy === policy).map((sample) => BigInt(sample.boundary_wall_ns[day])))),
      })),
    }])) },
    seed: baseline.config.seed,
    trading_days: baseline.config.trading_days,
    cpu_affinity: baseline.cpu_affinity,
    contract_metrics_complete: false,
    measurement_gaps: baseline.measurement_gaps,
    runtime_environment: baseline.runtime_environment,
    windows: ordered,
    interpretation_limit: "五个 batch 是分别受监督的时间窗口；仅保留各窗口的原始配对结果，不合并成单一连续 300 秒测量，也不据 CPU 百分比宣称提速。",
  };
}

export async function main(argv = process.argv.slice(2), dependencies = {}) {
  const [command, ...args] = argv;
  if (command === "batch") {
    if (args.length !== 3 || !/^[1-5]$/.test(args[1])) fail("usage: runtime-sync-policy.mjs batch <config.json> <1..5> <new-output-dir>");
    const configPath = path.resolve(args[0]);
    const batchIndex = Number(args[1]);
    const outputDirectory = path.resolve(args[2]);
    const config = JSON.parse(await fs.readFile(configPath, "utf8"));
    config.output_directory = outputDirectory;
    return runRuntimeSyncPolicy(config, { ...dependencies, batchIndex });
  }
  if (command === "aggregate") {
    if (args.length !== 6) fail("usage: runtime-sync-policy.mjs aggregate <new-report.json> <batch1-report> <batch2-report> <batch3-report> <batch4-report> <batch5-report>");
    const outputPath = path.resolve(args[0]);
    const reports = await Promise.all(args.slice(1).map(async (file) => JSON.parse(await fs.readFile(path.resolve(file), "utf8"))));
    const aggregate = aggregateRuntimeSyncPolicy(reports);
    await fs.writeFile(outputPath, `${JSON.stringify(aggregate, null, 2)}\n`, { flag: "wx" });
    return aggregate;
  }
  fail("usage: runtime-sync-policy.mjs <batch|aggregate> ...");
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    process.stderr.write(`${error.stack ?? error.message}\n`);
    process.exitCode = 1;
  });
}
