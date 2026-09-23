#!/usr/bin/env node
/**
 * 变更前量价基线采集器（company-information-npc-intentions 计划的 before 锚点）。
 *
 * 用真实的 release cargo example（packages/engine/examples/baseline_fixture.rs）逐 seed
 * 生成确定性报告，逐份对账（seed、配置回显、逐笔与日 K 成交量/成交额、极端样本），
 * 并把原始 JSON 与运行清单（git revision、dirty paths、工具链、硬件、逐次 argv、
 * 退出码、耗时、sha256）写入输出目录。矩阵场景与压缩 300 tick/日成本场景分开采集，
 * 绝不合并成一份报告。
 *
 * 用法：
 * `before` 只保留解析和密封历史语料的兼容代码，CLI 不再允许重跑。
 */
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import fsp from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { prepareWorkspacePaths, validateWorkspaceOutputPath } from "../workspace-paths.mjs";

/** 独立 seed 矩阵；重复 seed 会压窄跨 seed 区间，必须在采集前拒绝。 */
export const MATRIX_SEEDS = [1, 2, 3, 4, 5, 7, 11, 19, 23, 31];

export const TRADING_DAYS = 30;
export const CROSS_YEAR_SEEDS = [1, 7, 11, 19, 31];
export const SENSITIVITY_MULTIPLIERS = [0.5, 1, 2];
export const K7_PRIMARY_NATURAL_DAYS = 5;
export const K7_CROSS_YEAR_NATURAL_DAYS = 8;
export const K7_ORDINARY_TEST_MAX_MS = 10_000;
export const K7_CHILD_TIMEOUT_MS = 300_000;
export const K7_BATCH_TIMEOUT_MS = 300_000;
export const K7_CLEANUP_RESERVE_MS = 1_000;

/**
 * 两个场景与 engine example 的构造必须一致：
 * - matrix：apps/web/src/config/defaults.ts DEFAULT_SETUP 的等值副本（完整 15300 tick/日）。
 * - compressed-300：packages/engine/tests/session.rs large_retail_account_setup(20_000)
 *   的等值副本（300 tick/日成本场景）。
 * stockCodes 为排序后的规范顺序，用于报告对账。
 */
export const SCENARIOS = {
  matrix: {
    ticksPerDay: 15300,
    auctionTicks: 900,
    closingAuctionTicks: 180,
    stockCodes: ["000812", "002156", "300260", "600101", "600610"],
    retailCount: 20000,
    instCount: 5,
    hotCount: 2,
    historyLen: 20,
    t1Enabled: true,
  },
  "compressed-300": {
    ticksPerDay: 300,
    auctionTicks: 0,
    closingAuctionTicks: 0,
    stockCodes: ["000812", "002156", "300260", "600101", "600610"],
    retailCount: 20000,
    instCount: 5,
    hotCount: 2,
    historyLen: 5,
    t1Enabled: true,
  },
};

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const K7_CHECKPOINT_SCHEMA = "k7-baseline-checkpoint-v4";
const K7_RUNNER_VERSION = "2026-09-23-cleanup-closed-deadline-v7";
const K7_SOURCE_FINGERPRINT_ALGORITHM = "k7-simulation-source-v1";
const K7_DETERMINISM_RECEIPT_SCHEMA = "k7-determinism-receipt-v1";
const K7_RESOURCE_POLICY_SCHEMA = "k7-resource-policy-v7";
const K7_MAX_CONCURRENT_CHILD_EXECUTIONS = 30;
const K7_MIN_RAYON_THREADS_PER_SEED = 4;
const K7_SOURCE_INPUTS = [
  ".cargo",
  "Cargo.lock",
  "Cargo.toml",
  "packages/engine",
  "rust-toolchain.toml",
  "scripts/simulation/baseline-run.mjs",
  "scripts/simulation/verify-k7-root.mjs",
];

export function validateSeedMatrix(seeds) {
  if (seeds.length === 0) {
    throw new Error("seed 矩阵不能为空：跨 seed 汇总至少需要一个独立样本");
  }
  const seen = new Set();
  for (const seed of seeds) {
    if (seen.has(seed)) {
      throw new Error(`seed 矩阵存在重复 seed：${seed}；重复路径会伪装成多个独立样本并压窄区间`);
    }
    seen.add(seed);
  }
}

export function buildExampleArgs(scenarioName, seed, tradingDays) {
  return [
    "run",
    "-p",
    "engine",
    "--release",
    "--example",
    "baseline_fixture",
    "--",
    scenarioName,
    String(seed),
    String(tradingDays),
  ];
}

export function buildK7ExampleArgs(scenario, seed, days, behaviorMultiplier, eventMultiplier, c01Multiplier) {
  return [scenario, String(seed), String(days), String(behaviorMultiplier), String(eventMultiplier), String(c01Multiplier)];
}

export function buildK7FixtureBuildArgs() {
  return ["build", "-p", "engine", "--release", "--features", "simulation-diagnostics", "--example", "k7_baseline_fixture", "--message-format=json-render-diagnostics"];
}

export function buildK7WorkspaceEnv(workspacePaths, baseEnv = process.env) {
  return {
    ...baseEnv,
    CARGO_TARGET_DIR: workspacePaths.cargoTargetDir,
    TMPDIR: workspacePaths.processTmpDir,
    TMP: workspacePaths.processTmpDir,
    TEMP: workspacePaths.processTmpDir,
  };
}

export function resolveBaselineFixtureTimeoutMs(
  configured = process.env.BASELINE_FIXTURE_TIMEOUT_MS,
) {
  if (configured === undefined || configured === "") return K7_CHILD_TIMEOUT_MS;
  if (!/^\d+$/.test(String(configured))) {
    throw new Error(`BASELINE_FIXTURE_TIMEOUT_MS must be a positive integer no greater than 300000ms; got ${configured}`);
  }
  const timeoutMs = Number(configured);
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs <= 0 || timeoutMs > K7_CHILD_TIMEOUT_MS) {
    throw new Error(`BASELINE_FIXTURE_TIMEOUT_MS must be within the five-minute (300000ms) hard maximum; got ${configured}`);
  }
  return timeoutMs;
}

export function buildK7ResourcePolicy(availableCpuCount, availableCpuSource, maximumThreadCount = "auto") {
  if (!Number.isInteger(availableCpuCount) || availableCpuCount <= 0) {
    throw new Error("K7 resource policy requires a positive process-available CPU count");
  }
  if (maximumThreadCount !== "auto" && (!Number.isInteger(maximumThreadCount) || maximumThreadCount <= 0)) {
    throw new Error("K7 maximum thread count must be a positive integer or auto");
  }
  const totalThreadBudget = maximumThreadCount === "auto"
    ? availableCpuCount
    : Math.min(availableCpuCount, maximumThreadCount);
  const maxConcurrentChildExecutions = Math.min(
    K7_MAX_CONCURRENT_CHILD_EXECUTIONS,
    Math.max(1, Math.floor(totalThreadBudget / K7_MIN_RAYON_THREADS_PER_SEED)),
  );
  return {
    schema: K7_RESOURCE_POLICY_SCHEMA,
    available_cpu_count: availableCpuCount,
    available_cpu_source: availableCpuSource,
    maximum_thread_count: maximumThreadCount,
    max_concurrent_child_executions: maxConcurrentChildExecutions,
    rayon_threads_per_seed: Math.max(1, Math.floor(totalThreadBudget / maxConcurrentChildExecutions)),
    ordinary_test_max_ms: K7_ORDINARY_TEST_MAX_MS,
    child_timeout_ms: K7_CHILD_TIMEOUT_MS,
    batch_timeout_ms: K7_BATCH_TIMEOUT_MS,
    execution_timeout_ms: K7_BATCH_TIMEOUT_MS - K7_CLEANUP_RESERVE_MS,
    cleanup_reserve_ms: K7_CLEANUP_RESERVE_MS,
  };
}

export function buildK7Deadline({
  batchTimeoutMs = K7_BATCH_TIMEOUT_MS,
  childTimeoutMs = K7_CHILD_TIMEOUT_MS,
  cleanupReserveMs = Math.min(K7_CLEANUP_RESERVE_MS, Math.max(1, Math.floor(batchTimeoutMs / 5))),
  now = Date.now,
} = {}) {
  if (!Number.isInteger(batchTimeoutMs) || batchTimeoutMs <= 0 || batchTimeoutMs > K7_BATCH_TIMEOUT_MS) {
    throw new Error(`K7 batch timeout must be within the five-minute (300000ms) hard maximum; got ${batchTimeoutMs}`);
  }
  if (!Number.isInteger(childTimeoutMs) || childTimeoutMs <= 0 || childTimeoutMs > K7_CHILD_TIMEOUT_MS) {
    throw new Error(`K7 child timeout must be within the five-minute (300000ms) hard maximum; got ${childTimeoutMs}`);
  }
  if (!Number.isInteger(cleanupReserveMs) || cleanupReserveMs <= 0 || cleanupReserveMs >= batchTimeoutMs) {
    throw new Error(`K7 cleanup reserve must be a positive integer smaller than the ${batchTimeoutMs}ms batch deadline; got ${cleanupReserveMs}`);
  }
  if (typeof now !== "function") throw new Error("K7 deadline clock must be a function");
  const startedAtMs = now();
  if (!Number.isFinite(startedAtMs)) throw new Error("K7 deadline clock returned a non-finite start time");
  const executionTimeoutMs = batchTimeoutMs - cleanupReserveMs;
  const expiresAtMs = startedAtMs + executionTimeoutMs;
  const hardExpiresAtMs = startedAtMs + batchTimeoutMs;
  const controller = new AbortController();
  const timer = setTimeout(() => {
    controller.abort(new Error(`K7 batch exhausted the shared ${batchTimeoutMs}ms deadline`));
  }, executionTimeoutMs);
  timer.unref?.();
  const remainingMs = () => Math.max(0, Math.floor(expiresAtMs - now()));
  const cleanupRemainingMs = () => Math.max(0, Math.floor(hardExpiresAtMs - now()));
  const assertRemaining = (context = "K7 batch") => {
    const remaining = remainingMs();
    if (remaining <= 0) {
      if (!controller.signal.aborted) {
        controller.abort(new Error(`${context} exhausted the shared ${batchTimeoutMs}ms K7 batch deadline`));
      }
      throw new Error(`${context} exhausted the shared ${batchTimeoutMs}ms K7 batch deadline`);
    }
    return remaining;
  };
  return {
    startedAtMs,
    expiresAtMs,
    hardExpiresAtMs,
    batchTimeoutMs,
    executionTimeoutMs,
    cleanupReserveMs,
    configuredChildTimeoutMs: childTimeoutMs,
    remainingMs,
    cleanupRemainingMs,
    assertRemaining,
    signal: controller.signal,
    ref() {
      timer.ref?.();
    },
    dispose() {
      clearTimeout(timer);
    },
    childTimeoutMs(context = "K7 child") {
      return Math.min(childTimeoutMs, assertRemaining(context));
    },
  };
}

export async function withK7Deadline(options, task) {
  const deadline = buildK7Deadline(options);
  deadline.ref();
  const taskPromise = Promise.resolve().then(() => task(deadline));
  let observeDeadline;
  const deadlineReached = new Promise((resolve) => {
    observeDeadline = () => resolve({ source: "deadline" });
    deadline.signal.addEventListener("abort", observeDeadline, { once: true });
  });
  const taskOutcome = taskPromise.then(
    (value) => ({ source: "task", value }),
    (error) => ({ source: "task", error }),
  );
  let cleanupTimer;
  try {
    const outcome = await Promise.race([taskOutcome, deadlineReached]);
    if (outcome.source === "task") {
      if (deadline.signal.aborted) {
        throw deadline.signal.reason instanceof Error
          ? deadline.signal.reason
          : new Error(`K7 batch exhausted the shared ${deadline.batchTimeoutMs}ms deadline`);
      }
      if (Object.hasOwn(outcome, "error")) throw outcome.error;
      return outcome.value;
    }

    const cleanupRemainingMs = deadline.cleanupRemainingMs();
    if (cleanupRemainingMs <= 0) {
      throw new Error(`K7 batch exhausted its total ${deadline.batchTimeoutMs}ms deadline before cleanup could complete`);
    }
    const cleanupHardStop = new Promise((_, reject) => {
      cleanupTimer = setTimeout(() => {
        reject(new Error(`K7 batch cleanup did not settle within the total ${deadline.batchTimeoutMs}ms deadline`));
      }, cleanupRemainingMs);
    });
    await Promise.race([
      taskPromise.then(() => undefined, () => undefined),
      cleanupHardStop,
    ]);
    throw deadline.signal.reason instanceof Error
      ? deadline.signal.reason
      : new Error(`K7 batch exhausted the shared ${deadline.batchTimeoutMs}ms deadline`);
  } finally {
    deadline.signal.removeEventListener("abort", observeDeadline);
    clearTimeout(cleanupTimer);
    deadline.dispose();
    taskPromise.catch(() => undefined);
  }
}

function parseCgroupCpuMax(cpuMax) {
  const [quota, period] = cpuMax.trim().split(/\s+/);
  if (quota === "max") return undefined;
  if (!/^\d+$/.test(quota) || !/^\d+$/.test(period) || Number(period) === 0) return undefined;
  return Math.max(1, Math.floor(Number(quota) / Number(period)));
}

export async function detectK7ResourcePolicy({ availableParallelism = os.availableParallelism, logicalCpuCount = () => os.cpus().length, readCpuMax = () => fsp.readFile("/sys/fs/cgroup/cpu.max", "utf8"), maximumThreadCount = "auto" } = {}) {
  const processAvailableCpuCount = availableParallelism();
  const nodeAvailable = Number.isInteger(processAvailableCpuCount) && processAvailableCpuCount > 0;
  let availableCpuCount;
  let availableCpuSource;
  if (nodeAvailable) {
    availableCpuCount = processAvailableCpuCount;
    availableCpuSource = "node_available_parallelism";
  } else {
    const fallbackCpuCount = logicalCpuCount();
    if (!Number.isInteger(fallbackCpuCount) || fallbackCpuCount <= 0) {
      throw new Error("K7 resource policy cannot detect an available CPU count");
    }
    availableCpuCount = fallbackCpuCount;
    availableCpuSource = "host_logical_cpu_count";
  }
  try {
    const quotaCpuCount = parseCgroupCpuMax(await readCpuMax());
    if (quotaCpuCount !== undefined && quotaCpuCount < availableCpuCount) {
      availableCpuCount = quotaCpuCount;
      availableCpuSource = `${availableCpuSource}+cgroup_cpu_max`;
    }
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }
  return buildK7ResourcePolicy(availableCpuCount, availableCpuSource, maximumThreadCount);
}

function validateK7ResourcePolicy(resourcePolicy) {
  const expected = resourcePolicy === null || typeof resourcePolicy !== "object"
    ? undefined
    : buildK7ResourcePolicy(
      resourcePolicy.available_cpu_count,
      resourcePolicy.available_cpu_source,
      resourcePolicy.maximum_thread_count,
    );
  if (resourcePolicy?.schema !== K7_RESOURCE_POLICY_SCHEMA
    || !Number.isInteger(resourcePolicy.available_cpu_count)
    || resourcePolicy.available_cpu_count <= 0
    || typeof resourcePolicy.available_cpu_source !== "string"
    || resourcePolicy.available_cpu_source.length === 0
    || (resourcePolicy.maximum_thread_count !== "auto"
      && (!Number.isInteger(resourcePolicy.maximum_thread_count) || resourcePolicy.maximum_thread_count <= 0))
    || resourcePolicy.max_concurrent_child_executions !== expected?.max_concurrent_child_executions
    || resourcePolicy.rayon_threads_per_seed !== expected?.rayon_threads_per_seed
    || resourcePolicy.ordinary_test_max_ms !== K7_ORDINARY_TEST_MAX_MS
    || resourcePolicy.child_timeout_ms !== K7_CHILD_TIMEOUT_MS
    || resourcePolicy.batch_timeout_ms !== K7_BATCH_TIMEOUT_MS
    || resourcePolicy.execution_timeout_ms !== K7_BATCH_TIMEOUT_MS - K7_CLEANUP_RESERVE_MS
    || resourcePolicy.cleanup_reserve_ms !== K7_CLEANUP_RESERVE_MS) {
    throw new Error("K7 resource policy must bound concurrent seed processes and their aggregate Rayon CPU budget");
  }
}

export function parseCliArgs(argv) {
  const [command, ...rest] = argv;
  if (!new Set(["before", "after", "sensitivity"]).has(command)) {
    throw new Error(`未知命令 ${JSON.stringify(command ?? "(空)")}；仅支持 before、after、sensitivity`);
  }
  let outputDir;
  let scenarioArg = "all";
  let resume = false;
  let batchSize = Number.POSITIVE_INFINITY;
  let maximumThreadCount = "auto";
  for (let index = 0; index < rest.length; index += 1) {
    const flag = rest[index];
    if (flag === "--resume" && command !== "before") {
      resume = true;
      continue;
    }
    const value = rest[index + 1];
    if (value === undefined) {
      throw new Error(`参数 ${flag} 缺少取值；用法：before --output <目录> [--scenario name|all]`);
    }
    if (flag === "--output") {
      outputDir = value;
      index += 1;
    } else if (flag === "--batch-size" && command !== "before") {
      if (!/^\d+$/.test(value) || Number(value) === 0) throw new Error("--batch-size must be a positive integer");
      batchSize = Number(value);
      index += 1;
    } else if (flag === "--max-threads" && command !== "before") {
      if (value === "auto") {
        maximumThreadCount = "auto";
      } else if (/^\d+$/.test(value) && Number(value) > 0) {
        maximumThreadCount = Number(value);
      } else {
        throw new Error("--max-threads must be a positive integer or auto");
      }
      index += 1;
    } else if (command === "before" && flag === "--scenario") {
      scenarioArg = value;
      index += 1;
    } else {
      throw new Error(`未知参数 ${flag}；用法：before --output <目录> [--scenario name|all]`);
    }
  }
  if (typeof outputDir !== "string" || outputDir.length === 0) {
    throw new Error("缺少必需参数 --output <目录>：before 证据必须落到显式指定的目录");
  }
  if (command !== "before") return { command, outputDir, scenarios: ["primary"], resume, batchSize, maximumThreadCount };
  const scenarios = scenarioArg === "all" ? Object.keys(SCENARIOS) : scenarioArg.split(",");
  for (const name of scenarios) {
    if (!Object.hasOwn(SCENARIOS, name)) {
      throw new Error(`未知场景 ${name}；可选：${Object.keys(SCENARIOS).join("、")} 或 all`);
    }
  }
  return { command, outputDir, scenarios, resume, batchSize };
}

function sameSeedList(left, right) {
  return left.length === right.length && left.every((seed, index) => seed === right[index]);
}

function requireSensitivityMatrix(values, name) {
  if (!sameSeedList(values, SENSITIVITY_MULTIPLIERS)) {
    throw new Error(`${name} multiplier matrix must be exactly 0.5, 1, 2`);
  }
}

function validateK7Output(expected) {
  const { scenario, seed, naturalDays, behavior, event, c01, sourceFingerprintDigest, parsed } = expected;
  if (parsed.source !== "fresh_current_k7_setup") throw new Error("after report source must be fresh_current_k7_setup; save-backed provenance is rejected");
  if (parsed.build_source_fingerprint !== sourceFingerprintDigest) throw new Error("K7 fixture binary/source fingerprint mismatch");
  if (Object.hasOwn(parsed, "save_path") || Object.hasOwn(parsed, "load")) throw new Error("after report must not contain save provenance fields");
  if (parsed.scenario !== scenario || parsed.seed !== String(seed) || parsed.natural_days !== naturalDays) throw new Error("K7 report scenario, seed, or natural-day request mismatch");
  if (parsed.multipliers?.behavior !== behavior || parsed.multipliers?.event !== event || parsed.multipliers?.c01_denominator_assumption !== c01) throw new Error("K7 report multiplier echo mismatch");
  if (parsed.calendar?.natural_days !== naturalDays || typeof parsed.calendar?.trading_days !== "number" || typeof parsed.calendar?.closed_days !== "number") throw new Error("K7 report lacks natural-day calendar accounting");
  if (parsed.calendar.trading_days + parsed.calendar.closed_days !== naturalDays) throw new Error("K7 calendar accounting does not cover every natural day");
  const expectedProfile = scenario === "primary"
    ? { id: "primary-bounded-representative-v1", retail: 64, ticks: 30, opening: 3, closing: 2, start: "2030-01-01" }
    : { id: "cross-year-bounded-representative-v1", retail: 32, ticks: 20, opening: 3, closing: 2, start: "2030-12-27" };
  const profile = parsed.verification_profile;
  if (profile?.schema !== "k7-bounded-representative-profile-v1"
    || profile.profile_id !== expectedProfile.id
    || profile.scope !== "bounded_representative_not_full_market_scale"
    || profile.retail_count !== expectedProfile.retail
    || profile.inst_count !== 5
    || profile.hot_count !== 2
    || profile.stock_count !== 5
    || profile.ticks_per_trading_day !== expectedProfile.ticks
    || profile.opening_auction_ticks !== expectedProfile.opening
    || profile.continuous_ticks !== expectedProfile.ticks - expectedProfile.opening - expectedProfile.closing
    || profile.closing_auction_ticks !== expectedProfile.closing
    || profile.start_date !== expectedProfile.start
    || JSON.stringify(profile.market_phases) !== JSON.stringify(["opening_auction", "continuous", "closing_auction"])) {
    throw new Error(`K7 ${scenario} report lacks the exact bounded representative verification profile`);
  }
  if (parsed.price_volume?.runs?.length !== 1 || parsed.price_volume.runs[0]?.seed !== String(seed)) throw new Error("K7 report must retain the raw per-seed price-volume run");
  const execution = parsed.price_volume.runs[0].retail_execution;
  if (execution?.filled_share_ratio === null && parsed.causal?.ratio_absent_reason !== "no_submissions") throw new Error("zero/no-sample report must retain an explicit no_submissions reason");
}

function quantile(values, probability) {
  const ordered = [...values].sort((left, right) => left - right);
  const index = probability * (ordered.length - 1);
  const lower = Math.floor(index);
  const upper = Math.ceil(index);
  return ordered[lower] + (ordered[upper] - ordered[lower]) * (index - lower);
}

function distribution(values) {
  const mean = values.reduce((total, value) => total + value, 0) / values.length;
  return { sample_count: values.length, minimum: Math.min(...values), p05: quantile(values, 0.05), p25: quantile(values, 0.25), median: quantile(values, 0.5), p75: quantile(values, 0.75), p95: quantile(values, 0.95), maximum: Math.max(...values), mean };
}

function summarizeK7Runs(runs) {
  const perStock = {};
  for (const run of runs) {
    for (const [code, stock] of Object.entries(run.raw.price_volume.runs[0].stocks)) {
      (perStock[code] ??= []).push({ seed: run.seed, stock });
    }
  }
  return Object.fromEntries(Object.entries(perStock).map(([code, entries]) => {
    const metric = (name) => distribution(entries.map(({ stock }) => stock[name]));
    const worstLiquidity = entries.reduce((worst, entry) => entry.stock.zero_volume_days > worst.stock.zero_volume_days ? entry : worst);
    const largestDrawdown = entries.reduce((worst, entry) => entry.stock.maximum_drawdown_bps > worst.stock.maximum_drawdown_bps ? entry : worst);
    const largestReturn = entries.reduce((worst, entry) => Math.abs(entry.stock.terminal_return_bps) > Math.abs(worst.stock.terminal_return_bps) ? entry : worst);
    return [code, { raw_seed_count: entries.length, mean_daily_volume: metric("mean_daily_volume"), zero_volume_day_ratio: metric("zero_volume_days"), terminal_return_bps: metric("terminal_return_bps"), maximum_drawdown_bps: metric("maximum_drawdown_bps"), extremes: { highest_zero_volume_days: { seed: worstLiquidity.seed, value: worstLiquidity.stock.zero_volume_days }, largest_absolute_terminal_return_bps: { seed: largestReturn.seed, value: Math.abs(largestReturn.stock.terminal_return_bps) }, maximum_drawdown_bps: { seed: largestDrawdown.seed, value: largestDrawdown.stock.maximum_drawdown_bps } } }];
  }));
}

function expectField(scenarioName, seed, field, expected, actual) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(
      `报告字段 ${field} 与请求配置不符（场景 ${scenarioName}，seed ${seed}）：期望 ${JSON.stringify(expected)}，实际 ${JSON.stringify(actual)}`,
    );
  }
}

/** u64 十进制字符串 → BigInt；格式非法时带场景与 seed 显式报错，绝不抛裸 SyntaxError。 */
function decimalBigInt(scenarioName, seed, field, value) {
  if (typeof value !== "string" || !/^\d+$/.test(value)) {
    throw new Error(
      `报告字段 ${field} 必须是十进制字符串（场景 ${scenarioName}，seed ${seed}）：实际 ${JSON.stringify(value)}`,
    );
  }
  return BigInt(value);
}

/**
 * 对账单份 fixture 输出：工具身份、场景、seed、配置回显、交易日数，以及引擎自身的
 * 逐笔↔日 K 成交量/成交额恒等式、双边参与量恒等式和逐股极端样本保留。
 * u64 字段在 JSON 中是十进制字符串，相加时必须用 BigInt。
 *
 * `engine_error_events`（SettlementError/VError 计数）是引擎显式报告的行为指标：
 * 当前合法默认 setup 下可能非零，必须如实采集并在 manifest 与日志中显式呈现，
 * 绝不静默丢弃或伪装成 0；这里只校验字段是十进制字符串。
 */
export function validateFixtureOutput(scenarioName, seed, tradingDays, parsed) {
  const scenario = SCENARIOS[scenarioName];
  const ctx = [scenarioName, seed];
  expectField(...ctx, "tool", "baseline_fixture", parsed.tool);
  expectField(...ctx, "scenario", scenarioName, parsed.scenario);
  expectField(...ctx, "seed", String(seed), parsed.seed);
  expectField(...ctx, "trading_days", tradingDays, parsed.trading_days);

  expectField(...ctx, "config.stock_codes", [...scenario.stockCodes].sort(), parsed.config?.stock_codes);
  expectField(...ctx, "config.retail_count", scenario.retailCount, parsed.config?.retail_count);
  expectField(...ctx, "config.inst_count", scenario.instCount, parsed.config?.inst_count);
  expectField(...ctx, "config.hot_count", scenario.hotCount, parsed.config?.hot_count);
  expectField(...ctx, "config.ticks_per_day", String(scenario.ticksPerDay), parsed.config?.ticks_per_day);
  expectField(...ctx, "config.auction_ticks", String(scenario.auctionTicks), parsed.config?.auction_ticks);
  expectField(
    ...ctx,
    "config.closing_auction_ticks",
    String(scenario.closingAuctionTicks),
    parsed.config?.closing_auction_ticks,
  );
  expectField(...ctx, "config.history_len", scenario.historyLen, parsed.config?.history_len);
  expectField(...ctx, "config.t1_enabled", scenario.t1Enabled, parsed.config?.t1_enabled);

  expectField(...ctx, "report.trading_days", tradingDays, parsed.report?.trading_days);
  expectField(...ctx, "report.ticks_per_day", String(scenario.ticksPerDay), parsed.report?.ticks_per_day);
  expectField(...ctx, "report.runs 数量", 1, parsed.report?.runs?.length);

  const run = parsed.report.runs[0];
  expectField(...ctx, "report.runs[0].seed", String(seed), run.seed);
  if (typeof run.engine_error_events !== "string" || !/^\d+$/.test(run.engine_error_events)) {
    throw new Error(
      `报告字段 report.runs[0].engine_error_events 必须是十进制字符串（场景 ${scenarioName}，seed ${seed}）：实际 ${JSON.stringify(run.engine_error_events)}`,
    );
  }

  const sortedCodes = [...scenario.stockCodes].sort();
  expectField(...ctx, "report.runs[0].stocks 代码集合", sortedCodes, Object.keys(run.stocks ?? {}).sort());

  let totalVolume = 0n;
  for (const [code, stock] of Object.entries(run.stocks)) {
    expectField(...ctx, `report.runs[0].stocks[${code}].completed_days`, tradingDays, stock.completed_days);
    if (stock.zero_volume_days + stock.traded_days !== stock.completed_days) {
      throw new Error(
        `报告字段 report.runs[0].stocks[${code}].zero_volume_days + traded_days 与 completed_days 不符（场景 ${scenarioName}，seed ${seed}）：期望 ${stock.completed_days}，实际 ${stock.zero_volume_days + stock.traded_days}`,
      );
    }
    expectField(...ctx, `report.runs[0].stocks[${code}].daily_returns_bps 天数`, tradingDays, stock.daily_returns_bps?.length);
    expectField(
      ...ctx,
      `report.runs[0].stocks[${code}].trade_event_volume`,
      stock.total_daily_volume,
      stock.trade_event_volume,
    );
    expectField(
      ...ctx,
      `report.runs[0].stocks[${code}].trade_event_turnover_cents`,
      stock.total_daily_turnover_cents,
      stock.trade_event_turnover_cents,
    );
    const split =
      decimalBigInt(scenarioName, seed, `report.runs[0].stocks[${code}].auction_volume`, stock.auction_volume) +
      decimalBigInt(scenarioName, seed, `report.runs[0].stocks[${code}].continuous_volume`, stock.continuous_volume);
    if (split !== decimalBigInt(scenarioName, seed, `report.runs[0].stocks[${code}].total_daily_volume`, stock.total_daily_volume)) {
      throw new Error(
        `报告字段 report.runs[0].stocks[${code}].auction_volume + continuous_volume 与 total_daily_volume 不符（场景 ${scenarioName}，seed ${seed}）：期望 ${stock.total_daily_volume}，实际 ${split}`,
      );
    }
    totalVolume += decimalBigInt(scenarioName, seed, `report.runs[0].stocks[${code}].total_daily_volume`, stock.total_daily_volume);
  }

  const twoSided = decimalBigInt(
    scenarioName,
    seed,
    "report.runs[0].participant_execution.two_sided_participant_shares",
    run.participant_execution?.two_sided_participant_shares,
  );
  if (twoSided !== totalVolume * 2n) {
    throw new Error(
      `报告字段 report.runs[0].participant_execution.two_sided_participant_shares 与市场成交量两倍不符（场景 ${scenarioName}，seed ${seed}）：期望 ${totalVolume * 2n}，实际 ${twoSided}`,
    );
  }

  expectField(...ctx, "report.stocks（跨 seed 汇总）代码集合", sortedCodes, Object.keys(parsed.report.stocks ?? {}).sort());

  const extremeCases = parsed.report.extreme_cases ?? [];
  const extremeCodes = new Set(extremeCases.map((entry) => entry.code));
  if (extremeCases.length === 0 || extremeCodes.size !== scenario.stockCodes.length) {
    throw new Error(
      `报告字段 report.extreme_cases 未保留每股极端样本（场景 ${scenarioName}，seed ${seed}）：期望覆盖 ${JSON.stringify(sortedCodes)}，实际覆盖 ${JSON.stringify([...extremeCodes])}`,
    );
  }
  const foreignSeed = extremeCases.find((entry) => entry.seed !== String(seed));
  if (foreignSeed !== undefined) {
    throw new Error(
      `报告字段 report.extreme_cases 混入了其他 seed 的样本（场景 ${scenarioName}，seed ${seed}）：${foreignSeed.code} → seed ${foreignSeed.seed}`,
    );
  }
}

function terminateProcessTree(child) {
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

/** 真实子进程执行；超时或无法启动都显式抛错，绝不返回伪造的成功结果。 */
export function realExec(file, args, { cwd, timeoutMs, env, signal }) {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) {
      reject(new Error(`${file} ${args.join(" ")} 未启动：共享 K7 批次截止时间已耗尽`));
      return;
    }
    const child = spawn(file, args, {
      cwd,
      env,
      windowsHide: true,
      detached: process.platform !== "win32",
    });
    let stdout = "";
    let stderr = "";
    let terminationReason;
    let settled = false;
    const cleanup = () => {
      clearTimeout(timer);
      signal?.removeEventListener("abort", abortChild);
    };
    const abortChild = () => {
      terminationReason = `共享 K7 批次截止时间（最多 ${K7_BATCH_TIMEOUT_MS}ms）已耗尽`;
      terminateProcessTree(child);
    };
    const timer = setTimeout(() => {
      terminationReason = `子进程超过 ${timeoutMs}ms`;
      terminateProcessTree(child);
    }, timeoutMs);
    signal?.addEventListener("abort", abortChild, { once: true });
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("error", (error) => {
      if (settled) return;
      settled = true;
      cleanup();
      reject(new Error(`无法启动 ${file} ${args.join(" ")}：${error.message}`));
    });
    child.on("close", (code) => {
      if (settled) return;
      settled = true;
      cleanup();
      if (terminationReason !== undefined || code === null) {
        reject(new Error(`${file} ${args.join(" ")} ${terminationReason ?? "被异常终止"}`));
        return;
      }
      resolve({ code, stdout, stderr });
    });
  });
}

async function ensureFreshOutputDir(outputDir) {
  let entries;
  try {
    entries = await fsp.readdir(outputDir);
  } catch (error) {
    if (error?.code !== "ENOENT") {
      throw error;
    }
    return;
  }
  if (entries.length > 0) {
    throw new Error(`输出目录已存在且非空：${outputDir}（${entries.length} 个条目）；为避免混入旧证据，请换新目录或先手动清空`);
  }
}

async function mustSucceed(exec, repoRoot, file, args, deadline) {
  const timeoutMs = deadline === undefined
    ? Math.min(60_000, K7_CHILD_TIMEOUT_MS)
    : Math.min(60_000, deadline.childTimeoutMs(`${file} ${args.join(" ")}`));
  const { code, stdout, stderr } = await exec(file, args, {
    cwd: repoRoot,
    timeoutMs,
    signal: deadline?.signal,
  });
  deadline?.assertRemaining(`${file} ${args.join(" ")}`);
  if (code !== 0) {
    throw new Error(`采集 ${file} ${args.join(" ")} 失败（退出码 ${code}）：${stderr.trim()}`);
  }
  return stdout;
}

async function collectGit(exec, repoRoot, deadline) {
  const revision = (await mustSucceed(exec, repoRoot, "git", ["rev-parse", "HEAD"], deadline)).trim();
  const branch = (await mustSucceed(exec, repoRoot, "git", ["branch", "--show-current"], deadline)).trim();
  const porcelain = await mustSucceed(exec, repoRoot, "git", ["status", "--porcelain"], deadline);
  const dirtyPaths = porcelain.split(/\r?\n/).filter((line) => line.length > 0);
  return { revision, branch, dirty_paths: dirtyPaths };
}

async function collectK7Git(exec, repoRoot, deadline) {
  const git = await collectGit(exec, repoRoot, deadline);
  const dirtyPaths = (await mustSucceed(exec, repoRoot, "git", ["status", "--porcelain", "--", ...K7_SOURCE_INPUTS], deadline))
    .split(/\r?\n/)
    .filter((line) => line.length > 0);
  return { ...git, dirty_paths: dirtyPaths };
}

async function collectK7SourceFingerprint(exec, repoRoot, deadline) {
  const sourceArguments = ["--", ...K7_SOURCE_INPUTS];
  const committedTree = (await mustSucceed(exec, repoRoot, "git", ["rev-parse", "HEAD^{tree}"], deadline)).trim();
  const dirtyPatch = await mustSucceed(exec, repoRoot, "git", ["diff", "--binary", "--no-ext-diff", "HEAD", ...sourceArguments], deadline);
  const files = [];
  async function collectFiles(relativePath) {
    deadline.assertRemaining(`source fingerprint ${relativePath}`);
    let entry;
    try {
      entry = await fsp.lstat(path.join(repoRoot, relativePath));
    } catch (error) {
      if (error?.code === "ENOENT") return;
      throw error;
    }
    if (entry.isFile()) {
      files.push({
        path: relativePath,
        sha256: sha256(await fsp.readFile(path.join(repoRoot, relativePath), { signal: deadline.signal })),
      });
      return;
    }
    if (!entry.isDirectory()) throw new Error(`K7 source input has unsupported filesystem entry: ${relativePath}`);
    const entries = await fsp.readdir(path.join(repoRoot, relativePath), { withFileTypes: true });
    for (const entry of entries.sort((left, right) => left.name.localeCompare(right.name))) {
      const childPath = path.posix.join(relativePath, entry.name);
      if (entry.isDirectory()) {
        await collectFiles(childPath);
      } else if (entry.isFile()) {
        files.push({
          path: childPath,
          sha256: sha256(await fsp.readFile(path.join(repoRoot, childPath), { signal: deadline.signal })),
        });
      } else {
        throw new Error(`K7 source input has unsupported filesystem entry: ${childPath}`);
      }
    }
  }
  for (const sourceInput of K7_SOURCE_INPUTS) await collectFiles(sourceInput);
  deadline.assertRemaining("source fingerprint completion");
  const state = {
    algorithm: K7_SOURCE_FINGERPRINT_ALGORITHM,
    committed_tree: committedTree,
    dirty_patch_sha256: sha256(dirtyPatch),
    files,
  };
  return { ...state, digest: sha256(JSON.stringify(state)) };
}

function parseK7ExecutableFromCargoMessages(stdout, workspaceRoot) {
  let executablePath;
  for (const line of stdout.split(/\r?\n/)) {
    if (line.trim().length === 0) continue;
    let message;
    try {
      message = JSON.parse(line);
    } catch (error) {
      throw new Error(`K7 fixture build emitted malformed Cargo JSON: ${error.message}`);
    }
    if (message.reason === "compiler-artifact"
      && message.target?.name === "k7_baseline_fixture"
      && message.target?.kind?.includes("example")
      && typeof message.executable === "string") {
      executablePath = path.resolve(message.executable);
    }
  }
  if (executablePath === undefined) throw new Error("K7 fixture build did not report its executable artifact");
  const relativePath = path.relative(workspaceRoot, executablePath);
  if (relativePath.length === 0 || relativePath.startsWith("..") || path.isAbsolute(relativePath)) {
    throw new Error(`K7 fixture executable must stay inside the workspace: ${executablePath}`);
  }
  return { executablePath, relativePath: relativePath.split(path.sep).join("/") };
}

export async function prepareK7FixtureExecutable({ exec = realExec, repoRoot = REPO_ROOT, workspacePaths, deadline, resourcePolicy, sourceFingerprint }) {
  const buildArgs = buildK7FixtureBuildArgs();
  const cargoBuildJobs = resourcePolicy.maximum_thread_count === "auto"
    ? resourcePolicy.available_cpu_count
    : Math.min(resourcePolicy.available_cpu_count, resourcePolicy.maximum_thread_count);
  workspacePaths ??= await prepareWorkspacePaths({ sourceRoot: repoRoot, scope: "k7" });
  const workspaceEnv = buildK7WorkspaceEnv(workspacePaths);
  const startedAt = Date.now();
  const { code, stdout, stderr } = await exec("cargo", buildArgs, {
    cwd: repoRoot,
    timeoutMs: deadline.childTimeoutMs("K7 fixture build"),
    signal: deadline.signal,
    env: {
      ...workspaceEnv,
      CARGO_BUILD_JOBS: String(cargoBuildJobs),
      K7_SOURCE_FINGERPRINT_DIGEST: sourceFingerprint.digest,
    },
  });
  deadline.assertRemaining("K7 fixture build");
  if (code !== 0) {
    throw new Error(`K7 fixture build failed with exit ${code}: ${stderr.trim()}`);
  }
  const { executablePath, relativePath } = parseK7ExecutableFromCargoMessages(stdout, workspacePaths.workspaceRoot);
  const binary = await fsp.readFile(executablePath, { signal: deadline.signal });
  deadline.assertRemaining("K7 fixture binary hashing");
  return {
    executable_path: executablePath,
    executable_relative_path: relativePath,
    binary_sha256: sha256(binary),
    binary_bytes: binary.length,
    build_argv: ["cargo", ...buildArgs],
    cargo_build_jobs: cargoBuildJobs,
    embedded_source_fingerprint_digest: sourceFingerprint.digest,
    workspace_root: workspacePaths.workspaceRoot,
    cargo_target_dir: workspacePaths.cargoTargetDir,
    process_tmp_dir: workspacePaths.processTmpDir,
    build_wall_ms: Date.now() - startedAt,
  };
}

/** 工具链版本逐项采集；某项不可用时显式记录 available:false 与原因，绝不静默省略。 */
async function collectToolchain(exec, repoRoot, deadline) {
  const toolchain = {};
  const blocked = [];
  for (const [name, args] of [
    ["node", ["--version"]],
    ["cargo", ["--version"]],
  ]) {
    toolchain[name] = {
      available: true,
      version: (await mustSucceed(exec, repoRoot, name, args, deadline)).trim(),
    };
  }
  try {
    toolchain.pnpm = {
      available: true,
      version: (await mustSucceed(exec, repoRoot, "corepack", ["pnpm", "--version"], deadline)).trim(),
    };
  } catch (error) {
    toolchain.pnpm = { available: false, error: error.message };
    blocked.push(`corepack pnpm --version 不可用：${error.message}`);
  }
  return { toolchain, blocked };
}

function platformSummary() {
  const cpus = os.cpus();
  return {
    os: os.type(),
    release: os.release(),
    arch: process.arch,
    cpu_model: cpus[0]?.model ?? "unknown",
    cpu_count: cpus.length,
    total_memory_bytes: os.totalmem(),
  };
}

async function runOneFixture({ scenarioName, seed, dir, exec, repoRoot, timeoutMs, signal, write }) {
  const args = buildExampleArgs(scenarioName, seed, TRADING_DAYS);
  const startedAt = Date.now();
  const { code, stdout, stderr } = await exec("cargo", args, { cwd: repoRoot, timeoutMs, signal });
  const wallMs = Date.now() - startedAt;
  if (code !== 0) {
    throw new Error(
      `${scenarioName} seed ${seed} 运行失败（退出码 ${code}）：cargo ${args.join(" ")}\nstderr：${stderr.trim().slice(0, 2000) || "(空)"}`,
    );
  }
  if (stdout.trim().length === 0) {
    throw new Error(`${scenarioName} seed ${seed} 未输出报告 JSON（stdout 0 字节）：cargo ${args.join(" ")}`);
  }
  let parsed;
  try {
    parsed = JSON.parse(stdout);
  } catch (error) {
    throw new Error(`${scenarioName} seed ${seed} 报告不是合法 JSON：${error.message}`);
  }
  validateFixtureOutput(scenarioName, seed, TRADING_DAYS, parsed);
  const buffer = Buffer.from(stdout, "utf8");
  const sha256 = createHash("sha256").update(buffer).digest("hex");
  if (write) {
    await fsp.writeFile(path.join(dir, `seed-${seed}.json`), buffer);
  }
  const run0 = parsed.report.runs[0];
  return {
    seed,
    argv: ["cargo", ...args],
    cwd: repoRoot,
    exit_code: code,
    wall_ms: wallMs,
    stdout_bytes: buffer.length,
    sha256,
    engine_error_events: run0.engine_error_events,
    trade_events: run0.trade_events,
  };
}

async function captureScenario({ scenarioName, outputDir, exec, repoRoot, timeoutMs, deadline, log }) {
  const scenarioDir = path.join(outputDir, scenarioName);
  await fsp.mkdir(scenarioDir, { recursive: true });
  const runs = [];
  for (const seed of MATRIX_SEEDS) {
    const run = await runOneFixture({
      scenarioName,
      seed,
      dir: scenarioDir,
      exec,
      repoRoot,
      timeoutMs: Math.min(timeoutMs, deadline.childTimeoutMs(`${scenarioName} seed ${seed}`)),
      signal: deadline.signal,
      write: true,
    });
    log(`[${scenarioName}] seed ${run.seed} 完成：${run.wall_ms}ms，sha256=${run.sha256.slice(0, 12)}…`);
    if (run.engine_error_events !== "0") {
      log(
        `[${scenarioName}] seed ${run.seed} 注意：engine_error_events=${run.engine_error_events}（当前行为的显式记录，已写入 manifest，不静默）`,
      );
    }
    runs.push(run);
  }

  const firstSeed = MATRIX_SEEDS[0];
  const rerun = await runOneFixture({
    scenarioName,
    seed: firstSeed,
    dir: scenarioDir,
    exec,
    repoRoot,
    timeoutMs: Math.min(timeoutMs, deadline.childTimeoutMs(`${scenarioName} determinism rerun`)),
    signal: deadline.signal,
    write: false,
  });
  const firstDigest = runs.find((run) => run.seed === firstSeed).sha256;
  if (rerun.sha256 !== firstDigest) {
    throw new Error(
      `${scenarioName} seed ${firstSeed} 两次运行摘要不一致：first=${firstDigest} rerun=${rerun.sha256}；同 seed 同配置必须逐字节一致（确定性被破坏）`,
    );
  }
  log(`[${scenarioName}] 确定性复核通过：seed ${firstSeed} 两次 sha256 相同`);
  return {
    definition: SCENARIOS[scenarioName],
    runs,
    determinism_check: {
      seed: firstSeed,
      first_digest: firstDigest,
      rerun_digest: rerun.sha256,
      identical: true,
      rerun_wall_ms: rerun.wall_ms,
    },
  };
}

export async function captureBaseline({
  outputDir,
  exec = realExec,
  repoRoot = REPO_ROOT,
  scenarios = Object.keys(SCENARIOS),
  timeoutMs = resolveBaselineFixtureTimeoutMs(),
  batchTimeoutMs = K7_BATCH_TIMEOUT_MS,
  command = "before",
  log = console.log,
}) {
  return withK7Deadline({ childTimeoutMs: timeoutMs, batchTimeoutMs }, async (deadline) => {
  validateSeedMatrix(MATRIX_SEEDS);
  await ensureFreshOutputDir(outputDir);
  await fsp.mkdir(outputDir, { recursive: true });

  const git = await collectGit(exec, repoRoot, deadline);
  const { toolchain, blocked } = await collectToolchain(exec, repoRoot, deadline);
  for (const item of blocked) {
    log(`显式记录（不静默省略）：${item}`);
  }

  const manifest = {
    command,
    scope: "historical_baseline_capture_not_current_acceptance",
    generated_at: new Date().toISOString(),
    runner: { script: "scripts/simulation/baseline-run.mjs", node: process.version, matrix_seeds: [...MATRIX_SEEDS], trading_days: TRADING_DAYS },
    git,
    toolchain,
    blocked,
    platform: platformSummary(),
    scenarios: {},
  };

  for (const scenarioName of scenarios) {
    manifest.scenarios[scenarioName] = await captureScenario({
      scenarioName,
      outputDir,
      exec,
      repoRoot,
      timeoutMs,
      deadline,
      log,
    });
  }

  deadline.assertRemaining("before manifest publication");
  const manifestPath = path.join(outputDir, "manifest.json");
  await publishBeforeDeadline(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`, writeAtomically, deadline);
  log(`manifest 已写入：${manifestPath}`);
  return manifest;
  });
}

export async function writeAtomically(filePath, content, { renameFile = fsp.rename } = {}) {
  const temporaryPath = `${filePath}.${process.pid}.${Date.now()}.tmp`;
  try {
    await fsp.writeFile(temporaryPath, content);
    await renameFile(temporaryPath, filePath);
  } finally {
    await fsp.rm(temporaryPath, { force: true });
  }
}

export async function publishBeforeDeadline(filePath, content, atomicWrite, deadline, {
  renameFile = fsp.rename,
  removeFile = fsp.rm,
  readFile = fsp.readFile,
} = {}) {
  deadline.assertRemaining(`publication of ${filePath}`);
  const stagedPath = `${filePath}.${process.pid}.${Date.now()}.deadline-stage`;
  let previousContent;
  let hadPreviousFile = false;
  let published = false;
  try {
    try {
      previousContent = await readFile(filePath);
      hadPreviousFile = true;
    } catch (error) {
      if (error?.code !== "ENOENT") throw error;
    }
    await atomicWrite(stagedPath, content, {
      finalPath: filePath,
      signal: deadline.signal,
    });
    deadline.assertRemaining(`publication of ${filePath}`);
    if (deadline.signal.aborted) throw deadline.signal.reason;
    await renameFile(stagedPath, filePath);
    published = true;
    deadline.assertRemaining(`publication of ${filePath}`);
  } catch (error) {
    if (published) {
      try {
        if (hadPreviousFile) {
          await writeAtomically(filePath, previousContent);
        } else {
          await removeFile(filePath, { force: true });
        }
      } catch (rollbackError) {
        throw new AggregateError(
          [error, rollbackError],
          `publication of ${filePath} crossed its deadline and rollback failed`,
        );
      }
    }
    throw error;
  } finally {
    await removeFile(stagedPath, { force: true });
  }
}

function sha256(content) {
  return createHash("sha256").update(content).digest("hex");
}

function k7Identity({ git, sourceFingerprint, preparedFixture, resourcePolicy, scenario, seeds, naturalDays, behavior, event, c01 }) {
  return {
    runner: { schema: K7_CHECKPOINT_SCHEMA, version: K7_RUNNER_VERSION, script: "scripts/simulation/baseline-run.mjs" },
    git: { revision: git.revision, dirty_paths: git.dirty_paths },
    source_fingerprint: sourceFingerprint,
    resource_policy: resourcePolicy,
    fixture: {
      source: "fresh_current_k7_setup",
      executable_relative_path: preparedFixture.executable_relative_path,
      binary_sha256: preparedFixture.binary_sha256,
      binary_bytes: preparedFixture.binary_bytes,
      embedded_source_fingerprint_digest: preparedFixture.embedded_source_fingerprint_digest,
      build_argv: preparedFixture.build_argv,
      cargo_build_jobs: preparedFixture.cargo_build_jobs,
      workspace_root: preparedFixture.workspace_root,
      cargo_target_dir: preparedFixture.cargo_target_dir,
      process_tmp_dir: preparedFixture.process_tmp_dir,
      argv: [preparedFixture.executable_relative_path, ...buildK7ExampleArgs(scenario, "<seed>", naturalDays, behavior, event, c01)],
    },
    scenario,
    ordered_seeds: [...seeds],
    natural_days: naturalDays,
    multipliers: { behavior, event, c01_denominator_assumption: c01 },
  };
}

function checkpointDigest(checkpoint) {
  return sha256(JSON.stringify({ schema: checkpoint.schema, identity: checkpoint.identity, identity_digest: checkpoint.identity_digest, completed: checkpoint.completed }));
}

function determinismReceiptDigest(receipt) {
  return sha256(JSON.stringify({ schema: receipt.schema, identity: receipt.identity, identity_digest: receipt.identity_digest, seed: receipt.seed, first_digest: receipt.first_digest, rerun_digest: receipt.rerun_digest, identical: receipt.identical, argv: receipt.argv, exit_code: receipt.exit_code, wall_ms: receipt.wall_ms, revision: receipt.revision, source_fingerprint_digest: receipt.source_fingerprint_digest }));
}

function createExecutionPermit(limit) {
  if (limit !== Number.POSITIVE_INFINITY && (!Number.isInteger(limit) || limit < 0)) {
    throw new Error("K7 batch size must be a non-negative integer");
  }
  return { remaining: limit, used: 0 };
}

function acquireExecutionPermit(permit) {
  if (permit.remaining === 0) return false;
  if (permit.remaining !== Number.POSITIVE_INFINITY) permit.remaining -= 1;
  permit.used += 1;
  return true;
}

function createK7ExecutionPool(maxConcurrent) {
  if (!Number.isInteger(maxConcurrent) || maxConcurrent <= 0) {
    throw new Error("K7 execution pool concurrency must be a positive integer");
  }
  let active = 0;
  const waiters = [];
  const acquire = async () => {
    if (active >= maxConcurrent) {
      await new Promise((resolve) => waiters.push(resolve));
    }
    active += 1;
  };
  const release = () => {
    active -= 1;
    const next = waiters.shift();
    if (next !== undefined) next();
  };
  return {
    async run(task) {
      await acquire();
      try {
        return await task();
      } finally {
        release();
      }
    },
  };
}

async function settleAllOrThrow(promises) {
  const outcomes = await Promise.allSettled(promises);
  const failure = outcomes.find((outcome) => outcome.status === "rejected");
  if (failure !== undefined) throw failure.reason;
  return outcomes.map((outcome) => outcome.value);
}

function validateCheckpoint(checkpoint, identity) {
  if (checkpoint === null || typeof checkpoint !== "object" || Array.isArray(checkpoint)) throw new Error("K7 checkpoint must be a JSON object");
  if (checkpoint.schema !== K7_CHECKPOINT_SCHEMA) throw new Error(`K7 checkpoint schema is unsupported or legacy: ${JSON.stringify(checkpoint.schema)}`);
  const identityDigest = sha256(JSON.stringify(identity));
  if (JSON.stringify(checkpoint.identity?.resource_policy) !== JSON.stringify(identity.resource_policy)) {
    throw new Error("K7 checkpoint resource policy mismatch");
  }
  if (checkpoint.identity_digest !== identityDigest || JSON.stringify(checkpoint.identity) !== JSON.stringify(identity)) throw new Error("K7 checkpoint identity/revision/configuration mismatch");
  if (!Array.isArray(checkpoint.completed)) throw new Error("K7 checkpoint completed entries must be an array");
  if (checkpoint.checkpoint_digest !== checkpointDigest(checkpoint)) throw new Error("K7 checkpoint digest mismatch");
  const seen = new Set();
  for (const entry of checkpoint.completed) {
    if (!Number.isInteger(entry?.seed) || seen.has(entry.seed)) throw new Error(`K7 checkpoint has duplicate or malformed seed: ${entry?.seed}`);
    if (!identity.ordered_seeds.includes(entry.seed)) throw new Error(`K7 checkpoint seed ${entry.seed} is not in the exact requested matrix`);
    if (typeof entry.file !== "string" || !/^[a-f0-9]{64}$/.test(entry.sha256 ?? "")) throw new Error(`K7 checkpoint seed ${entry.seed} lacks a valid raw artifact digest`);
    if (entry.revision !== identity.git.revision || JSON.stringify(entry.dirty_paths) !== JSON.stringify(identity.git.dirty_paths) || entry.source_fingerprint_digest !== identity.source_fingerprint.digest) throw new Error(`K7 checkpoint seed ${entry.seed} source fingerprint or git provenance mismatch`);
    if (!sameSeedList(entry.ordered_seeds, identity.ordered_seeds)) throw new Error(`K7 checkpoint seed ${entry.seed} ordered seed matrix mismatch`);
    if (JSON.stringify(entry.argv) !== JSON.stringify(identity.fixture.argv.map((value) => value === "<seed>" ? String(entry.seed) : value))) throw new Error(`K7 checkpoint seed ${entry.seed} argv mismatch`);
    if (entry.exit_code !== 0 || !Number.isFinite(entry.wall_ms) || entry.wall_ms < 0) throw new Error(`K7 checkpoint seed ${entry.seed} execution metadata is malformed`);
    seen.add(entry.seed);
  }
}

async function readCheckpoint(checkpointPath, identity) {
  let text;
  try {
    text = await fsp.readFile(checkpointPath, "utf8");
  } catch (error) {
    if (error?.code === "ENOENT") return undefined;
    throw error;
  }
  let checkpoint;
  try {
    checkpoint = JSON.parse(text);
  } catch (error) {
    throw new Error(`K7 checkpoint JSON is malformed: ${error.message}`);
  }
  validateCheckpoint(checkpoint, identity);
  return checkpoint;
}

async function persistCheckpoint(checkpointPath, identity, completed, atomicWrite, deadline) {
  const checkpoint = { schema: K7_CHECKPOINT_SCHEMA, identity, identity_digest: sha256(JSON.stringify(identity)), completed };
  checkpoint.checkpoint_digest = checkpointDigest(checkpoint);
  await publishBeforeDeadline(checkpointPath, `${JSON.stringify(checkpoint, null, 2)}\n`, atomicWrite, deadline);
  return checkpoint;
}

async function persistSeedCheckpoint(dir, identity, entry, atomicWrite, deadline) {
  const checkpointPath = path.join(dir, `seed-${entry.seed}.checkpoint.json`);
  return persistCheckpoint(checkpointPath, identity, [entry], atomicWrite, deadline);
}

function validateDeterminismReceipt(receipt, identity, canonical) {
  if (receipt === null || typeof receipt !== "object" || Array.isArray(receipt)) throw new Error("K7 determinism receipt must be a JSON object");
  if (receipt.schema !== K7_DETERMINISM_RECEIPT_SCHEMA) throw new Error(`K7 determinism receipt schema is unsupported: ${JSON.stringify(receipt.schema)}`);
  const identityDigest = sha256(JSON.stringify(identity));
  if (receipt.identity_digest !== identityDigest || JSON.stringify(receipt.identity) !== JSON.stringify(identity)) throw new Error("K7 determinism receipt identity/source fingerprint mismatch");
  if (receipt.seed !== canonical.seed || receipt.first_digest !== canonical.sha256 || receipt.rerun_digest !== canonical.sha256 || receipt.identical !== true) throw new Error("K7 determinism receipt digest or canonical seed mismatch");
  if (JSON.stringify(receipt.argv) !== JSON.stringify(canonical.argv) || receipt.exit_code !== 0 || !Number.isFinite(receipt.wall_ms) || receipt.wall_ms < 0) throw new Error("K7 determinism receipt execution metadata is malformed");
  if (receipt.revision !== identity.git.revision || receipt.source_fingerprint_digest !== identity.source_fingerprint.digest) throw new Error("K7 determinism receipt source provenance mismatch");
  if (receipt.receipt_digest !== determinismReceiptDigest(receipt)) throw new Error("K7 determinism receipt digest mismatch");
}

async function readDeterminismReceipt(receiptPath, identity, canonical) {
  let text;
  try {
    text = await fsp.readFile(receiptPath, "utf8");
  } catch (error) {
    if (error?.code === "ENOENT") return undefined;
    throw error;
  }
  let receipt;
  try {
    receipt = JSON.parse(text);
  } catch (error) {
    throw new Error(`K7 determinism receipt JSON is malformed: ${error.message}`);
  }
  validateDeterminismReceipt(receipt, identity, canonical);
  return receipt;
}

async function persistDeterminismReceipt(receiptPath, identity, canonical, rerun, atomicWrite, deadline) {
  const receipt = {
    schema: K7_DETERMINISM_RECEIPT_SCHEMA,
    identity,
    identity_digest: sha256(JSON.stringify(identity)),
    seed: canonical.seed,
    first_digest: canonical.sha256,
    rerun_digest: rerun.sha256,
    identical: true,
    argv: rerun.argv,
    exit_code: rerun.exit_code,
    wall_ms: rerun.wall_ms,
    revision: identity.git.revision,
    source_fingerprint_digest: identity.source_fingerprint.digest,
  };
  receipt.receipt_digest = determinismReceiptDigest(receipt);
  await publishBeforeDeadline(receiptPath, `${JSON.stringify(receipt, null, 2)}\n`, atomicWrite, deadline);
  return receipt;
}

async function recoverSeedReceipts(dir, identity) {
  const entries = await fsp.readdir(dir);
  const receipts = entries.filter((entry) => /^seed-\d+\.checkpoint\.json$/.test(entry)).sort();
  const completed = [];
  for (const receiptName of receipts) {
    const receipt = await readCheckpoint(path.join(dir, receiptName), identity);
    if (receipt === undefined || receipt.completed.length !== 1) throw new Error(`K7 per-seed checkpoint is malformed: ${receiptName}`);
    completed.push(receipt.completed[0]);
  }
  const seen = new Set();
  for (const entry of completed) {
    if (seen.has(entry.seed)) throw new Error(`K7 per-seed checkpoint recovery has duplicate seed: ${entry.seed}`);
    seen.add(entry.seed);
  }
  return completed;
}

async function captureK7Run({ outputDir, exec, repoRoot, deadline, preparedFixture, resourcePolicy, executionPool, scenario, seed, naturalDays, behavior, event, c01, write, permit, atomicWrite = writeAtomically }) {
  if (!acquireExecutionPermit(permit)) return undefined;
  const args = buildK7ExampleArgs(scenario, seed, naturalDays, behavior, event, c01);
  const startedAt = Date.now();
  const { code, stdout, stderr } = await executionPool.run(() => exec(preparedFixture.executable_path, args, {
    cwd: repoRoot,
    timeoutMs: deadline.childTimeoutMs(`${scenario} seed ${seed}`),
    signal: deadline.signal,
    env: {
      ...process.env,
      CARGO_TARGET_DIR: preparedFixture.cargo_target_dir,
      TMPDIR: preparedFixture.process_tmp_dir,
      TMP: preparedFixture.process_tmp_dir,
      TEMP: preparedFixture.process_tmp_dir,
      RAYON_NUM_THREADS: String(resourcePolicy.rayon_threads_per_seed),
      K7_SOURCE_FINGERPRINT_DIGEST: preparedFixture.embedded_source_fingerprint_digest,
    },
  }));
  deadline.assertRemaining(`${scenario} seed ${seed}`);
  if (code !== 0) throw new Error(`${scenario} seed ${seed} failed with exit ${code}: ${stderr.trim()}`);
  let parsed;
  try { parsed = JSON.parse(stdout); } catch (error) { throw new Error(`${scenario} seed ${seed} output is not JSON: ${error.message}`); }
  validateK7Output({ scenario, seed, naturalDays, behavior, event, c01, sourceFingerprintDigest: preparedFixture.embedded_source_fingerprint_digest, parsed });
  const buffer = Buffer.from(stdout, "utf8");
  const sha256 = createHash("sha256").update(buffer).digest("hex");
  if (write) {
    await publishBeforeDeadline(path.join(outputDir, `seed-${seed}.json`), buffer, atomicWrite, deadline);
  }
  return {
    seed,
    argv: [preparedFixture.executable_relative_path, ...args],
    exit_code: code,
    wall_ms: Date.now() - startedAt,
    sha256,
    raw: parsed,
  };
}

async function validateCompletedK7Run({ dir, entry, git, sourceFingerprint, preparedFixture, resourcePolicy, scenario, naturalDays, behavior, event, c01 }) {
  const identity = k7Identity({
    git,
    sourceFingerprint,
    preparedFixture,
    resourcePolicy,
    scenario,
    seeds: entry.ordered_seeds,
    naturalDays,
    behavior,
    event,
    c01,
  });
  const receipt = await readCheckpoint(path.join(dir, `seed-${entry.seed}.checkpoint.json`), identity);
  if (receipt === undefined || receipt.completed.length !== 1 || JSON.stringify(receipt.completed[0]) !== JSON.stringify(entry)) {
    throw new Error(`K7 checkpoint seed ${entry.seed} receipt does not exactly match its matrix checkpoint`);
  }
  const rawPath = path.join(dir, entry.file);
  let buffer;
  try {
    buffer = await fsp.readFile(rawPath);
  } catch (error) {
    throw new Error(`K7 checkpoint seed ${entry.seed} raw artifact cannot be read: ${error.message}`);
  }
  if (sha256(buffer) !== entry.sha256) throw new Error(`K7 checkpoint seed ${entry.seed} raw artifact digest mismatch`);
  let raw;
  try {
    raw = JSON.parse(buffer.toString("utf8"));
  } catch (error) {
    throw new Error(`K7 checkpoint seed ${entry.seed} raw artifact is malformed JSON: ${error.message}`);
  }
  validateK7Output({ scenario, seed: entry.seed, naturalDays, behavior, event, c01, sourceFingerprintDigest: sourceFingerprint.digest, parsed: raw });
  return { ...entry, raw };
}

async function captureK7Matrix({ outputDir, exec, repoRoot, deadline, git, sourceFingerprint, preparedFixture, resourcePolicy, executionPool, scenario, seeds, naturalDays, behavior, event, c01, permit, resume = false, atomicWrite = writeAtomically }) {
  validateSeedMatrix(seeds);
  const dir = path.join(outputDir, `${scenario}-b${behavior}-e${event}-c${c01}`);
  await fsp.mkdir(dir, { recursive: true });
  const identity = k7Identity({ git, sourceFingerprint, preparedFixture, resourcePolicy, scenario, seeds, naturalDays, behavior, event, c01 });
  const checkpointPath = path.join(dir, "checkpoint.json");
  let checkpoint = await readCheckpoint(checkpointPath, identity);
  if (checkpoint === undefined && resume) {
    const recovered = await recoverSeedReceipts(dir, identity);
    if (recovered.length > 0) {
      checkpoint = await persistCheckpoint(checkpointPath, identity, recovered, atomicWrite, deadline);
    } else {
      const entries = await fsp.readdir(dir);
      if (entries.length > 0) throw new Error(`K7 resume requires an exact-spec checkpoint for existing artifacts: ${checkpointPath}`);
    }
  }
  const completed = checkpoint?.completed ?? [];
  const validated = [];
  for (const entry of completed) validated.push(await validateCompletedK7Run({ dir, entry, git, sourceFingerprint, preparedFixture, resourcePolicy, scenario, naturalDays, behavior, event, c01 }));
  const completedSeeds = new Set(validated.map((entry) => entry.seed));
  let executed = 0;
  const pendingSeeds = seeds.filter((seed) => !completedSeeds.has(seed));
  if (pendingSeeds.length > 0) {
    const outcomes = await Promise.allSettled(pendingSeeds.map((seed) => captureK7Run({
      outputDir: dir,
      exec,
      repoRoot,
      deadline,
      preparedFixture,
      resourcePolicy,
      executionPool,
      scenario,
      seed,
      naturalDays,
      behavior,
      event,
      c01,
      write: true,
      permit,
      atomicWrite,
    })));
    let executionBudgetExhausted = false;
    for (const outcome of outcomes) {
      if (outcome.status === "rejected") continue;
      const run = outcome.value;
      if (run === undefined) {
        executionBudgetExhausted = true;
        continue;
      }
      const entry = { seed: run.seed, file: `seed-${run.seed}.json`, sha256: run.sha256, argv: run.argv, exit_code: run.exit_code, wall_ms: run.wall_ms, revision: git.revision, dirty_paths: git.dirty_paths, source_fingerprint_digest: sourceFingerprint.digest, ordered_seeds: [...seeds] };
      completed.push(entry);
      completed.sort((left, right) => seeds.indexOf(left.seed) - seeds.indexOf(right.seed));
      await persistSeedCheckpoint(dir, identity, entry, atomicWrite, deadline);
      await persistCheckpoint(checkpointPath, identity, completed, atomicWrite, deadline);
      validated.push({ ...entry, raw: run.raw });
      completedSeeds.add(run.seed);
      executed += 1;
    }
    const failure = outcomes.find((outcome) => outcome.status === "rejected");
    if (failure !== undefined) throw failure.reason;
    if (executionBudgetExhausted && permit.remaining !== 0) {
      throw new Error("K7 execution permit reported exhaustion with a non-zero remaining budget");
    }
  }
  const orderedRuns = validated.sort((left, right) => seeds.indexOf(left.seed) - seeds.indexOf(right.seed));
  const complete = sameSeedList(orderedRuns.map((entry) => entry.seed), seeds);
  if (complete && orderedRuns.length !== seeds.length) throw new Error("K7 checkpoint cannot finalize with duplicate or extra seed records");
  return { scenario, seeds: [...seeds], natural_days: naturalDays, multipliers: { behavior, event, c01_denominator_assumption: c01 }, source_fingerprint: sourceFingerprint, resource_policy: resourcePolicy, identity, runs: orderedRuns, complete, finalized: false, executed, checkpoint: path.relative(outputDir, checkpointPath), quantiles_and_extremes: complete ? summarizeK7Runs(orderedRuns) : undefined };
}

function validatePreparedK7Fixture(preparedFixture, resourcePolicy, sourceFingerprint, workspacePaths) {
  const workspaceRoot = workspacePaths.workspaceRoot;
  const executableRelative = typeof preparedFixture?.executable_relative_path === "string"
    ? preparedFixture.executable_relative_path.split("/").join(path.sep)
    : "";
  const expectedExecutable = path.join(workspaceRoot, executableRelative);
  if (typeof preparedFixture?.executable_path !== "string"
    || typeof preparedFixture.executable_relative_path !== "string"
    || !/^[a-f0-9]{64}$/.test(preparedFixture.binary_sha256 ?? "")
    || !Number.isSafeInteger(preparedFixture.binary_bytes)
    || preparedFixture.binary_bytes <= 0
    || preparedFixture.embedded_source_fingerprint_digest !== sourceFingerprint.digest
    || JSON.stringify(preparedFixture.build_argv) !== JSON.stringify(["cargo", ...buildK7FixtureBuildArgs()])
    || !Number.isInteger(preparedFixture.cargo_build_jobs)
    || preparedFixture.cargo_build_jobs <= 0
    || preparedFixture.cargo_build_jobs > resourcePolicy.available_cpu_count
    || preparedFixture.workspace_root !== workspaceRoot
    || preparedFixture.cargo_target_dir !== workspacePaths.cargoTargetDir
    || preparedFixture.process_tmp_dir !== workspacePaths.processTmpDir
    || path.resolve(preparedFixture.executable_path) !== expectedExecutable
    || path.relative(workspacePaths.cargoTargetDir, expectedExecutable).startsWith("..")
    || !Number.isFinite(preparedFixture.build_wall_ms)
    || preparedFixture.build_wall_ms < 0
    || preparedFixture.build_wall_ms > K7_BATCH_TIMEOUT_MS) {
    throw new Error("prepared K7 fixture must bind its executable path, build argv, byte length, and SHA-256 digest");
  }
}

function k7BuildRecord(preparedFixture) {
  return {
    argv: preparedFixture.build_argv,
    cargo_build_jobs: preparedFixture.cargo_build_jobs,
    wall_ms: preparedFixture.build_wall_ms,
    executable_relative_path: preparedFixture.executable_relative_path,
    workspace_root: preparedFixture.workspace_root,
    cargo_target_dir: preparedFixture.cargo_target_dir,
    process_tmp_dir: preparedFixture.process_tmp_dir,
    binary_sha256: preparedFixture.binary_sha256,
    binary_bytes: preparedFixture.binary_bytes,
  };
}

async function prepareK7Output({ outputDir, resume, exec, repoRoot, workspacePaths, prepareFixture, resourcePolicy, prepareTimeoutMs = K7_BATCH_TIMEOUT_MS }) {
  return withK7Deadline({
    childTimeoutMs: Math.min(K7_CHILD_TIMEOUT_MS, prepareTimeoutMs),
    batchTimeoutMs: prepareTimeoutMs,
  }, async (deadline) => {
  const resolvedWorkspacePaths = workspacePaths === undefined
    ? await prepareWorkspacePaths({ sourceRoot: repoRoot, scope: "k7" })
    : await prepareWorkspacePaths({ sourceRoot: repoRoot, scope: "k7", workspaceRoot: workspacePaths.workspaceRoot });
  outputDir = await validateWorkspaceOutputPath(resolvedWorkspacePaths.workspaceRoot, outputDir);
  if (resume) {
    try { await fsp.access(outputDir); } catch (error) { if (error?.code === "ENOENT") throw new Error(`K7 resume output directory does not exist: ${outputDir}`); throw error; }
  } else {
    await ensureFreshOutputDir(outputDir);
    await fsp.mkdir(outputDir, { recursive: true });
    await validateWorkspaceOutputPath(resolvedWorkspacePaths.workspaceRoot, outputDir);
  }
  const [git, sourceFingerprint] = await Promise.all([
    collectK7Git(exec, repoRoot, deadline),
    collectK7SourceFingerprint(exec, repoRoot, deadline),
  ]);
  const preparedFixture = await prepareFixture({ exec, repoRoot, workspacePaths: resolvedWorkspacePaths, deadline, resourcePolicy, sourceFingerprint });
  validatePreparedK7Fixture(preparedFixture, resourcePolicy, sourceFingerprint, resolvedWorkspacePaths);
  const sourceFingerprintAfterBuild = await collectK7SourceFingerprint(exec, repoRoot, deadline);
  if (sourceFingerprintAfterBuild.digest !== sourceFingerprint.digest) {
    throw new Error("K7 source fingerprint changed while preparing the fixture binary");
  }
  return { git, sourceFingerprint, preparedFixture };
  });
}

async function finalizeK7Matrix(matrix, { exec, repoRoot, deadline, preparedFixture, outputDir, permit, executionPool, atomicWrite = writeAtomically }) {
  if (!matrix.complete) return matrix;
  const rerunSeed = matrix.seeds.at(-1);
  const canonical = matrix.runs.find((run) => run.seed === rerunSeed);
  const receiptPath = path.join(outputDir, path.dirname(matrix.checkpoint), "determinism.checkpoint.json");
  const receipt = await readDeterminismReceipt(receiptPath, matrix.identity, canonical);
  if (receipt !== undefined) {
    return { ...matrix, finalized: true, determinism_check: { seed: receipt.seed, first_digest: receipt.first_digest, rerun_digest: receipt.rerun_digest, identical: true, revision: receipt.revision, receipt: path.basename(receiptPath) } };
  }
  const rerun = await captureK7Run({ outputDir, exec, repoRoot, deadline, preparedFixture, resourcePolicy: matrix.resource_policy, executionPool, scenario: matrix.scenario, seed: rerunSeed, naturalDays: matrix.natural_days, behavior: matrix.multipliers.behavior, event: matrix.multipliers.event, c01: matrix.multipliers.c01_denominator_assumption, write: false, permit });
  if (rerun === undefined) return { ...matrix, finalized: false, finalization: { complete: false, reason: "execution_budget_exhausted" } };
  if (rerun.sha256 !== canonical.sha256) throw new Error(`K7 deterministic rerun differs for ${matrix.scenario} seed ${rerunSeed}: ${canonical.sha256} != ${rerun.sha256}`);
  const persisted = await persistDeterminismReceipt(receiptPath, matrix.identity, canonical, rerun, atomicWrite, deadline);
  return { ...matrix, finalized: true, determinism_check: { seed: rerunSeed, first_digest: canonical.sha256, rerun_digest: rerun.sha256, identical: true, revision: persisted.revision, receipt: path.basename(receiptPath) } };
}

async function finalizeSensitivityReports(dimensions, options) {
  const uniqueReports = new Map();
  for (const dimension of dimensions) {
    const key = `${dimension.report.multipliers.behavior}/${dimension.report.multipliers.event}/${dimension.report.multipliers.c01_denominator_assumption}`;
    if (!uniqueReports.has(key)) uniqueReports.set(key, dimension.report);
  }
  const entries = [...uniqueReports.entries()];
  const finalized = new Map();
  for (let offset = 0; offset < entries.length; offset += 3) {
    const group = entries.slice(offset, offset + 3);
    const reports = await settleAllOrThrow(group.map(([, report]) => finalizeK7Matrix(report, options)));
    for (let index = 0; index < group.length; index += 1) finalized.set(group[index][0], reports[index]);
  }
  return dimensions.map((dimension) => {
    const key = `${dimension.report.multipliers.behavior}/${dimension.report.multipliers.event}/${dimension.report.multipliers.c01_denominator_assumption}`;
    return { ...dimension, report: finalized.get(key) };
  });
}

export async function captureAfter({ outputDir, exec = realExec, repoRoot = REPO_ROOT, workspacePaths, timeoutMs = K7_CHILD_TIMEOUT_MS, batchTimeoutMs = K7_BATCH_TIMEOUT_MS, prepareTimeoutMs = K7_BATCH_TIMEOUT_MS, seeds = MATRIX_SEEDS, reportSource = "fresh_current_k7_setup", primaryNaturalDays = K7_PRIMARY_NATURAL_DAYS, crossYearNaturalDays = K7_CROSS_YEAR_NATURAL_DAYS, batchSize = Number.POSITIVE_INFINITY, resume = false, atomicWrite = writeAtomically, prepareFixture = prepareK7FixtureExecutable, resourcePolicy, maximumThreadCount = "auto" }) {
  if (reportSource !== "fresh_current_k7_setup") throw new Error("after report source must be fresh_current_k7_setup");
  if (!sameSeedList(seeds, MATRIX_SEEDS)) throw new Error("after seed list must exactly match Task 1 seed matrix");
  resourcePolicy ??= await detectK7ResourcePolicy({ maximumThreadCount });
  validateK7ResourcePolicy(resourcePolicy);
  const { git, sourceFingerprint, preparedFixture } = await prepareK7Output({ outputDir, resume, exec, repoRoot, workspacePaths, prepareFixture, resourcePolicy, prepareTimeoutMs });
  return withK7Deadline({ childTimeoutMs: timeoutMs, batchTimeoutMs }, async (deadline) => {
  const permit = createExecutionPermit(batchSize);
  const executionPool = createK7ExecutionPool(resourcePolicy.max_concurrent_child_executions);
  const capturePrimary = () => captureK7Matrix({ outputDir, exec, repoRoot, deadline, git, sourceFingerprint, preparedFixture, resourcePolicy, executionPool, scenario: "primary", seeds, naturalDays: primaryNaturalDays, behavior: 1, event: 1, c01: 1, permit, resume, atomicWrite });
  const captureCrossYear = () => captureK7Matrix({ outputDir, exec, repoRoot, deadline, git, sourceFingerprint, preparedFixture, resourcePolicy, executionPool, scenario: "cross-year", seeds: CROSS_YEAR_SEEDS, naturalDays: crossYearNaturalDays, behavior: 1, event: 1, c01: 1, permit, resume, atomicWrite });
  let primary;
  let crossYear;
  if (batchSize === Number.POSITIVE_INFINITY) {
    [primary, crossYear] = await settleAllOrThrow([capturePrimary(), captureCrossYear()]);
  } else {
    primary = await capturePrimary();
    crossYear = await captureCrossYear();
  }
  if (!primary.complete || !crossYear.complete) return { command: "after", incomplete: true, primary, cross_year_four_industry: crossYear };
  const finalizePrimary = () => finalizeK7Matrix(primary, { exec, repoRoot, deadline, preparedFixture, outputDir, permit, executionPool, atomicWrite });
  const finalizeCrossYear = () => finalizeK7Matrix(crossYear, { exec, repoRoot, deadline, preparedFixture, outputDir, permit, executionPool, atomicWrite });
  if (batchSize === Number.POSITIVE_INFINITY) {
    [primary, crossYear] = await settleAllOrThrow([finalizePrimary(), finalizeCrossYear()]);
  } else {
    primary = await finalizePrimary();
    crossYear = await finalizeCrossYear();
  }
  if (!primary.finalized || !crossYear.finalized) return { command: "after", incomplete: true, primary, cross_year_four_industry: crossYear };
  deadline.assertRemaining("after manifest publication");
  const manifest = { command: "after", source: reportSource, git, source_fingerprint: sourceFingerprint, fixture_binary: primary.identity.fixture, fixture_build: k7BuildRecord(preparedFixture), resource_policy: resourcePolicy, primary, cross_year_four_industry: crossYear, c06_external_market_calibration: "not_completed_no_authorized_data" };
  await publishBeforeDeadline(path.join(outputDir, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`, atomicWrite, deadline);
  return manifest;
  });
}

export async function captureSensitivity({ outputDir, exec = realExec, repoRoot = REPO_ROOT, workspacePaths, timeoutMs = K7_CHILD_TIMEOUT_MS, batchTimeoutMs = K7_BATCH_TIMEOUT_MS, prepareTimeoutMs = K7_BATCH_TIMEOUT_MS, behaviorMultipliers = SENSITIVITY_MULTIPLIERS, eventMultipliers = SENSITIVITY_MULTIPLIERS, c01Multipliers = SENSITIVITY_MULTIPLIERS, naturalDays = K7_PRIMARY_NATURAL_DAYS, batchSize = Number.POSITIVE_INFINITY, resume = false, atomicWrite = writeAtomically, prepareFixture = prepareK7FixtureExecutable, resourcePolicy, maximumThreadCount = "auto" }) {
  requireSensitivityMatrix(behaviorMultipliers, "behavior");
  requireSensitivityMatrix(eventMultipliers, "event");
  requireSensitivityMatrix(c01Multipliers, "C01 denominator");
  resourcePolicy ??= await detectK7ResourcePolicy({ maximumThreadCount });
  validateK7ResourcePolicy(resourcePolicy);
  const { git, sourceFingerprint, preparedFixture } = await prepareK7Output({ outputDir, resume, exec, repoRoot, workspacePaths, prepareFixture, resourcePolicy, prepareTimeoutMs });
  return withK7Deadline({ childTimeoutMs: timeoutMs, batchTimeoutMs }, async (deadline) => {
  const requests = [
    ...behaviorMultipliers.map((multiplier) => ({ dimension: "behavior", multiplier, behavior: multiplier, event: 1, c01: 1 })),
    ...eventMultipliers.map((multiplier) => ({ dimension: "event", multiplier, behavior: 1, event: multiplier, c01: 1 })),
    ...c01Multipliers.map((multiplier) => ({ dimension: "c01_volume_denominator_assumption", multiplier, behavior: 1, event: 1, c01: multiplier })),
  ];
  const permit = createExecutionPermit(batchSize);
  const executionPool = createK7ExecutionPool(resourcePolicy.max_concurrent_child_executions);
  const uniqueRequests = new Map();
  for (const request of requests) {
    const key = `${request.behavior}/${request.event}/${request.c01}`;
    if (!uniqueRequests.has(key)) uniqueRequests.set(key, request);
  }
  const uniqueEntries = [...uniqueRequests.entries()];
  const canonical = new Map();
  for (let offset = 0; offset < uniqueEntries.length; offset += 3) {
    const group = uniqueEntries.slice(offset, offset + 3);
    const reports = await settleAllOrThrow(group.map(([, request]) => captureK7Matrix({ outputDir, exec, repoRoot, deadline, git, sourceFingerprint, preparedFixture, resourcePolicy, executionPool, scenario: "primary", seeds: MATRIX_SEEDS, naturalDays, behavior: request.behavior, event: request.event, c01: request.c01, permit, resume, atomicWrite })));
    for (let index = 0; index < group.length; index += 1) canonical.set(group[index][0], reports[index]);
  }
  const seen = new Set();
  const dimensions = requests.map((request) => {
    const key = `${request.behavior}/${request.event}/${request.c01}`;
    const reused = seen.has(key);
    seen.add(key);
    return { dimension: request.dimension, multiplier: request.multiplier, reuse: reused ? { canonical_spec: key, validated: true } : { executed_or_resumed: true }, report: canonical.get(key) };
  });
  if (dimensions.some((dimension) => !dimension.report.complete)) return { command: "sensitivity", incomplete: true, dimensions };
  const finalizedDimensions = await finalizeSensitivityReports(dimensions, { exec, repoRoot, deadline, preparedFixture, outputDir, permit, executionPool, atomicWrite });
  if (finalizedDimensions.some((dimension) => !dimension.report.finalized)) return { command: "sensitivity", incomplete: true, dimensions: finalizedDimensions };
  deadline.assertRemaining("sensitivity manifest publication");
  const manifest = { command: "sensitivity", source: "fresh_current_k7_setup", git, source_fingerprint: sourceFingerprint, fixture_binary: finalizedDimensions[0].report.identity.fixture, fixture_build: k7BuildRecord(preparedFixture), resource_policy: resourcePolicy, dimensions: finalizedDimensions, c06_external_market_calibration: "not_completed_no_authorized_data" };
  await publishBeforeDeadline(path.join(outputDir, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`, atomicWrite, deadline);
  return manifest;
  });
}

export async function main(argv) {
  const { command, outputDir, scenarios, resume, batchSize, maximumThreadCount } = parseCliArgs(argv);
  if (command === "after") { await captureAfter({ outputDir, resume, batchSize, maximumThreadCount }); return; }
  if (command === "sensitivity") { await captureSensitivity({ outputDir, resume, batchSize, maximumThreadCount }); return; }
  throw new Error(`before 是已密封历史采集工具，不属于当前验收且禁止重跑；请只读现有语料：${outputDir}（解析场景：${scenarios.join("、")}）`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(`基线采集失败：${error.message}`);
    process.exitCode = 1;
  });
}
