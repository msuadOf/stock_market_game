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
function bindCapturedChild({ command, args, options, spawnProcess, terminate, controller, env, deadlineAt, clock, onStdout, onStderr, onTermination }) {
  const state = { child: undefined, capture: { stdout: "", stderr: "" }, terminated: Promise.resolve() };
  state.child = spawnProcess(command, args, { ...options, env });
  state.child.stdout?.on("data", (chunk) => { state.capture.stdout += chunk.toString(); onStdout(chunk); });
  state.child.stderr?.on("data", (chunk) => { state.capture.stderr += chunk.toString(); onStderr(chunk); });
  controller.signal.addEventListener("abort", () => {
    state.terminated = Promise.resolve(terminate(state.child, false, Date.now() + Math.max(1, deadlineAt - clock())));
    onTermination(state.terminated);
  }, { once: true });
  return state;
}

function stableJson(value) {
  if (Array.isArray(value)) return `[${value.map(stableJson).join(",")}]`;
  if (isRecord(value)) return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableJson(value[key])}`).join(",")}}`;
  return JSON.stringify(value);
}

async function collectProcessSample({ endpoint, config, runSample, spawnProcess, terminate, controller, envSnapshot, deadlineAt, executionDeadlineAt, clock, openFile, outputPrefix, onCapture, onTermination }) {
  let stdoutHandle;
  let stderrHandle;
  let streamed = false;
  let capture = { stdout: "", stderr: "" };
  let rawWriteError;
  let pendingWrites = Promise.resolve();
  let terminated = Promise.resolve();
  let rawPersisted = false;
  const queueWrite = (handle, chunk) => {
    if (handle === undefined) return;
    pendingWrites = pendingWrites.then(async () => {
      if (rawWriteError !== undefined) throw rawWriteError;
      await handle.writeFile(chunk);
    }).catch((error) => { rawWriteError = error; if (!controller.signal.aborted) controller.abort(error); });
  };
  const spawnBound = (command, args, options) => {
    if (controller.signal.aborted) throw controller.signal.reason;
    const state = bindCapturedChild({
      command, args, options, spawnProcess, terminate, controller,
      env: { ...envSnapshot, ENGINE_STEP_RUNTIME: "1" }, deadlineAt, clock,
      onStdout: (chunk) => { streamed = true; capture = state.capture; onCapture?.(capture); queueWrite(stdoutHandle, chunk); },
      onStderr: (chunk) => { streamed = true; capture = state.capture; onCapture?.(capture); queueWrite(stderrHandle, chunk); },
      onTermination: (promise) => { terminated = promise; onTermination?.(promise); },
    });
    capture = state.capture;
    onCapture?.(capture);
    return state.child;
  };
  const evidence = () => ({ capture: { ...capture }, rawPersisted });
  let primary;
  let measured;
  try {
    if (controller.signal.aborted) throw controller.signal.reason;
    if (outputPrefix !== undefined) {
      stdoutHandle = await openFile(`${outputPrefix}.stdout`, "wx");
      try { stderrHandle = await openFile(`${outputPrefix}.stderr`, "wx"); }
      catch (error) {
        try { await stdoutHandle.close(); }
        catch (closeError) { throw new AggregateError([error, closeError], `stderr raw open failed and stdout raw close also failed: ${error.message}; ${closeError.message}`, { cause: error }); }
        finally { stdoutHandle = undefined; }
        throw error;
      }
      rawPersisted = true;
    }
    if (controller.signal.aborted) throw controller.signal.reason;
    measured = await runSample(endpoint, {
      rssSampleIntervalMs: config.sample_interval_ms,
      signal: controller.signal,
      spawn: spawnBound,
      terminate: async (child) => terminate(child, false, Date.now() + Math.max(1, deadlineAt - clock())),
    });
    if (clock() >= executionDeadlineAt) {
      if (!controller.signal.aborted) controller.abort(new Error("sample completed beyond execution deadline"));
      fail("sample completed beyond the execution deadline");
    }
    if (controller.signal.aborted) fail("sample was cancelled by the shared deadline");
    if (outputPrefix !== undefined && !streamed) {
      capture = { stdout: measured.stdout, stderr: measured.stderr };
      onCapture?.(capture);
      await Promise.all([stdoutHandle.writeFile(measured.stdout), stderrHandle.writeFile(measured.stderr)]);
    }
    await pendingWrites;
    if (rawWriteError !== undefined) throw rawWriteError;
    if (stdoutHandle !== undefined || stderrHandle !== undefined) {
      await Promise.all([stdoutHandle?.close(), stderrHandle?.close()]);
      stdoutHandle = undefined; stderrHandle = undefined;
    }
    return { measured, evidence: evidence() };
  } catch (error) {
    primary = error;
    const cleanupErrors = [];
    if (controller.signal.aborted) {
      try { await terminated; } catch (terminationError) { cleanupErrors.push(terminationError); }
    }
    try { await pendingWrites; } catch (writeError) { cleanupErrors.push(writeError); }
    if (rawWriteError !== undefined && !cleanupErrors.includes(rawWriteError)) cleanupErrors.push(rawWriteError);
    if (stdoutHandle !== undefined || stderrHandle !== undefined) {
      try { await Promise.all([stdoutHandle?.close(), stderrHandle?.close()]); }
      catch (closeError) { cleanupErrors.push(closeError); }
      stdoutHandle = undefined; stderrHandle = undefined;
    }
    const combined = cleanupErrors.length === 0 ? primary
      : new AggregateError([primary, ...cleanupErrors], `sample failed and process/raw cleanup also failed: ${[primary, ...cleanupErrors].map((entry) => entry.message).join("; ")}`, { cause: primary });
    Object.defineProperty(combined, "sampleEvidence", { value: evidence(), configurable: true });
    throw combined;
  }
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
function hasConfiguredSubset(expected, actual, root = true) {
  if (Array.isArray(expected)) return Array.isArray(actual) && expected.length === actual.length
    && expected.every((entry, index) => hasConfiguredSubset(entry, actual[index], false));
  if (isRecord(expected)) return isRecord(actual)
    && Object.entries(expected).every(([key, value]) => {
      if (Object.hasOwn(actual, key)) return hasConfiguredSubset(value, actual[key], false);
      if (value === null) return true;
      return root && value === false && ["rights_offering_enabled", "issuer_repurchase_enabled", "auto_corporate_foundation"].includes(key);
    });
  return isDeepStrictEqual(expected, actual);
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
  const runtimeEnvironment = { ENGINE_STEP_RUNTIME: childEnvironmentSnapshot.ENGINE_STEP_RUNTIME ?? "1", STEP_PROFILE: childEnvironmentSnapshot.STEP_PROFILE ?? null };
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
    let measuredResult;
    let capture = activeCapture;
    let rawPersisted = false;
    try {
      const outputPrefix = outputDirectory === undefined ? undefined : samplePrefix(outputDirectory, batchIndex, { phase, round, position, policy });
      rawPersisted = outputPrefix !== undefined;
      const collected = await collectProcessSample({
        endpoint, config, runSample, spawnProcess, terminate, controller, envSnapshot: childEnvironmentSnapshot,
        deadlineAt, executionDeadlineAt, clock, openFile, outputPrefix,
        onCapture: (value) => { capture = value; activeCapture = value; },
        onTermination: (promise) => { terminated = promise; },
      });
      const measured = collected.measured;
      measuredResult = measured;
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
      if (controller.signal.aborted && error.sampleEvidence === undefined) {
        try { await terminated; } catch (terminationError) { cleanupErrors.push(terminationError); }
      }
      const evidence = error.sampleEvidence ?? { capture, rawPersisted };
      if (outputDirectory !== undefined && clock() < executionDeadlineAt && !samples.some((sample) => sample.phase === phase && sample.round === round && sample.position === position)) {
        try { await persistFailure(outputDirectory, batchIndex, { phase, round, position, policy, error }, writeBeforeDeadline, measuredResult ?? evidence.capture, !evidence.rawPersisted); }
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
    exactKeys(report.runtime_environment, ["ENGINE_STEP_RUNTIME", "STEP_PROFILE"], "aggregate runtime_environment");
    if (report.runtime_environment.ENGINE_STEP_RUNTIME !== "1" || (report.runtime_environment.STEP_PROFILE !== null && typeof report.runtime_environment.STEP_PROFILE !== "string")) fail("aggregate runtime_environment is invalid");
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
  if (command === "memory-growth") {
    if (args.length !== 2) fail("usage: runtime-sync-policy.mjs memory-growth <config.json> <new-output-dir>");
    const configPath = path.resolve(args[0]);
    const outputDirectory = path.resolve(args[1]);
    const config = JSON.parse(await fs.readFile(configPath, "utf8"));
    config.output_directory = outputDirectory;
    return runRuntimeSyncMemoryGrowth(config, dependencies);
  }
  if (command === "aggregate") {
    if (args.length !== 6) fail("usage: runtime-sync-policy.mjs aggregate <new-report.json> <batch1-report> <batch2-report> <batch3-report> <batch4-report> <batch5-report>");
    const outputPath = path.resolve(args[0]);
    const reports = await Promise.all(args.slice(1).map(async (file) => JSON.parse(await fs.readFile(path.resolve(file), "utf8"))));
    const aggregate = aggregateRuntimeSyncPolicy(reports);
    await fs.writeFile(outputPath, `${JSON.stringify(aggregate, null, 2)}\n`, { flag: "wx" });
    return aggregate;
  }
  fail("usage: runtime-sync-policy.mjs <batch|memory-growth|aggregate> ...");
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    process.stderr.write(`${error.stack ?? error.message}\n`);
    process.exitCode = 1;
  });
}

const MEMORY_DOMAINS = {
  save_slot: ["serialized_bytes"],
  plans: ["serialized_bytes", "entry_count", "terminal_entry_count", "owner_sequence_count"],
  retained_market_days: ["serialized_bytes", "entry_count"],
  personal_trade_confirmations: ["serialized_bytes", "account_count", "entry_count"],
  filled_orders: ["serialized_bytes", "stock_count", "entry_count"],
  snapshot: ["serialized_bytes", "account_count", "market_count", "daily_candle_count"],
  runtime_state: ["serialized_bytes", "live_envelope_count", "retail_projection_seen_count", "strategy_state_count"],
};

export function validateRuntimeSyncMemoryGrowthConfig(config) {
  if (!isRecord(config)) fail("memory-growth config must be an object");
  const required = ["schema", "schema_version", "binary", "cwd", "setup_file", "seed", "civil_days_n", "policy", "expected_closed_dates", "cpu_affinity", "source_revision"];
  for (const key of required) if (!(key in config)) fail(`memory-growth config is missing ${key}`);
  const allowed = [...required, "sample_interval_ms", "deadline_ms", "output_directory"];
  for (const key of Object.keys(config)) if (!allowed.includes(key)) fail(`memory-growth config has unknown key ${key}`);
  if (config.schema !== "runtime-sync-memory-growth-config" || config.schema_version !== 1) fail("memory-growth config schema is unsupported");
  for (const key of ["binary", "cwd", "setup_file"]) if (typeof config[key] !== "string" || !path.isAbsolute(config[key])) fail(`${key} must be absolute`);
  canonicalU64(config.seed, "seed");
  positiveSafeInteger(config.civil_days_n, "civil_days_n");
  if (config.civil_days_n > Math.floor(Number.MAX_SAFE_INTEGER / 2)) fail("civil_days_n makes 2N exceed safe integer range");
  if (!["every_tick", "day_end"].includes(config.policy)) fail("policy must be every_tick or day_end");
  if (!Array.isArray(config.expected_closed_dates) || config.expected_closed_dates.length > 2 * config.civil_days_n
    || !config.expected_closed_dates.every((date, index, dates) => isDate(date) && (index === 0 || dates[index - 1] < date))) fail("expected_closed_dates must be a sorted unique ISO subset of at most 2N civil days");
  if (!Array.isArray(config.cpu_affinity) || config.cpu_affinity.length === 0
    || !config.cpu_affinity.every((cpu) => Number.isSafeInteger(cpu) && cpu >= 0)
    || new Set(config.cpu_affinity).size !== config.cpu_affinity.length) fail("cpu_affinity must be a non-empty list of unique Linux CPU numbers");
  if (typeof config.source_revision !== "string" || !/^[a-f0-9]{40}$/.test(config.source_revision)) fail("source_revision must be a full lowercase 40-hex revision");
  if (config.output_directory !== undefined && (typeof config.output_directory !== "string" || !path.isAbsolute(config.output_directory))) fail("output_directory must be absolute");
  const sampleInterval = config.sample_interval_ms ?? 5;
  const deadline = config.deadline_ms ?? MAX_DEADLINE_MS;
  if (!Number.isSafeInteger(sampleInterval) || sampleInterval < 1) fail("sample_interval_ms must be an integer >= 1");
  if (!Number.isSafeInteger(deadline) || deadline <= CLEANUP_RESERVE_MS || deadline > MAX_DEADLINE_MS) fail(`deadline_ms must be > ${CLEANUP_RESERVE_MS} and <= ${MAX_DEADLINE_MS}`);
  return { ...config, sample_interval_ms: sampleInterval, deadline_ms: deadline };
}

function validateMemoryGrowthSample(measured, config, setup, label = "memory-growth sample") {
  exactKeys(measured, ["wall_ns", "peak_process_tree_rss_bytes", "process_tree_thread_state", "stdout", "stderr"], `${label} measurement`);
  positiveDecimal(measured.wall_ns, `${label} wall_ns`);
  positiveSafeInteger(measured.peak_process_tree_rss_bytes, `${label} process-tree peak RSS`);
  validateThreadState(measured.process_tree_thread_state, label);
  if (typeof measured.stdout !== "string" || typeof measured.stderr !== "string") fail(`${label} stdout/stderr must be strings`);
  let sample;
  try { sample = JSON.parse(measured.stdout); } catch (error) { fail(`${label} stdout is not sample JSON: ${error.message}`); }
  if (!isRecord(sample) || sample.schema !== "runtime-sync-memory-growth-sample" || sample.schema_version !== 1) fail(`${label} schema is unsupported`);
  if (sample.policy !== config.policy || sample.seed !== config.seed || sample.civil_days_n !== config.civil_days_n) fail(`${label} policy/seed/civil_days_n mismatch`);
  if (!isRecord(sample.setup) || !isRecord(sample.effective_setup)) fail(`${label} setup/effective_setup missing`);
  if (sample.setup.runtime_sync_policy !== config.policy) fail(`${label} source setup policy mismatch`);
  if (!isDeepStrictEqual(normalizeSetup(sample.setup, label), normalizeSetup(setup, "configured"))) fail(`${label} setup differs from input setup`);
  if (sample.effective_setup.runtime_sync_policy !== config.policy
    || !hasConfiguredSubset({ ...sample.setup, runtime_sync_policy: config.policy }, sample.effective_setup)) fail(`${label} effective_setup differs from configured setup facts`);
  for (const field of ["npc_count", "stocks", "completed_trading_days", "completed_civil_days", "completed_ticks"]) {
    if (!Number.isSafeInteger(sample[field]) || sample[field] < 0) fail(`${label} ${field} must be a non-negative integer`);
  }
  if (sample.completed_civil_days !== 2 * config.civil_days_n || sample.completed_trading_days + sample.closed_dates?.length !== sample.completed_civil_days) fail(`${label} completed civil/trading day counts mismatch`);
  if (!Array.isArray(sample.civil_day_boundaries) || sample.civil_day_boundaries.length !== 2 * config.civil_days_n) fail(`${label} civil-day boundary count mismatch`);
  if (!Array.isArray(sample.closed_dates) || !isDeepStrictEqual(sample.closed_dates, config.expected_closed_dates)) fail(`${label} closed_dates differ from pre-registered dates`);
  const startDate = setup.start_date;
  if (!isDate(startDate)) fail(`${label} configured setup.start_date is invalid`);
  const endDate = addCivilDays(startDate, 2 * config.civil_days_n);
  if (endDate === startDate || !isDate(endDate)) fail(`${label} civil-day window exceeds supported ISO date range`);
  if (config.expected_closed_dates.some((date) => date < startDate || date >= endDate)) fail(`${label} expected_closed_dates contains a date outside the run window`);
  const ticksPerDay = setup.ticks_per_day;
  positiveSafeInteger(ticksPerDay, `${label} setup.ticks_per_day`);
  let priorTick = 0;
  let priorSeq = 0;
  for (const [index, boundary] of sample.civil_day_boundaries.entries()) {
    if (!isRecord(boundary) || !isDate(boundary.settled_date) || !isDate(boundary.next_date)
      || boundary.settled_date !== addCivilDays(startDate, index) || boundary.next_date !== addCivilDays(boundary.settled_date, 1)
      || !Number.isSafeInteger(boundary.tick) || boundary.tick < 0 || !Number.isSafeInteger(boundary.seq) || boundary.seq < priorSeq
      || !["intraday_trading", "closed_day"].includes(boundary.settled_phase)) fail(`${label} civil-day boundary ${index} is invalid or discontinuous`);
    if ((boundary.settled_phase === "closed_day") !== config.expected_closed_dates.includes(boundary.settled_date)) fail(`${label} boundary closed phase differs from pre-registered dates`);
    const expectedTick = priorTick + (boundary.settled_phase === "intraday_trading" ? ticksPerDay : 0);
    if (boundary.tick !== expectedTick) fail(`${label} boundary ${index} tick does not advance by the configured civil-day policy`);
    priorTick = boundary.tick;
    priorSeq = boundary.seq;
  }
  const finalBoundary = sample.civil_day_boundaries.at(-1);
  if (sample.final_civil_date !== finalBoundary.next_date || sample.final_phase !== "intraday_trading" && sample.final_phase !== "closed_day") fail(`${label} final civil date/phase mismatch`);
  if (sample.completed_ticks !== finalBoundary.tick || sample.last_tick !== finalBoundary.tick || sample.final_seq !== finalBoundary.seq
    || sample.completed_trading_days !== sample.civil_day_boundaries.filter((entry) => entry.settled_phase === "intraday_trading").length) fail(`${label} completed tick/trading-day count differs from boundaries`);
  if (sample.shutdown_confirmed !== true || !isRecord(sample.shutdown)) fail(`${label} shutdown was not confirmed`);
  if (!isRecord(sample.topology) || sample.topology.groups !== 1
    || sample.topology.desks !== sample.setup.stocks?.length
    || sample.stocks !== sample.setup.stocks?.length
    || sample.npc_count !== sample.setup.npcs?.retail_count + sample.setup.npcs?.inst_count + sample.setup.npcs?.hot_count) fail(`${label} topology/stocks/npc counts mismatch`);
  if (sample.run_wall_ns === undefined || sample.initialization_wall_ns === undefined) fail(`${label} initialization/run timing is missing`);
  positiveDecimal(sample.run_wall_ns, `${label} run_wall_ns`);
  positiveDecimal(sample.initialization_wall_ns, `${label} initialization_wall_ns`);
  if (sample.pure_flush_wall_ns !== null || sample.pure_flush_availability !== "unavailable"
    || sample.coverage_table_peak_count !== null || sample.coverage_table_peak_availability !== "unavailable") fail(`${label} unavailable instrumentation fields are inconsistent`);
  if (typeof sample.measurement_effects !== "string" || sample.measurement_effects.length === 0) fail(`${label} measurement effects declaration is missing`);
  if (sample.shutdown.runtime === null) {
    if (sample.completed_ticks !== 0) fail(`${label} nonzero-tick run lacks runtime shutdown receipts`);
  } else {
    const runtime = sample.shutdown.runtime;
    const workerCount = sample.topology.groups + sample.topology.desks;
    if (!isRecord(runtime) || runtime.owned_workers !== workerCount || runtime.joined_workers !== workerCount
      || runtime.group_finals !== sample.topology.groups || runtime.desk_finals !== sample.topology.desks
      || !Array.isArray(runtime.worker_results) || runtime.worker_results.length !== workerCount
      || runtime.worker_results.some((entry) => !isRecord(entry) || !isRecord(entry.result)
        || Object.keys(entry.result).length !== 1 || entry.result.Ok !== null)) fail(`${label} runtime worker shutdown receipts are invalid`);
  }
  if (!Array.isArray(sample.observations) || sample.observations.length !== 2) fail(`${label} must contain exactly two observations`);
  const points = [config.civil_days_n, 2 * config.civil_days_n];
  for (let i = 0; i < 2; i += 1) {
    const observation = sample.observations[i];
    if (!isRecord(observation) || observation.civil_day_count !== points[i]) fail(`${label} observation ${i} is not at the registered N/2N point`);
    const boundary = sample.civil_day_boundaries[points[i] - 1];
    for (const key of ["settled_date", "tick", "seq"]) if (observation[key] !== boundary[key]) fail(`${label} observation ${i} ${key} differs from its boundary`);
    if (observation.completed_trading_days !== sample.civil_day_boundaries.slice(0, points[i]).filter((entry) => entry.settled_phase === "intraday_trading").length
      || observation.closed_civil_days !== sample.civil_day_boundaries.slice(0, points[i]).filter((entry) => entry.settled_phase === "closed_day").length) fail(`${label} observation ${i} day counts mismatch`);
    if (observation.candidate_lifetime !== "captured_clone_alive_update_dropped") fail(`${label} observation ${i} candidate lifetime mismatch`);
    positiveSafeInteger(observation.rss_before_serialization_bytes, `${label} observation RSS before serialization`);
    positiveSafeInteger(observation.rss_after_serialization_bytes, `${label} observation RSS after serialization`);
    if (observation.process_tree_process_count !== 1 || observation.rss_source !== "linux_proc_self_status_vmrss_all_tasks_children_checked" || observation.serialized_bytes_are_resident_allocation !== false) fail(`${label} observation process/RSS attribution mismatch`);
    exactKeys(observation.domains, Object.keys(MEMORY_DOMAINS), `${label} observation domains`);
    for (const [domain, fields] of Object.entries(MEMORY_DOMAINS)) {
      exactKeys(observation.domains[domain], fields, `${label} observation ${domain}`);
      canonicalU64(observation.domains[domain].serialized_bytes, `${label} ${domain}.serialized_bytes`);
      for (const field of fields.slice(1)) if (!Number.isSafeInteger(observation.domains[domain][field]) || observation.domains[domain][field] < 0) fail(`${label} ${domain}.${field} must be a non-negative integer`);
    }
    if (observation.domains.plans.terminal_entry_count > observation.domains.plans.entry_count) fail(`${label} plans terminal entries exceed total entries`);
  }
  return { sample, measurement: measured };
}

function addCivilDays(date, count) {
  const value = new Date(`${date}T00:00:00.000Z`);
  value.setUTCDate(value.getUTCDate() + count);
  return value.toISOString().slice(0, 10);
}

function growthDelta(firstObservation, secondObservation, firstDomain, secondDomain, field) {
  const delta = BigInt(secondDomain[field]) - BigInt(firstDomain[field]);
  const ticks = secondObservation.tick - firstObservation.tick;
  return { delta: delta.toString(), delta_ticks: ticks, per_tick_slope: ticks === 0 ? { availability: "unavailable", value: null } : { availability: "available", value: `${delta}/${ticks}` } };
}

export async function runRuntimeSyncMemoryGrowth(inputConfig, dependencies = {}) {
  const { runSample = runProcessSample, clock = () => performance.now(), readFile = (file) => fs.readFile(file), writeFile = (...args) => fs.writeFile(...args), openFile = (...args) => fs.open(...args), mkdir = (...args) => fs.mkdir(...args), renameFile = (...args) => fs.rename(...args), unlinkFile = (...args) => fs.unlink(...args), load = defaultLoad, spawnProcess = spawn, terminate = terminateTree, setDeadlineTimer = setTimeout, clearDeadlineTimer = clearTimeout } = dependencies;
  if (process.platform !== "linux") fail("memory-growth runner requires Linux taskset and /proc sampling");
  const config = validateRuntimeSyncMemoryGrowthConfig(inputConfig);
  const envSnapshot = { ...process.env };
  if ((envSnapshot.ENGINE_STEP_RUNTIME ?? "1") !== "1") fail("ENGINE_STEP_RUNTIME 必须为 1 或未设置，以确保 native runtime 实际启用");
  const startedAt = clock();
  const deadlineAt = startedAt + config.deadline_ms;
  const executionDeadlineAt = deadlineAt - CLEANUP_RESERVE_MS;
  if (config.output_directory !== undefined) {
    await mkdir(config.output_directory, { recursive: false });
    await mkdir(path.join(config.output_directory, "samples"), { recursive: false });
  }
  if (clock() >= executionDeadlineAt) fail("memory-growth deadline exhausted before input fingerprinting");
  const [setupBytes, binaryBytes] = await Promise.all([readFile(config.setup_file), readFile(config.binary)]);
  const setup = JSON.parse(Buffer.isBuffer(setupBytes) ? setupBytes.toString("utf8") : String(setupBytes));
  if (!isRecord(setup) || !isDate(setup.start_date)) fail("memory-growth setup must include valid start_date");
  if (config.civil_days_n > Math.floor((Date.UTC(9999, 11, 31) - Date.parse(`${setup.start_date}T00:00:00.000Z`)) / 86400000 / 2)) fail("memory-growth civil-day window exceeds supported ISO date range");
  const civilEndDate = addCivilDays(setup.start_date, 2 * config.civil_days_n);
  if (!isDate(civilEndDate) || config.expected_closed_dates.some((date) => date < setup.start_date || date >= civilEndDate)) fail("memory-growth expected_closed_dates falls outside the setup civil-day window");
  if (clock() >= executionDeadlineAt) fail("memory-growth deadline exhausted during input fingerprinting");
  const runtimeEnvironment = { ENGINE_STEP_RUNTIME: "1", STEP_PROFILE: envSnapshot.STEP_PROFILE ?? null };
  const configForHash = { ...config, runtime_environment: runtimeEnvironment };
  delete configForHash.output_directory;
  const configSha256 = digest(stableJson(configForHash));
  const cpuList = config.cpu_affinity.join(",");
  const endpoint = { command: ["taskset", "-c", cpuList, config.binary, "memory-growth", config.setup_file, config.policy, config.seed, String(config.civil_days_n)], cwd: config.cwd, source_fingerprint: digest(binaryBytes) };
  const loadBefore = load();
  const item = { policy: config.policy };
  const prefix = config.output_directory === undefined ? undefined : path.join(config.output_directory, "samples", `memory-growth-${config.policy}`);
  let capture = { stdout: "", stderr: "" };
  let controller;
  let terminated = Promise.resolve();
  let timedOut = false;
  const abort = () => {
    timedOut = true;
    if (controller && !controller.signal.aborted) controller.abort(new Error("memory-growth reached its shared deadline"));
  };
  const timer = setDeadlineTimer(abort, Math.max(0, executionDeadlineAt - clock()));
  let measured;
  try {
    controller = new AbortController();
    if (timedOut || clock() >= executionDeadlineAt) controller.abort(new Error("memory-growth reached its shared deadline before process start"));
    const collected = await collectProcessSample({
      endpoint, config, runSample, spawnProcess, terminate, controller, envSnapshot, deadlineAt, executionDeadlineAt, clock, openFile,
      outputPrefix: prefix, onCapture: (value) => { capture = value; }, onTermination: (promise) => { terminated = promise; },
    });
    measured = collected.measured;
    if (timedOut || clock() >= executionDeadlineAt) fail("memory-growth sample completed beyond the execution deadline");
    if (controller.signal.aborted) { await terminated; fail("memory-growth sample was cancelled by the shared deadline"); }
    const validated = validateMemoryGrowthSample(measured, config, setup);
    if (clock() >= executionDeadlineAt) fail("memory-growth deadline exhausted before input recheck");
    const [setupAfterBytes, binaryAfterBytes] = await Promise.all([readFile(config.setup_file), readFile(config.binary)]);
    if (!Buffer.from(setupBytes).equals(Buffer.from(setupAfterBytes)) || !Buffer.from(binaryBytes).equals(Buffer.from(binaryAfterBytes))) fail("binary or setup content changed during memory-growth run");
    if (timedOut || clock() >= executionDeadlineAt) fail("memory-growth deadline exhausted during validation");
    const first = validated.sample.observations[0]; const second = validated.sample.observations[1];
    const domains = Object.fromEntries(Object.keys(MEMORY_DOMAINS).map((domain) => [domain, growthDelta(first, second, first.domains[domain], second.domains[domain], "serialized_bytes")]));
    const report = {
      schema: "runtime-sync-memory-growth-report", schema_version: 1, status: "COMPLETE_MEMORY_GROWTH_DIAGNOSTIC",
      policy: config.policy, seed: config.seed, civil_days_n: config.civil_days_n, source_revision: config.source_revision,
      cpu_affinity: config.cpu_affinity, setup_input: setup, binary_sha256: digest(binaryBytes), setup_sha256: digest(setupBytes),
      config_sha256: configSha256, runtime_environment: runtimeEnvironment,
      run_wall_ns: validated.sample.run_wall_ns, initialization_wall_ns: validated.sample.initialization_wall_ns,
      pure_flush_wall_ns: validated.sample.pure_flush_wall_ns, pure_flush_availability: validated.sample.pure_flush_availability,
      coverage_table_peak_count: validated.sample.coverage_table_peak_count,
      coverage_table_peak_availability: validated.sample.coverage_table_peak_availability,
      measurement_effects: validated.sample.measurement_effects,
      completed_civil_days: validated.sample.completed_civil_days, completed_trading_days: validated.sample.completed_trading_days,
      completed_ticks: validated.sample.completed_ticks, final_civil_date: validated.sample.final_civil_date, final_phase: validated.sample.final_phase,
      closed_dates: validated.sample.closed_dates, civil_day_boundaries: validated.sample.civil_day_boundaries,
      shutdown_confirmed: validated.sample.shutdown_confirmed, shutdown: validated.sample.shutdown,
      peak_process_tree_rss_bytes: measured.peak_process_tree_rss_bytes, process_tree_thread_state: measured.process_tree_thread_state,
      observations: validated.sample.observations, domain_serialized_growth: domains,
      contract_metrics_complete: false,
      measurement_gaps: ["pure_flush_wall_ns unavailable", "coverage_table_peak_count unavailable", "各域 serialized_bytes 不覆盖完整 resident allocation，且不相加解释 RSS", "候选点 RSS before/after 是观测快照；独立 runProcessSample process-tree peak 不是域 resident 归因"],
      candidate_rss_snapshot_delta_bytes: second.rss_after_serialization_bytes - first.rss_after_serialization_bytes,
      private_coverage_peak_availability: "unavailable",
      measurement_limit: "serialized_bytes 是序列化内容长度，不代表驻留分配；候选点 RSS 为进程快照，不是峰值或域驻留量。",
      cpu_load: { before: loadBefore, after: load() }, stdout: measured.stdout, stderr: measured.stderr,
    };
    if (prefix) {
      const reportPath = path.join(config.output_directory, "runtime-sync-memory-growth-report.json");
      const partialPath = `${reportPath}.partial`;
      if (clock() >= executionDeadlineAt) fail("memory-growth deadline exhausted before report publication");
      let publicationStage = "not-created";
      try {
        publicationStage = "partial-write-attempted";
        await writeFile(partialPath, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
        publicationStage = "partial-created";
        if (clock() >= executionDeadlineAt) fail("memory-growth deadline exhausted before report publication");
        await renameFile(partialPath, reportPath);
        publicationStage = "published";
        if (clock() >= executionDeadlineAt) fail("memory-growth deadline exhausted during report publication");
      } catch (error) {
        const cleanupPath = publicationStage === "published" ? reportPath : ["partial-write-attempted", "partial-created"].includes(publicationStage) ? partialPath : undefined;
        if (cleanupPath !== undefined) {
          try { await unlinkFile(cleanupPath); }
          catch (cleanupError) {
            const label = publicationStage === "published" ? "report" : "partial report";
            throw new AggregateError([error, cleanupError], `memory-growth ${label} publication failed and cleanup also failed: ${error.message}; ${cleanupError.message}`, { cause: error });
          }
        }
        throw error;
      }
    }
    return report;
  } catch (error) {
    abort();
    const cleanupErrors = [];
    if (error.sampleEvidence === undefined) {
      try { await terminated; } catch (terminationError) { cleanupErrors.push(terminationError); }
    }
    const evidence = error.sampleEvidence ?? { capture, rawPersisted: prefix !== undefined };
    if (prefix && clock() < executionDeadlineAt) {
      const failure = { policy: config.policy, error: error.message, stdout: evidence.capture.stdout, stderr: evidence.capture.stderr };
      try { await writeFile(`${prefix}.failure.json`, `${JSON.stringify(failure, null, 2)}\n`, { flag: "wx" }); }
      catch (artifactError) { cleanupErrors.push(artifactError); }
    }
    if (cleanupErrors.length > 0) throw new AggregateError([error, ...cleanupErrors], `memory-growth failed and cleanup/evidence also failed: ${[error, ...cleanupErrors].map((entry) => entry.message).join("; ")}`, { cause: error });
    throw error;
  } finally { clearDeadlineTimer(timer); }
}
