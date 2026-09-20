#!/usr/bin/env node
import { spawn } from "node:child_process";
import fsp from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual } from "node:util";

const CONFIG_SCHEMA = "escrow-perf-config-v1";
const SAMPLE_SCHEMA = "escrow-perf-sample-v1";
const REPORT_SCHEMA = "escrow-perf-report-v1";
const REQUIRED_PHASES = Array.from({ length: 10 }, (_, index) => `P${index}`);

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

function validateEndpoint(endpoint, label) {
  exactKeys(endpoint, ["command", "cwd", "source_fingerprint"], label);
  if (!Array.isArray(endpoint.command) || endpoint.command.length === 0 || !endpoint.command.every((part) => typeof part === "string" && part.length > 0)) fail(`${label}.command must be a non-empty argv array`);
  if (typeof endpoint.cwd !== "string" || !path.isAbsolute(endpoint.cwd)) fail(`${label}.cwd must be absolute`);
  validateSourceFingerprint(endpoint.source_fingerprint, `${label}.source_fingerprint`);
}

export function validatePerformanceConfig(config) {
  exactKeys(config, ["schema", "workload", "environment_contract", "warmup_runs", "sample_count", "rss_sample_interval_ms", "before", "after"], "performance config");
  if (config.schema !== CONFIG_SCHEMA) fail("performance config schema is unsupported");
  exactKeys(config.workload, ["scenario", "seed", "completed_ticks", "profile", "features"], "performance config workload");
  if (typeof config.workload.scenario !== "string" || config.workload.scenario.length === 0) fail("performance workload scenario is missing");
  if (!(typeof config.workload.seed === "string" || Number.isSafeInteger(config.workload.seed))) fail("performance workload seed is invalid");
  if (!Number.isSafeInteger(config.workload.completed_ticks) || config.workload.completed_ticks <= 0) fail("performance workload completed_ticks must be positive");
  if (typeof config.workload.profile !== "string" || config.workload.profile.length === 0) fail("performance workload profile is missing");
  if (!Array.isArray(config.workload.features) || !config.workload.features.every((feature) => typeof feature === "string" && feature.length > 0)
    || new Set(config.workload.features).size !== config.workload.features.length) fail("performance workload features are invalid or duplicate");
  exactKeys(config.environment_contract, ["cargo", "rustc", "target", "rustflags", "cargo_jobs"], "performance environment contract");
  for (const field of ["cargo", "rustc", "target", "rustflags"]) if (typeof config.environment_contract[field] !== "string") fail(`performance environment contract ${field} must be a string`);
  if (!Number.isSafeInteger(config.environment_contract.cargo_jobs) || config.environment_contract.cargo_jobs <= 0) fail("performance environment contract cargo_jobs must be positive");
  if (!Number.isSafeInteger(config.warmup_runs) || config.warmup_runs < 0) fail("performance warmup_runs is invalid");
  if (!Number.isSafeInteger(config.sample_count) || config.sample_count <= 0) fail("performance sample_count must be positive");
  if (!Number.isSafeInteger(config.rss_sample_interval_ms) || config.rss_sample_interval_ms <= 0) fail("performance RSS sample interval must be positive");
  validateEndpoint(config.before, "performance config before");
  validateEndpoint(config.after, "performance config after");
  return config;
}

async function linuxProcessTreeRssBytes(rootPid) {
  if (process.platform !== "linux") fail("process-tree RSS sampling currently requires Linux /proc");
  const pending = [rootPid];
  const visited = new Set();
  let totalKiB = 0;
  while (pending.length > 0) {
    const pid = pending.pop();
    if (visited.has(pid)) continue;
    visited.add(pid);
    try {
      const [status, children] = await Promise.all([
        fsp.readFile(`/proc/${pid}/status`, "utf8"),
        fsp.readFile(`/proc/${pid}/task/${pid}/children`, "utf8"),
      ]);
      const match = status.match(/^VmRSS:\s+(\d+)\s+kB$/m);
      if (match) totalKiB += Number(match[1]);
      for (const child of children.trim().split(/\s+/).filter(Boolean)) pending.push(Number(child));
    } catch (error) {
      if (error?.code !== "ENOENT" && error?.code !== "ESRCH") throw error;
    }
  }
  return totalKiB * 1024;
}

function parseSampleStdout(stdout, label) {
  let parsed;
  try {
    parsed = JSON.parse(stdout);
  } catch (error) {
    fail(`${label} stdout must be exactly one JSON sample: ${error.message}`);
  }
  exactKeys(parsed, ["schema", "workload", "environment_contract", "completed_ticks", "source_fingerprint", "phase_wall_ns", "runnable_samples"], `${label} sample`);
  if (parsed.schema !== SAMPLE_SCHEMA) fail(`${label} sample schema is unsupported`);
  if (!Number.isSafeInteger(parsed.completed_ticks) || parsed.completed_ticks <= 0) fail(`${label} completed_ticks is invalid`);
  validateSourceFingerprint(parsed.source_fingerprint, `${label} source_fingerprint`);
  if (!isRecord(parsed.workload)) fail(`${label} workload echo is missing`);
  if (!isRecord(parsed.environment_contract) || Object.keys(parsed.environment_contract).length === 0) fail(`${label} environment/toolchain echo is missing`);
  if (parsed.phase_wall_ns !== null) {
    if (!isRecord(parsed.phase_wall_ns)) fail(`${label} phase_wall_ns must be null or an object`);
    for (const [phase, value] of Object.entries(parsed.phase_wall_ns)) {
      if (!/^P[0-9]+$/.test(phase) || typeof value !== "string" || !/^(0|[1-9][0-9]*)$/.test(value)) fail(`${label} phase wall entry ${phase} is invalid`);
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
  exactKeys(measured, ["wall_ns", "peak_process_tree_rss_bytes", "stdout", "stderr"], `${label} measurement`);
  if (typeof measured.wall_ns !== "string" || !/^[1-9][0-9]*$/.test(measured.wall_ns)) fail(`${label} wall_ns must be a positive decimal string`);
  if (!Number.isSafeInteger(measured.peak_process_tree_rss_bytes) || measured.peak_process_tree_rss_bytes <= 0) fail(`${label} peak RSS must be positive`);
  if (typeof measured.stdout !== "string" || typeof measured.stderr !== "string") fail(`${label} stdout/stderr must be strings`);
  const sample = parseSampleStdout(measured.stdout, label);
  if (!isDeepStrictEqual(sample.workload, workload)) fail(`${label} workload scenario/seed/profile/features differ from the common workload`);
  if (!isRecord(environmentContract) || !isDeepStrictEqual(sample.environment_contract, environmentContract)) fail(`${label} environment/toolchain contract differs from the common environment`);
  if (sample.completed_ticks !== workload.completed_ticks) fail(`${label} completed tick count differs from the common workload`);
  if (sample.source_fingerprint !== endpoint.source_fingerprint) fail(`${label} source fingerprint differs from the configured endpoint`);
  if (isAfter && (sample.phase_wall_ns === null || sample.runnable_samples === null)) fail(`${label} new engine sample must include phase wall and runnable samples`);
  if (isAfter && JSON.stringify(Object.keys(sample.phase_wall_ns).sort()) !== JSON.stringify([...REQUIRED_PHASES].sort())) fail(`${label} new engine phase wall must include all phases P0 through P9 exactly once`);
  return {
    wall_ns: measured.wall_ns,
    peak_process_tree_rss_bytes: measured.peak_process_tree_rss_bytes,
    completed_ticks: sample.completed_ticks,
    workload: sample.workload,
    environment_contract: sample.environment_contract,
    ticks_per_second: sample.completed_ticks / (Number(BigInt(measured.wall_ns)) / 1_000_000_000),
    phase_wall_ns: sample.phase_wall_ns,
    runnable_samples: sample.runnable_samples,
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
      if (child.pid) peakRss = Math.max(peakRss, await linuxProcessTreeRssBytes(child.pid));
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
    stdout,
    stderr,
  };
}

function aggregateSamples(samples) {
  const throughput = samples.map((sample) => sample.ticks_per_second);
  const rss = samples.map((sample) => sample.peak_process_tree_rss_bytes);
  return {
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
}

function runtimeEnvironment() {
  const cpus = os.cpus();
  return {
    os: os.type(),
    release: os.release(),
    arch: process.arch,
    node: process.version,
    cpu_model: cpus[0]?.model ?? "unknown",
    logical_cpu_count: cpus.length,
    total_memory_bytes: os.totalmem(),
  };
}

export async function runPerformanceHarness(config, { runSample = runProcessSample, now = () => new Date().toISOString(), environment = runtimeEnvironment } = {}) {
  validatePerformanceConfig(config);
  for (let index = 0; index < config.warmup_runs; index += 1) {
    await runSample(config.before, { rssSampleIntervalMs: config.rss_sample_interval_ms, warmup: true, side: "before", index });
    await runSample(config.after, { rssSampleIntervalMs: config.rss_sample_interval_ms, warmup: true, side: "after", index });
  }
  const samples = { before: [], after: [] };
  for (let index = 0; index < config.sample_count; index += 1) {
    const order = index % 2 === 0 ? ["before", "after"] : ["after", "before"];
    for (const side of order) {
      const measured = await runSample(config[side], { rssSampleIntervalMs: config.rss_sample_interval_ms, warmup: false, side, index });
      samples[side].push(validateMeasuredSample(measured, config[side], config.workload, `${side} sample ${index}`, side === "after", config.environment_contract));
    }
  }
  const beforeAggregate = aggregateSamples(samples.before);
  const afterAggregate = aggregateSamples(samples.after);
  return {
    schema: REPORT_SCHEMA,
    generated_at: now(),
    workload: config.workload,
    environment_contract: config.environment_contract,
    runtime_environment: environment(),
    measurement_contract: {
      same_workload_for_both_sides: true,
      rss_scope: "Linux process tree rooted at the configured executable",
      throughput_source: "completed_ticks / measured wall time",
      cpu_utilization_used_as_throughput: false,
      preset_performance_threshold: null,
      warmup_runs: config.warmup_runs,
      sample_count: config.sample_count,
      alternating_measurement_order: true,
    },
    before: { source_fingerprint: config.before.source_fingerprint, command: config.before.command, cwd: config.before.cwd, samples: samples.before, aggregate: beforeAggregate },
    after: { source_fingerprint: config.after.source_fingerprint, command: config.after.command, cwd: config.after.cwd, samples: samples.after, aggregate: afterAggregate },
    comparison: {
      throughput_mean_ratio_after_over_before: afterAggregate.ticks_per_second.mean / beforeAggregate.ticks_per_second.mean,
      peak_rss_mean_ratio_after_over_before: afterAggregate.peak_process_tree_rss_bytes.mean / beforeAggregate.peak_process_tree_rss_bytes.mean,
    },
  };
}

async function writeAtomically(filePath, bytes) {
  try {
    await fsp.lstat(filePath);
    fail(`performance report output already exists: ${filePath}`);
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }
  const temporary = `${filePath}.${process.pid}.${Date.now()}.tmp`;
  await fsp.writeFile(temporary, bytes, { flag: "wx" });
  await fsp.rename(temporary, filePath);
}

export async function main(argv) {
  if (argv.length !== 2 || argv.some((value) => value.startsWith("-"))) {
    fail("usage: node scripts/simulation/escrow-performance-harness.mjs <config.json> <new-report.json>");
  }
  const [configPath, reportPath] = argv.map((value) => path.resolve(value));
  const configStat = await fsp.lstat(configPath);
  if (!configStat.isFile() || configStat.isSymbolicLink()) fail("performance config must be a regular file");
  const config = JSON.parse(await fsp.readFile(configPath, "utf8"));
  const report = await runPerformanceHarness(config);
  await writeAtomically(reportPath, `${JSON.stringify(report, null, 2)}\n`);
  console.log(JSON.stringify({ report: reportPath, before_samples: report.before.samples.length, after_samples: report.after.samples.length }));
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(`escrow performance harness failed: ${error.message}`);
    process.exitCode = 1;
  });
}
