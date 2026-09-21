#!/usr/bin/env node
/**
 * Task 9 paired performance evidence runner.
 *
 * Each endpoint prints one escrow-perf-sample-v2 JSON object per invocation.
 * `completed_ticks` is the total number of committed ticks across the declared
 * workload `repetitions`; throughput is derived only from that total and the
 * runner's measured wall clock. The baseline may report null phase/runnable
 * instrumentation. The new engine must report P0..P9 wall totals and at least
 * one Rayon registry-capacity sample. Actual runnable-thread evidence is
 * sampled independently from Linux /proc for the whole measured process tree;
 * CPU utilization is intentionally not substituted for throughput.
 */
import { spawn } from "node:child_process";
import fsp from "node:fs/promises";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual } from "node:util";
import { escrowSourceManifest } from "./escrow-source-manifest.mjs";

const CONFIG_SCHEMA = "escrow-perf-config-v2";
const SAMPLE_SCHEMA = "escrow-perf-sample-v2";
const REPORT_SCHEMA = "escrow-perf-report-v3";
const REQUIRED_PHASES = Array.from({ length: 10 }, (_, index) => `P${index}`);
const MAX_U64 = 2n ** 64n - 1n;
const WORKSPACE_ROOT = path.resolve(fileURLToPath(new URL("../..", import.meta.url)));

function fail(message) {
  throw new Error(message);
}

function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function exactKeys(value, expected, label) {
  if (!isRecord(value)) fail(`${label} must be an object`);
  const actual = Object.keys(value).sort();
  const wanted = [...expected].sort();
  if (JSON.stringify(actual) !== JSON.stringify(wanted)) fail(`${label} keys mismatch`);
}

function validateSourceFingerprint(value, label) {
  if (typeof value !== "string" || !/^[a-f0-9]{64}$/.test(value)) fail(`${label} must be a SHA-256 digest`);
}

function validateWorkspacePath(value, label, { directory = false } = {}) {
  if (typeof value !== "string" || !path.isAbsolute(value)) fail(`${label} must be absolute`);
  const normalized = path.normalize(value);
  const relative = path.relative(WORKSPACE_ROOT, normalized);
  if (relative === "" || relative === ".." || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) {
    fail(`${label} must be inside the workspace root ${WORKSPACE_ROOT}`);
  }
  let current = path.parse(normalized).root;
  for (const component of normalized.slice(current.length).split(path.sep).filter(Boolean)) {
    current = path.join(current, component);
    let stat;
    try { stat = fs.lstatSync(current); }
    catch (error) { if (error?.code === "ENOENT") fail(`${label} does not exist: ${normalized}`); throw error; }
    if (stat.isSymbolicLink()) fail(`${label} must not traverse a symbolic link: ${current}`);
  }
  let resolved;
  try { resolved = fs.realpathSync.native(normalized); }
  catch (error) { fail(`${label} cannot be resolved: ${error.message}`); }
  if (resolved !== normalized) fail(`${label} must be canonical and non-symlinked: ${normalized}`);
  const resolvedRelative = path.relative(WORKSPACE_ROOT, resolved);
  if (resolvedRelative === "" || resolvedRelative === ".." || resolvedRelative.startsWith(`..${path.sep}`) || path.isAbsolute(resolvedRelative)) {
    fail(`${label} resolves outside the workspace root ${WORKSPACE_ROOT}`);
  }
  if (directory && !fs.lstatSync(resolved).isDirectory()) fail(`${label} must be a directory`);
  return resolved;
}

function validateJsonValue(value, label) {
  if (value === null || typeof value === "string" || typeof value === "boolean") return;
  if (typeof value === "number") {
    if (!Number.isFinite(value)) fail(`${label} contains a non-finite number`);
    return;
  }
  if (Array.isArray(value)) {
    value.forEach((entry, index) => validateJsonValue(entry, `${label}[${index}]`));
    return;
  }
  if (!isRecord(value)) fail(`${label} contains a non-JSON value`);
  for (const [key, entry] of Object.entries(value)) {
    if (entry === undefined) fail(`${label}.${key} is undefined`);
    validateJsonValue(entry, `${label}.${key}`);
  }
}

function decimalU64(value, label, { positive = false } = {}) {
  if (typeof value !== "string" || !/^(0|[1-9][0-9]*)$/.test(value)) {
    fail(`${label} must be a canonical unsigned decimal string`);
  }
  const parsed = BigInt(value);
  if (parsed > MAX_U64 || (positive && parsed === 0n)) {
    fail(`${label} must be ${positive ? "a positive" : "an"} u64 decimal string`);
  }
  return parsed;
}

function validateEndpoint(endpoint, label) {
  exactKeys(endpoint, ["command", "cwd", "source_fingerprint"], label);
  if (!Array.isArray(endpoint.command) || endpoint.command.length === 0 || !endpoint.command.every((part) => typeof part === "string" && part.length > 0)) fail(`${label}.command must be a non-empty argv array`);
  endpoint.cwd = validateWorkspacePath(endpoint.cwd, `${label}.cwd`, { directory: true });
  validateSourceFingerprint(endpoint.source_fingerprint, `${label}.source_fingerprint`);
}

export function validatePerformanceConfig(config) {
  exactKeys(config, ["schema", "workload", "environment_contract", "warmup_runs", "sample_count", "rss_sample_interval_ms", "before", "after"], "performance config");
  if (config.schema !== CONFIG_SCHEMA) fail("performance config schema is unsupported");
  exactKeys(config.workload, ["scenario", "seed", "setup_manifest", "completed_ticks", "repetitions", "profile", "features"], "performance config workload");
  if (typeof config.workload.scenario !== "string" || config.workload.scenario.length === 0) fail("performance workload scenario is missing");
  if (!((typeof config.workload.seed === "string" && config.workload.seed.length > 0) || Number.isSafeInteger(config.workload.seed))) fail("performance workload seed is invalid");
  if (!isRecord(config.workload.setup_manifest) || Object.keys(config.workload.setup_manifest).length === 0) fail("performance workload setup_manifest must be a non-empty object");
  validateJsonValue(config.workload.setup_manifest, "performance workload setup_manifest");
  if (!Number.isSafeInteger(config.workload.completed_ticks) || config.workload.completed_ticks <= 0) fail("performance workload completed_ticks must be positive");
  if (!Number.isSafeInteger(config.workload.repetitions) || config.workload.repetitions <= 0) fail("performance workload repetitions must be positive");
  if (typeof config.workload.profile !== "string" || config.workload.profile.length === 0) fail("performance workload profile is missing");
  if (!Array.isArray(config.workload.features) || !config.workload.features.every((feature) => typeof feature === "string" && feature.length > 0)
    || new Set(config.workload.features).size !== config.workload.features.length) fail("performance workload features are invalid or duplicate");
  exactKeys(config.environment_contract, ["cargo", "rustc", "target", "rustflags", "cargo_jobs", "rayon_threads"], "performance environment contract");
  for (const field of ["cargo", "rustc", "target", "rustflags"]) if (typeof config.environment_contract[field] !== "string") fail(`performance environment contract ${field} must be a string`);
  if (!Number.isSafeInteger(config.environment_contract.cargo_jobs) || config.environment_contract.cargo_jobs <= 0) fail("performance environment contract cargo_jobs must be positive");
  if (!Number.isSafeInteger(config.environment_contract.rayon_threads) || config.environment_contract.rayon_threads <= 0) fail("performance environment contract rayon_threads must be positive");
  if (!Number.isSafeInteger(config.warmup_runs) || config.warmup_runs < 0) fail("performance warmup_runs is invalid");
  if (!Number.isSafeInteger(config.sample_count) || config.sample_count <= 0) fail("performance sample_count must be positive");
  if (!Number.isSafeInteger(config.rss_sample_interval_ms) || config.rss_sample_interval_ms <= 0) fail("performance RSS sample interval must be positive");
  validateEndpoint(config.before, "performance config before");
  validateEndpoint(config.after, "performance config after");
  return config;
}

async function linuxProcessTreeSample(rootPid) {
  if (process.platform !== "linux") fail("process-tree RSS sampling currently requires Linux /proc");
  const pending = [rootPid];
  const visited = new Set();
  let totalKiB = 0;
  let totalThreads = 0;
  let runnableThreads = 0;
  while (pending.length > 0) {
    const pid = pending.pop();
    if (visited.has(pid)) continue;
    visited.add(pid);
    try {
      const [status, taskEntries] = await Promise.all([
        fsp.readFile(`/proc/${pid}/status`, "utf8"),
        fsp.readdir(`/proc/${pid}/task`, { withFileTypes: true }),
      ]);
      const match = status.match(/^VmRSS:\s+(\d+)\s+kB$/m);
      if (match) totalKiB += Number(match[1]);
      const taskIds = taskEntries
        .filter((entry) => entry.isDirectory() && /^\d+$/.test(entry.name))
        .map((entry) => entry.name);
      const taskSamples = await Promise.all(taskIds.map(async (taskId) => {
        try {
          const [stat, children] = await Promise.all([
            fsp.readFile(`/proc/${pid}/task/${taskId}/stat`, "utf8"),
            fsp.readFile(`/proc/${pid}/task/${taskId}/children`, "utf8"),
          ]);
          const closeParenthesis = stat.lastIndexOf(")");
          if (closeParenthesis < 0) fail(`malformed /proc stat for pid ${pid} task ${taskId}`);
          const state = stat.slice(closeParenthesis + 1).trimStart()[0];
          if (typeof state !== "string") fail(`missing /proc task state for pid ${pid} task ${taskId}`);
          return { state, children };
        } catch (error) {
          if (error?.code === "ENOENT" || error?.code === "ESRCH") return null;
          throw error;
        }
      }));
      for (const task of taskSamples) {
        if (task === null) continue;
        totalThreads += 1;
        // Linux documents R as running or runnable. Sleeping Rayon workers are
        // deliberately excluded; registry capacity is reported separately.
        if (task.state === "R") runnableThreads += 1;
        for (const child of task.children.trim().split(/\s+/).filter(Boolean)) pending.push(Number(child));
      }
    } catch (error) {
      if (error?.code !== "ENOENT" && error?.code !== "ESRCH") throw error;
    }
  }
  return {
    process_count: visited.size,
    rss_bytes: totalKiB * 1024,
    total_threads: totalThreads,
    runnable_threads: runnableThreads,
  };
}

function summarizeThreadStateSamples(samples, sampleIntervalMs) {
  if (samples.length === 0) fail("measured command completed before a process-tree thread-state sample was observed");
  const values = (field) => samples.map((sample) => sample[field]);
  const summary = (field) => {
    const entries = values(field);
    return { minimum: Math.min(...entries), maximum: Math.max(...entries) };
  };
  const runnable = values("runnable_threads");
  const histogram = {};
  for (const value of runnable) histogram[String(value)] = (histogram[String(value)] ?? 0) + 1;
  return {
    schema: "linux-process-tree-thread-state-v1",
    sampled_state: "R (running or runnable)",
    sample_interval_ms: sampleIntervalMs,
    sample_count: samples.length,
    process_count: summary("process_count"),
    total_threads: summary("total_threads"),
    runnable_threads: {
      minimum: Math.min(...runnable),
      maximum: Math.max(...runnable),
      mean: runnable.reduce((total, value) => total + value, 0) / runnable.length,
      histogram,
    },
  };
}

function validateThreadStateSampling(value, label) {
  exactKeys(value, ["schema", "sampled_state", "sample_interval_ms", "sample_count", "process_count", "total_threads", "runnable_threads"], `${label} thread-state sampling`);
  if (value.schema !== "linux-process-tree-thread-state-v1" || value.sampled_state !== "R (running or runnable)") fail(`${label} thread-state sampling contract is unsupported`);
  if (!Number.isSafeInteger(value.sample_interval_ms) || value.sample_interval_ms <= 0
    || !Number.isSafeInteger(value.sample_count) || value.sample_count <= 0) fail(`${label} thread-state sampling interval/count is invalid`);
  for (const field of ["process_count", "total_threads"]) {
    exactKeys(value[field], ["minimum", "maximum"], `${label} ${field}`);
    if (!Number.isSafeInteger(value[field].minimum) || value[field].minimum <= 0
      || !Number.isSafeInteger(value[field].maximum) || value[field].maximum < value[field].minimum) fail(`${label} ${field} range is invalid`);
  }
  exactKeys(value.runnable_threads, ["minimum", "maximum", "mean", "histogram"], `${label} runnable_threads`);
  if (!Number.isSafeInteger(value.runnable_threads.minimum) || value.runnable_threads.minimum < 0
    || !Number.isSafeInteger(value.runnable_threads.maximum) || value.runnable_threads.maximum < value.runnable_threads.minimum
    || !Number.isFinite(value.runnable_threads.mean) || value.runnable_threads.mean < 0
    || !isRecord(value.runnable_threads.histogram)) fail(`${label} runnable thread summary is invalid`);
  let histogramCount = 0;
  let histogramTotal = 0;
  for (const [threads, count] of Object.entries(value.runnable_threads.histogram)) {
    if (!/^(0|[1-9][0-9]*)$/.test(threads) || !Number.isSafeInteger(count) || count <= 0) fail(`${label} runnable thread histogram is invalid`);
    const parsedThreads = Number(threads);
    if (parsedThreads < value.runnable_threads.minimum || parsedThreads > value.runnable_threads.maximum) fail(`${label} runnable thread histogram is outside its declared range`);
    histogramCount += count;
    histogramTotal += parsedThreads * count;
  }
  if (histogramCount !== value.sample_count || Math.abs(histogramTotal / histogramCount - value.runnable_threads.mean) > Number.EPSILON) {
    fail(`${label} runnable thread histogram does not reproduce its sample count and mean`);
  }
  if (value.runnable_threads.maximum > value.total_threads.maximum) fail(`${label} runnable threads exceed sampled total threads`);
  return value;
}

function parseSampleStdout(stdout, label) {
  let parsed;
  try {
    parsed = JSON.parse(stdout);
  } catch (error) {
    fail(`${label} stdout must be exactly one JSON sample: ${error.message}`);
  }
  if (isRecord(parsed) && (parsed.status === "BLOCKED" || parsed.status === "FAIL")) {
    fail(`${label} explicitly reported ${parsed.status}`);
  }
  exactKeys(parsed, ["schema", "status", "workload", "environment_contract", "completed_ticks", "source_fingerprint", "phase_wall_ns", "runnable_samples"], `${label} sample`);
  if (parsed.schema !== SAMPLE_SCHEMA) fail(`${label} sample schema is unsupported`);
  if (parsed.status !== "PASS") fail(`${label} status must be PASS`);
  if (!Number.isSafeInteger(parsed.completed_ticks) || parsed.completed_ticks <= 0) fail(`${label} completed_ticks is invalid`);
  validateSourceFingerprint(parsed.source_fingerprint, `${label} source_fingerprint`);
  if (!isRecord(parsed.workload)) fail(`${label} workload echo is missing`);
  if (!isRecord(parsed.environment_contract) || Object.keys(parsed.environment_contract).length === 0) fail(`${label} environment/toolchain echo is missing`);
  if (parsed.phase_wall_ns !== null) {
    if (!isRecord(parsed.phase_wall_ns)) fail(`${label} phase_wall_ns must be null or an object`);
    for (const [phase, value] of Object.entries(parsed.phase_wall_ns)) {
      if (!/^P[0-9]+$/.test(phase)) fail(`${label} phase wall entry ${phase} is invalid`);
      decimalU64(value, `${label} phase wall entry ${phase}`);
    }
  }
  if (parsed.runnable_samples !== null && (!Array.isArray(parsed.runnable_samples)
    || parsed.runnable_samples.length === 0
    || !parsed.runnable_samples.every((sample) => Number.isSafeInteger(sample) && sample >= 0))) {
    fail(`${label} runnable_samples must be null or a non-empty array of non-negative integers`);
  }
  return parsed;
}

export function validateMeasuredSample(measured, endpoint, workload, label, isAfter, environmentContract) {
  exactKeys(measured, ["wall_ns", "peak_process_tree_rss_bytes", "process_tree_thread_state", "stdout", "stderr"], `${label} measurement`);
  const wallNs = decimalU64(measured.wall_ns, `${label} wall_ns`, { positive: true });
  if (!Number.isSafeInteger(measured.peak_process_tree_rss_bytes) || measured.peak_process_tree_rss_bytes <= 0) fail(`${label} peak RSS must be positive`);
  if (typeof measured.stdout !== "string" || typeof measured.stderr !== "string") fail(`${label} stdout/stderr must be strings`);
  validateThreadStateSampling(measured.process_tree_thread_state, label);
  if (/\bBLOCKED\b/u.test(measured.stderr) || /\bFAIL(?:ED)?\b/u.test(measured.stderr)) fail(`${label} stderr reported BLOCKED/FAIL`);
  const sample = parseSampleStdout(measured.stdout, label);
  if (!isDeepStrictEqual(sample.workload, workload)) fail(`${label} workload scenario/seed/setup/count/profile/features differ from the common workload`);
  if (!isRecord(environmentContract) || !isDeepStrictEqual(sample.environment_contract, environmentContract)) fail(`${label} environment/toolchain contract differs from the common environment`);
  if (sample.completed_ticks !== workload.completed_ticks) fail(`${label} completed tick count differs from the common workload`);
  if (sample.source_fingerprint !== endpoint.source_fingerprint) fail(`${label} source fingerprint differs from the configured endpoint`);
  if (isAfter && (sample.phase_wall_ns === null || sample.runnable_samples === null)) fail(`${label} new engine sample must include phase wall and runnable samples`);
  if (isAfter && JSON.stringify(Object.keys(sample.phase_wall_ns).sort()) !== JSON.stringify([...REQUIRED_PHASES].sort())) fail(`${label} new engine phase wall must include all phases P0 through P9 exactly once`);
  if (isAfter && !sample.runnable_samples.every((capacity) => capacity === environmentContract.rayon_threads)) {
    fail(`${label} Rayon registry-capacity samples differ from the configured pool size`);
  }
  const ticksPerSecond = sample.completed_ticks * 1_000_000_000 / Number(wallNs);
  if (!Number.isFinite(ticksPerSecond) || ticksPerSecond <= 0) fail(`${label} ticks_per_second is not finite and positive`);
  return {
    wall_ns: measured.wall_ns,
    peak_process_tree_rss_bytes: measured.peak_process_tree_rss_bytes,
    completed_ticks: sample.completed_ticks,
    workload: sample.workload,
    environment_contract: sample.environment_contract,
    ticks_per_second: ticksPerSecond,
    phase_wall_ns: sample.phase_wall_ns,
    process_tree_thread_state: measured.process_tree_thread_state,
    rayon_registry_capacity_samples: sample.runnable_samples,
    stderr: measured.stderr,
  };
}

export async function runProcessSample(endpoint, { rssSampleIntervalMs }) {
  const startedAt = process.hrtime.bigint();
  const child = spawn(endpoint.command[0], endpoint.command.slice(1), {
    cwd: endpoint.cwd,
    env: process.env,
    shell: false,
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stdout = "";
  let stderr = "";
  let spawnError;
  let peakRss = 0;
  const threadStateSamples = [];
  let sampling = true;
  let sampleDelayHandle;
  let releaseSampleDelay;
  const waitForNextSample = () => new Promise((resolve) => {
    releaseSampleDelay = resolve;
    sampleDelayHandle = setTimeout(() => {
      sampleDelayHandle = undefined;
      releaseSampleDelay = undefined;
      resolve();
    }, rssSampleIntervalMs);
  });
  const stopSampling = () => {
    sampling = false;
    if (sampleDelayHandle !== undefined) clearTimeout(sampleDelayHandle);
    sampleDelayHandle = undefined;
    const release = releaseSampleDelay;
    releaseSampleDelay = undefined;
    release?.();
  };
  const sampler = (async () => {
    while (sampling) {
      if (child.pid) {
        const sample = await linuxProcessTreeSample(child.pid);
        peakRss = Math.max(peakRss, sample.rss_bytes);
        if (sample.total_threads > 0) threadStateSamples.push(sample);
      }
      if (sampling) await waitForNextSample();
    }
  })();
  child.stdout.on("data", (chunk) => { stdout += chunk; });
  child.stderr.on("data", (chunk) => { stderr += chunk; });
  child.on("error", (error) => { spawnError = error; });
  const { code, signal, endedAt } = await new Promise((resolve) => child.on("close", (code, signal) => {
    const endedAt = process.hrtime.bigint();
    stopSampling();
    resolve({ code, signal, endedAt });
  }));
  await sampler;
  if (spawnError) throw spawnError;
  if (code !== 0) fail(`${endpoint.command.join(" ")} exited ${code} signal ${signal ?? "none"}: ${stderr}`);
  if (peakRss <= 0) fail(`${endpoint.command.join(" ")} completed before a positive process-tree RSS sample was observed`);
  return {
    wall_ns: String(endedAt - startedAt),
    peak_process_tree_rss_bytes: peakRss,
    process_tree_thread_state: summarizeThreadStateSamples(threadStateSamples, rssSampleIntervalMs),
    stdout,
    stderr,
  };
}

function aggregateSamples(samples) {
  const throughput = samples.map((sample) => sample.ticks_per_second);
  const rss = samples.map((sample) => sample.peak_process_tree_rss_bytes);
  const aggregate = {
    sample_count: samples.length,
    ticks_per_second: {
      minimum: Math.min(...throughput),
      maximum: Math.max(...throughput),
      mean: throughput.reduce((total, value) => total + value, 0) / throughput.length,
    },
    peak_process_tree_rss_bytes: {
      minimum: Math.min(...rss),
      maximum: Math.max(...rss),
      mean: rss.reduce((total, value) => total + value, 0) / rss.length,
    },
  };
  for (const [metric, summary] of Object.entries({ ticks_per_second: aggregate.ticks_per_second, peak_process_tree_rss_bytes: aggregate.peak_process_tree_rss_bytes })) {
    for (const [statistic, value] of Object.entries(summary)) {
      if (!Number.isFinite(value) || value <= 0) fail(`${metric}.${statistic} must be finite and positive`);
    }
  }
  return aggregate;
}

function runtimeEnvironment() {
  const cpus = os.cpus();
  const processPaths = Object.fromEntries(["CARGO_TARGET_DIR", "TMPDIR", "TMP", "TEMP"].map((name) => [
    name,
    validateWorkspacePath(process.env[name], `process environment ${name}`, { directory: true }),
  ]));
  return {
    os: os.type(),
    release: os.release(),
    arch: process.arch,
    node: process.version,
    hostname: os.hostname(),
    cpu_model: cpus[0]?.model ?? "unknown",
    logical_cpu_count: cpus.length,
    total_memory_bytes: os.totalmem(),
    process_paths: processPaths,
  };
}

async function verifyEndpointSource(endpoint, label, sourceManifest) {
  const manifest = await sourceManifest(endpoint.cwd);
  if (!isRecord(manifest) || manifest.sha256 !== endpoint.source_fingerprint) {
    fail(`${label} source bytes do not match the configured source fingerprint`);
  }
  return manifest.sha256;
}

export async function runPerformanceHarness(config, {
  runSample = runProcessSample,
  now = () => new Date().toISOString(),
  environment = runtimeEnvironment,
  sourceManifest = escrowSourceManifest,
} = {}) {
  validatePerformanceConfig(config);
  const measure = async (side, context) => {
    const endpoint = config[side];
    const label = `${side} ${context.warmup ? "warmup" : "sample"} ${context.index}`;
    const before = await verifyEndpointSource(endpoint, `${label} before invocation`, sourceManifest);
    const measured = await runSample(endpoint, { rssSampleIntervalMs: config.rss_sample_interval_ms, side, ...context });
    const after = await verifyEndpointSource(endpoint, `${label} after invocation`, sourceManifest);
    if (before !== after) fail(`${label} source hash drifted while the sample was running`);
    return validateMeasuredSample(measured, endpoint, config.workload, label, side === "after", config.environment_contract);
  };
  for (let index = 0; index < config.warmup_runs; index += 1) {
    for (const side of ["before", "after"]) {
      await measure(side, { warmup: true, index });
    }
  }
  const samples = { before: [], after: [] };
  for (let index = 0; index < config.sample_count; index += 1) {
    const order = index % 2 === 0 ? ["before", "after"] : ["after", "before"];
    for (const side of order) {
      samples[side].push(await measure(side, { warmup: false, index }));
    }
  }
  const beforeAggregate = aggregateSamples(samples.before);
  const afterAggregate = aggregateSamples(samples.after);
  const throughputRatio = afterAggregate.ticks_per_second.mean / beforeAggregate.ticks_per_second.mean;
  const peakRssRatio = afterAggregate.peak_process_tree_rss_bytes.mean / beforeAggregate.peak_process_tree_rss_bytes.mean;
  if (!Number.isFinite(throughputRatio) || throughputRatio <= 0 || !Number.isFinite(peakRssRatio) || peakRssRatio <= 0) {
    fail("performance comparison ratios must be finite and positive");
  }
  const environmentManifest = environment();
  if (!isRecord(environmentManifest) || Object.keys(environmentManifest).length === 0) fail("environment manifest must be a non-empty object");
  validateJsonValue(environmentManifest, "environment manifest");
  return {
    schema: REPORT_SCHEMA,
    status: "PASS",
    generated_at: now(),
    workload: config.workload,
    environment_contract: config.environment_contract,
    environment_manifest: environmentManifest,
    measurement_contract: {
      same_machine_for_both_sides: true,
      same_workload_for_both_sides: true,
      comparison_key_fields: ["scenario", "seed", "setup_manifest", "completed_ticks", "repetitions", "profile", "features", "environment_contract"],
      rss_scope: "Linux process tree rooted at the configured executable",
      runnable_thread_source: "Linux /proc process-tree task state R (running or runnable)",
      rayon_registry_capacity_is_not_worker_activity: true,
      throughput_source: "completed_ticks / measured wall time",
      cpu_utilization_used_as_throughput: false,
      preset_performance_threshold: null,
      warmup_runs: config.warmup_runs,
      sample_count: config.sample_count,
      alternating_measurement_order: true,
      source_manifest_verified_before_and_after_every_invocation: true,
    },
    before: { role: "baseline", source_fingerprint: config.before.source_fingerprint, command: config.before.command, cwd: config.before.cwd, samples: samples.before, aggregate: beforeAggregate },
    after: { role: "new-engine", source_fingerprint: config.after.source_fingerprint, command: config.after.command, cwd: config.after.cwd, samples: samples.after, aggregate: afterAggregate },
    comparison: {
      conditions_match: true,
      throughput_mean_ratio_after_over_before: throughputRatio,
      peak_rss_mean_ratio_after_over_before: peakRssRatio,
    },
  };
}

export function reportPathForOutputDirectory(outputDirectory) {
  return path.join(path.resolve(outputDirectory), "perf-report.json");
}

export async function writeReportIdempotently(filePath, bytes) {
  try {
    const stat = await fsp.lstat(filePath);
    if (!stat.isFile() || stat.isSymbolicLink()) fail(`performance report output must be a regular non-symlink file: ${filePath}`);
    const existing = await fsp.readFile(filePath, "utf8");
    if (existing === bytes) return "unchanged";
    fail(`performance report output already exists with different content: ${filePath}`);
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }
  await fsp.writeFile(filePath, bytes, { flag: "wx" });
  return "created";
}

async function loadReusableReport(filePath, config, currentEnvironmentManifest) {
  let stat;
  try {
    stat = await fsp.lstat(filePath);
  } catch (error) {
    if (error?.code === "ENOENT") return null;
    throw error;
  }
  if (!stat.isFile() || stat.isSymbolicLink()) fail(`performance report output must be a regular non-symlink file: ${filePath}`);
  let report;
  try {
    report = JSON.parse(await fsp.readFile(filePath, "utf8"));
  } catch (error) {
    fail(`existing performance report is not valid JSON: ${error.message}`);
  }
  if (!isRecord(report) || report.schema !== REPORT_SCHEMA || report.status !== "PASS") {
    fail("existing performance report is not a reusable PASS report");
  }
  if (!isDeepStrictEqual(report.workload, config.workload)
    || !isDeepStrictEqual(report.environment_contract, config.environment_contract)
    || !isDeepStrictEqual(report.environment_manifest, currentEnvironmentManifest)
    || report.measurement_contract?.warmup_runs !== config.warmup_runs
    || report.measurement_contract?.sample_count !== config.sample_count
    || report.measurement_contract?.same_machine_for_both_sides !== true
    || report.measurement_contract?.source_manifest_verified_before_and_after_every_invocation !== true
    || report.comparison?.conditions_match !== true
    || !Number.isFinite(report.comparison?.throughput_mean_ratio_after_over_before)
    || report.comparison.throughput_mean_ratio_after_over_before <= 0
    || !Number.isFinite(report.comparison?.peak_rss_mean_ratio_after_over_before)
    || report.comparison.peak_rss_mean_ratio_after_over_before <= 0) {
    fail("existing performance report conditions differ from the requested run");
  }
  for (const side of ["before", "after"]) {
    if (report[side]?.source_fingerprint !== config[side].source_fingerprint
      || !isDeepStrictEqual(report[side]?.command, config[side].command)
      || report[side]?.cwd !== config[side].cwd
      || !Array.isArray(report[side]?.samples)
      || report[side].samples.length !== config.sample_count) {
      fail(`existing performance report ${side} endpoint or sample count differs from the requested run`);
    }
  }
  return report;
}

export async function main(argv, {
  runHarness = runPerformanceHarness,
  environment = runtimeEnvironment,
  sourceManifest = escrowSourceManifest,
  stdout = console.log,
} = {}) {
  if (argv.length !== 2 || argv.some((value) => value.startsWith("-"))) {
    fail("usage: node scripts/simulation/escrow-performance-harness.mjs <config.json> <output-directory>");
  }
  const [configPath, outputDirectory] = argv.map((value) => path.resolve(value));
  validateWorkspacePath(configPath, "performance config path");
  validateWorkspacePath(outputDirectory, "performance output directory", { directory: true });
  const configStat = await fsp.lstat(configPath);
  if (!configStat.isFile() || configStat.isSymbolicLink()) fail("performance config must be a regular file");
  const outputStat = await fsp.lstat(outputDirectory);
  if (!outputStat.isDirectory() || outputStat.isSymbolicLink()) fail("performance output must be a regular directory");
  const config = JSON.parse(await fsp.readFile(configPath, "utf8"));
  validatePerformanceConfig(config);
  const reportPath = reportPathForOutputDirectory(outputDirectory);
  const currentEnvironmentManifest = environment();
  if (!isRecord(currentEnvironmentManifest) || Object.keys(currentEnvironmentManifest).length === 0) fail("environment manifest must be a non-empty object");
  validateJsonValue(currentEnvironmentManifest, "environment manifest");
  await Promise.all([
    verifyEndpointSource(config.before, "baseline endpoint", sourceManifest),
    verifyEndpointSource(config.after, "new-engine endpoint", sourceManifest),
  ]);
  const reusable = await loadReusableReport(reportPath, config, currentEnvironmentManifest);
  if (reusable !== null) {
    const summary = { status: "PASS", report: reportPath, write: "unchanged", before_samples: reusable.before.samples.length, after_samples: reusable.after.samples.length };
    stdout(JSON.stringify(summary));
    return summary;
  }
  const report = await runHarness(config, { environment: () => currentEnvironmentManifest, sourceManifest });
  const write = await writeReportIdempotently(reportPath, `${JSON.stringify(report, null, 2)}\n`);
  const summary = { status: "PASS", report: reportPath, write, before_samples: report.before.samples.length, after_samples: report.after.samples.length };
  stdout(JSON.stringify(summary));
  return summary;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(JSON.stringify({ status: "FAIL", error: `escrow performance harness failed: ${error.message}` }));
    process.exitCode = 1;
  });
}
