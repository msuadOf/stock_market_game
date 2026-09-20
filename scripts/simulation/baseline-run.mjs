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
 *   node scripts/simulation/baseline-run.mjs before --output <目录> [--scenario matrix|compressed-300|all]
 */
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import fsp from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

/** 独立 seed 矩阵；重复 seed 会压窄跨 seed 区间，必须在采集前拒绝。 */
export const MATRIX_SEEDS = [1, 2, 3, 4, 5, 7, 11, 19, 23, 31];

export const TRADING_DAYS = 30;
export const CROSS_YEAR_SEEDS = [1, 7, 11, 19, 31];
export const SENSITIVITY_MULTIPLIERS = [0.5, 1, 2];

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

const DEFAULT_TIMEOUT_MS = Number.parseInt(process.env.BASELINE_FIXTURE_TIMEOUT_MS ?? "", 10) || 7_200_000;
const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const K7_CHECKPOINT_SCHEMA = "k7-baseline-checkpoint-v2";
const K7_RUNNER_VERSION = "2026-09-14-content-addressed-v2";
const K7_SOURCE_FINGERPRINT_ALGORITHM = "k7-simulation-source-v1";
const K7_DETERMINISM_RECEIPT_SCHEMA = "k7-determinism-receipt-v1";
const K7_RESOURCE_POLICY_SCHEMA = "k7-resource-policy-v3";
const K7_SOURCE_INPUTS = [
  ".cargo",
  "Cargo.lock",
  "Cargo.toml",
  "packages/engine",
  "rust-toolchain.toml",
  "scripts/simulation/baseline-run.mjs",
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
  return ["run", "-p", "engine", "--release", "--features", "simulation-diagnostics", "--example", "k7_baseline_fixture", "--", scenario, String(seed), String(days), String(behaviorMultiplier), String(eventMultiplier), String(c01Multiplier)];
}

export function buildK7ResourcePolicy(availableCpuCount, availableCpuSource, maximumThreadCount = "auto") {
  if (!Number.isInteger(availableCpuCount) || availableCpuCount <= 0) {
    throw new Error("K7 resource policy requires a positive process-available CPU count");
  }
  if (maximumThreadCount !== "auto" && (!Number.isInteger(maximumThreadCount) || maximumThreadCount <= 0)) {
    throw new Error("K7 maximum thread count must be a positive integer or auto");
  }
  return {
    schema: K7_RESOURCE_POLICY_SCHEMA,
    available_cpu_count: availableCpuCount,
    available_cpu_source: availableCpuSource,
    maximum_thread_count: maximumThreadCount,
    max_concurrent_seed_processes: 1,
    rayon_threads_per_seed: maximumThreadCount === "auto"
      ? availableCpuCount
      : Math.min(availableCpuCount, maximumThreadCount),
  };
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
  if (resourcePolicy?.schema !== K7_RESOURCE_POLICY_SCHEMA
    || !Number.isInteger(resourcePolicy.available_cpu_count)
    || resourcePolicy.available_cpu_count <= 0
    || typeof resourcePolicy.available_cpu_source !== "string"
    || resourcePolicy.available_cpu_source.length === 0
    || (resourcePolicy.maximum_thread_count !== "auto"
      && (!Number.isInteger(resourcePolicy.maximum_thread_count) || resourcePolicy.maximum_thread_count <= 0))
    || !Number.isInteger(resourcePolicy.max_concurrent_seed_processes)
    || resourcePolicy.max_concurrent_seed_processes !== 1
    || !Number.isInteger(resourcePolicy.rayon_threads_per_seed)
    || resourcePolicy.rayon_threads_per_seed !== (resourcePolicy.maximum_thread_count === "auto"
      ? resourcePolicy.available_cpu_count
      : Math.min(resourcePolicy.available_cpu_count, resourcePolicy.maximum_thread_count))) {
    throw new Error("K7 resource policy must run seeds serially within its detected CPU budget and explicit maximum");
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
  const { scenario, seed, naturalDays, behavior, event, c01, parsed } = expected;
  if (parsed.source !== "fresh_current_k7_setup") throw new Error("after report source must be fresh_current_k7_setup; save-backed provenance is rejected");
  if (Object.hasOwn(parsed, "save_path") || Object.hasOwn(parsed, "load")) throw new Error("after report must not contain save provenance fields");
  if (parsed.scenario !== scenario || parsed.seed !== String(seed) || parsed.natural_days !== naturalDays) throw new Error("K7 report scenario, seed, or natural-day request mismatch");
  if (parsed.multipliers?.behavior !== behavior || parsed.multipliers?.event !== event || parsed.multipliers?.c01_denominator_assumption !== c01) throw new Error("K7 report multiplier echo mismatch");
  if (parsed.calendar?.natural_days !== naturalDays || typeof parsed.calendar?.trading_days !== "number" || typeof parsed.calendar?.closed_days !== "number") throw new Error("K7 report lacks natural-day calendar accounting");
  if (parsed.calendar.trading_days + parsed.calendar.closed_days !== naturalDays) throw new Error("K7 calendar accounting does not cover every natural day");
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

/** 真实子进程执行；超时或无法启动都显式抛错，绝不返回伪造的成功结果。 */
function realExec(file, args, { cwd, timeoutMs, env }) {
  return new Promise((resolve, reject) => {
    const child = spawn(file, args, { cwd, env, windowsHide: true });
    let stdout = "";
    let stderr = "";
    let timedOut = false;
    const timer = setTimeout(() => {
      timedOut = true;
      child.kill();
    }, timeoutMs);
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("error", (error) => {
      clearTimeout(timer);
      reject(new Error(`无法启动 ${file} ${args.join(" ")}：${error.message}`));
    });
    child.on("close", (code) => {
      clearTimeout(timer);
      if (timedOut || code === null) {
        reject(new Error(`${file} ${args.join(" ")} 超过 ${timeoutMs}ms 被强制终止`));
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

async function mustSucceed(exec, repoRoot, file, args) {
  const { code, stdout, stderr } = await exec(file, args, { cwd: repoRoot, timeoutMs: 60_000 });
  if (code !== 0) {
    throw new Error(`采集 ${file} ${args.join(" ")} 失败（退出码 ${code}）：${stderr.trim()}`);
  }
  return stdout;
}

async function collectGit(exec, repoRoot) {
  const revision = (await mustSucceed(exec, repoRoot, "git", ["rev-parse", "HEAD"])).trim();
  const branch = (await mustSucceed(exec, repoRoot, "git", ["branch", "--show-current"])).trim();
  const porcelain = await mustSucceed(exec, repoRoot, "git", ["status", "--porcelain"]);
  const dirtyPaths = porcelain.split(/\r?\n/).filter((line) => line.length > 0);
  return { revision, branch, dirty_paths: dirtyPaths };
}

async function collectK7Git(exec, repoRoot) {
  const git = await collectGit(exec, repoRoot);
  const dirtyPaths = (await mustSucceed(exec, repoRoot, "git", ["status", "--porcelain", "--", ...K7_SOURCE_INPUTS]))
    .split(/\r?\n/)
    .filter((line) => line.length > 0);
  return { ...git, dirty_paths: dirtyPaths };
}

async function collectK7SourceFingerprint(exec, repoRoot) {
  const sourceArguments = ["--", ...K7_SOURCE_INPUTS];
  const committedTree = (await mustSucceed(exec, repoRoot, "git", ["rev-parse", "HEAD^{tree}"])).trim();
  const dirtyPatch = await mustSucceed(exec, repoRoot, "git", ["diff", "--binary", "--no-ext-diff", "HEAD", ...sourceArguments]);
  const files = [];
  async function collectFiles(relativePath) {
    let entry;
    try {
      entry = await fsp.lstat(path.join(repoRoot, relativePath));
    } catch (error) {
      if (error?.code === "ENOENT") return;
      throw error;
    }
    if (entry.isFile()) {
      files.push({ path: relativePath, sha256: sha256(await fsp.readFile(path.join(repoRoot, relativePath))) });
      return;
    }
    if (!entry.isDirectory()) throw new Error(`K7 source input has unsupported filesystem entry: ${relativePath}`);
    const entries = await fsp.readdir(path.join(repoRoot, relativePath), { withFileTypes: true });
    for (const entry of entries.sort((left, right) => left.name.localeCompare(right.name))) {
      const childPath = path.posix.join(relativePath, entry.name);
      if (entry.isDirectory()) {
        await collectFiles(childPath);
      } else if (entry.isFile()) {
        files.push({ path: childPath, sha256: sha256(await fsp.readFile(path.join(repoRoot, childPath))) });
      } else {
        throw new Error(`K7 source input has unsupported filesystem entry: ${childPath}`);
      }
    }
  }
  for (const sourceInput of K7_SOURCE_INPUTS) await collectFiles(sourceInput);
  const state = {
    algorithm: K7_SOURCE_FINGERPRINT_ALGORITHM,
    committed_tree: committedTree,
    dirty_patch_sha256: sha256(dirtyPatch),
    files,
  };
  return { ...state, digest: sha256(JSON.stringify(state)) };
}

/** 工具链版本逐项采集；某项不可用时显式记录 available:false 与原因，绝不静默省略。 */
async function collectToolchain(exec, repoRoot) {
  const toolchain = {};
  const blocked = [];
  for (const [name, args] of [
    ["node", ["--version"]],
    ["cargo", ["--version"]],
  ]) {
    toolchain[name] = {
      available: true,
      version: (await mustSucceed(exec, repoRoot, name, args)).trim(),
    };
  }
  try {
    toolchain.pnpm = {
      available: true,
      version: (await mustSucceed(exec, repoRoot, "corepack", ["pnpm", "--version"])).trim(),
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

async function runOneFixture({ scenarioName, seed, dir, exec, repoRoot, timeoutMs, write }) {
  const args = buildExampleArgs(scenarioName, seed, TRADING_DAYS);
  const startedAt = Date.now();
  const { code, stdout, stderr } = await exec("cargo", args, { cwd: repoRoot, timeoutMs });
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

async function captureScenario({ scenarioName, outputDir, exec, repoRoot, timeoutMs, log }) {
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
      timeoutMs,
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
    timeoutMs,
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
  timeoutMs = DEFAULT_TIMEOUT_MS,
  command = "before",
  log = console.log,
}) {
  validateSeedMatrix(MATRIX_SEEDS);
  await ensureFreshOutputDir(outputDir);
  await fsp.mkdir(outputDir, { recursive: true });

  const git = await collectGit(exec, repoRoot);
  const { toolchain, blocked } = await collectToolchain(exec, repoRoot);
  for (const item of blocked) {
    log(`显式记录（不静默省略）：${item}`);
  }

  const manifest = {
    command,
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
      log,
    });
  }

  const manifestPath = path.join(outputDir, "manifest.json");
  await fsp.writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
  log(`manifest 已写入：${manifestPath}`);
  return manifest;
}

async function writeAtomically(filePath, content) {
  const temporaryPath = `${filePath}.${process.pid}.${Date.now()}.tmp`;
  await fsp.writeFile(temporaryPath, content);
  await fsp.rename(temporaryPath, filePath);
}

function sha256(content) {
  return createHash("sha256").update(content).digest("hex");
}

function k7Identity({ git, sourceFingerprint, resourcePolicy, scenario, seeds, naturalDays, behavior, event, c01 }) {
  return {
    runner: { schema: K7_CHECKPOINT_SCHEMA, version: K7_RUNNER_VERSION, script: "scripts/simulation/baseline-run.mjs" },
    git: { revision: git.revision, dirty_paths: git.dirty_paths },
    source_fingerprint: sourceFingerprint,
    resource_policy: resourcePolicy,
    fixture: { source: "fresh_current_k7_setup", argv: buildK7ExampleArgs(scenario, "<seed>", naturalDays, behavior, event, c01) },
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
    if (JSON.stringify(entry.argv) !== JSON.stringify(["cargo", ...identity.fixture.argv.map((value) => value === "<seed>" ? String(entry.seed) : value)])) throw new Error(`K7 checkpoint seed ${entry.seed} argv mismatch`);
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

async function persistCheckpoint(checkpointPath, identity, completed, atomicWrite) {
  const checkpoint = { schema: K7_CHECKPOINT_SCHEMA, identity, identity_digest: sha256(JSON.stringify(identity)), completed };
  checkpoint.checkpoint_digest = checkpointDigest(checkpoint);
  await atomicWrite(checkpointPath, `${JSON.stringify(checkpoint, null, 2)}\n`);
  return checkpoint;
}

async function persistSeedCheckpoint(dir, identity, entry, atomicWrite) {
  const checkpointPath = path.join(dir, `seed-${entry.seed}.checkpoint.json`);
  return persistCheckpoint(checkpointPath, identity, [entry], atomicWrite);
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

async function persistDeterminismReceipt(receiptPath, identity, canonical, rerun, atomicWrite) {
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
  await atomicWrite(receiptPath, `${JSON.stringify(receipt, null, 2)}\n`);
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

async function captureK7Run({ outputDir, exec, repoRoot, timeoutMs, resourcePolicy, scenario, seed, naturalDays, behavior, event, c01, write, permit, atomicWrite = writeAtomically }) {
  if (!acquireExecutionPermit(permit)) return undefined;
  const args = buildK7ExampleArgs(scenario, seed, naturalDays, behavior, event, c01);
  const startedAt = Date.now();
  const { code, stdout, stderr } = await exec("cargo", args, {
    cwd: repoRoot,
    timeoutMs,
    env: { ...process.env, RAYON_NUM_THREADS: String(resourcePolicy.rayon_threads_per_seed) },
  });
  if (code !== 0) throw new Error(`${scenario} seed ${seed} failed with exit ${code}: ${stderr.trim()}`);
  let parsed;
  try { parsed = JSON.parse(stdout); } catch (error) { throw new Error(`${scenario} seed ${seed} output is not JSON: ${error.message}`); }
  validateK7Output({ scenario, seed, naturalDays, behavior, event, c01, parsed });
  const buffer = Buffer.from(stdout, "utf8");
  const sha256 = createHash("sha256").update(buffer).digest("hex");
  if (write) await atomicWrite(path.join(outputDir, `seed-${seed}.json`), buffer);
  return { seed, argv: ["cargo", ...args], exit_code: code, wall_ms: Date.now() - startedAt, sha256, raw: parsed };
}

async function validateCompletedK7Run({ dir, entry, git, sourceFingerprint, resourcePolicy, scenario, naturalDays, behavior, event, c01 }) {
  const identity = k7Identity({
    git,
    sourceFingerprint,
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
  validateK7Output({ scenario, seed: entry.seed, naturalDays, behavior, event, c01, parsed: raw });
  return { ...entry, raw };
}

async function captureK7Matrix({ outputDir, exec, repoRoot, timeoutMs, git, sourceFingerprint, resourcePolicy, scenario, seeds, naturalDays, behavior, event, c01, permit, resume = false, atomicWrite = writeAtomically }) {
  validateSeedMatrix(seeds);
  const dir = path.join(outputDir, `${scenario}-b${behavior}-e${event}-c${c01}`);
  await fsp.mkdir(dir, { recursive: true });
  const identity = k7Identity({ git, sourceFingerprint, resourcePolicy, scenario, seeds, naturalDays, behavior, event, c01 });
  const checkpointPath = path.join(dir, "checkpoint.json");
  let checkpoint = await readCheckpoint(checkpointPath, identity);
  if (checkpoint === undefined && resume) {
    const recovered = await recoverSeedReceipts(dir, identity);
    if (recovered.length > 0) {
      checkpoint = await persistCheckpoint(checkpointPath, identity, recovered, atomicWrite);
    } else {
      const entries = await fsp.readdir(dir);
      if (entries.length > 0) throw new Error(`K7 resume requires an exact-spec checkpoint for existing artifacts: ${checkpointPath}`);
    }
  }
  const completed = checkpoint?.completed ?? [];
  const validated = [];
  for (const entry of completed) validated.push(await validateCompletedK7Run({ dir, entry, git, sourceFingerprint, resourcePolicy, scenario, naturalDays, behavior, event, c01 }));
  const completedSeeds = new Set(validated.map((entry) => entry.seed));
  let executed = 0;
  const pendingSeeds = seeds.filter((seed) => !completedSeeds.has(seed));
  for (const seed of pendingSeeds) {
    const run = await captureK7Run({ outputDir: dir, exec, repoRoot, timeoutMs, resourcePolicy, scenario, seed, naturalDays, behavior, event, c01, write: true, permit, atomicWrite });
    if (run === undefined) break;
    const entry = { seed: run.seed, file: `seed-${run.seed}.json`, sha256: run.sha256, argv: run.argv, exit_code: run.exit_code, wall_ms: run.wall_ms, revision: git.revision, dirty_paths: git.dirty_paths, source_fingerprint_digest: sourceFingerprint.digest, ordered_seeds: [...seeds] };
    completed.push(entry);
    await persistSeedCheckpoint(dir, identity, entry, atomicWrite);
    await persistCheckpoint(checkpointPath, identity, completed, atomicWrite);
    validated.push({ ...entry, raw: run.raw });
    completedSeeds.add(seed);
    executed += 1;
  }
  const orderedRuns = validated.sort((left, right) => seeds.indexOf(left.seed) - seeds.indexOf(right.seed));
  const complete = sameSeedList(orderedRuns.map((entry) => entry.seed), seeds);
  if (complete && orderedRuns.length !== seeds.length) throw new Error("K7 checkpoint cannot finalize with duplicate or extra seed records");
  return { scenario, seeds: [...seeds], natural_days: naturalDays, multipliers: { behavior, event, c01_denominator_assumption: c01 }, source_fingerprint: sourceFingerprint, resource_policy: resourcePolicy, identity, runs: orderedRuns, complete, finalized: false, executed, checkpoint: path.relative(outputDir, checkpointPath), quantiles_and_extremes: complete ? summarizeK7Runs(orderedRuns) : undefined };
}

async function prepareK7Output({ outputDir, resume, exec, repoRoot }) {
  if (resume) {
    try { await fsp.access(outputDir); } catch (error) { if (error?.code === "ENOENT") throw new Error(`K7 resume output directory does not exist: ${outputDir}`); throw error; }
  } else {
    await ensureFreshOutputDir(outputDir);
    await fsp.mkdir(outputDir, { recursive: true });
  }
  const [git, sourceFingerprint] = await Promise.all([
    collectK7Git(exec, repoRoot),
    collectK7SourceFingerprint(exec, repoRoot),
  ]);
  return { git, sourceFingerprint };
}

async function finalizeK7Matrix(matrix, { exec, repoRoot, timeoutMs, outputDir, permit, atomicWrite = writeAtomically }) {
  if (!matrix.complete) return matrix;
  const rerunSeed = matrix.seeds.at(-1);
  const canonical = matrix.runs.find((run) => run.seed === rerunSeed);
  const receiptPath = path.join(outputDir, path.dirname(matrix.checkpoint), "determinism.checkpoint.json");
  const receipt = await readDeterminismReceipt(receiptPath, matrix.identity, canonical);
  if (receipt !== undefined) {
    return { ...matrix, finalized: true, determinism_check: { seed: receipt.seed, first_digest: receipt.first_digest, rerun_digest: receipt.rerun_digest, identical: true, revision: receipt.revision, receipt: path.basename(receiptPath) } };
  }
  const rerun = await captureK7Run({ outputDir, exec, repoRoot, timeoutMs, resourcePolicy: matrix.resource_policy, scenario: matrix.scenario, seed: rerunSeed, naturalDays: matrix.natural_days, behavior: matrix.multipliers.behavior, event: matrix.multipliers.event, c01: matrix.multipliers.c01_denominator_assumption, write: false, permit });
  if (rerun === undefined) return { ...matrix, finalized: false, finalization: { complete: false, reason: "execution_budget_exhausted" } };
  if (rerun.sha256 !== canonical.sha256) throw new Error(`K7 deterministic rerun differs for ${matrix.scenario} seed ${rerunSeed}: ${canonical.sha256} != ${rerun.sha256}`);
  const persisted = await persistDeterminismReceipt(receiptPath, matrix.identity, canonical, rerun, atomicWrite);
  return { ...matrix, finalized: true, determinism_check: { seed: rerunSeed, first_digest: canonical.sha256, rerun_digest: rerun.sha256, identical: true, revision: persisted.revision, receipt: path.basename(receiptPath) } };
}

async function finalizeSensitivityReports(dimensions, options) {
  const finalized = new Map();
  const reports = [];
  for (const dimension of dimensions) {
    const key = `${dimension.report.multipliers.behavior}/${dimension.report.multipliers.event}/${dimension.report.multipliers.c01_denominator_assumption}`;
    let report = finalized.get(key);
    if (report === undefined) {
      report = await finalizeK7Matrix(dimension.report, options);
      finalized.set(key, report);
    }
    reports.push({ ...dimension, report });
  }
  return reports;
}

export async function captureAfter({ outputDir, exec = realExec, repoRoot = REPO_ROOT, timeoutMs = DEFAULT_TIMEOUT_MS, seeds = MATRIX_SEEDS, reportSource = "fresh_current_k7_setup", primaryNaturalDays = TRADING_DAYS, crossYearNaturalDays = 400, batchSize = Number.POSITIVE_INFINITY, resume = false, atomicWrite = writeAtomically, resourcePolicy, maximumThreadCount = "auto" }) {
  if (reportSource !== "fresh_current_k7_setup") throw new Error("after report source must be fresh_current_k7_setup");
  if (!sameSeedList(seeds, MATRIX_SEEDS)) throw new Error("after seed list must exactly match Task 1 seed matrix");
  resourcePolicy ??= await detectK7ResourcePolicy({ maximumThreadCount });
  validateK7ResourcePolicy(resourcePolicy);
  const { git, sourceFingerprint } = await prepareK7Output({ outputDir, resume, exec, repoRoot });
  const permit = createExecutionPermit(batchSize);
  let primary = await captureK7Matrix({ outputDir, exec, repoRoot, timeoutMs, git, sourceFingerprint, resourcePolicy, scenario: "primary", seeds, naturalDays: primaryNaturalDays, behavior: 1, event: 1, c01: 1, permit, resume, atomicWrite });
  if (!primary.complete) return { command: "after", incomplete: true, primary };
  primary = await finalizeK7Matrix(primary, { exec, repoRoot, timeoutMs, outputDir, permit, atomicWrite });
  if (!primary.finalized) return { command: "after", incomplete: true, primary };
  const crossYear = await captureK7Matrix({ outputDir, exec, repoRoot, timeoutMs, git, sourceFingerprint, resourcePolicy, scenario: "cross-year", seeds: CROSS_YEAR_SEEDS, naturalDays: crossYearNaturalDays, behavior: 1, event: 1, c01: 1, permit, resume, atomicWrite });
  if (!crossYear.complete) return { command: "after", incomplete: true, primary, cross_year_four_industry: crossYear };
  const completedCrossYear = await finalizeK7Matrix(crossYear, { exec, repoRoot, timeoutMs, outputDir, permit, atomicWrite });
  if (!completedCrossYear.finalized) return { command: "after", incomplete: true, primary, cross_year_four_industry: completedCrossYear };
  const manifest = { command: "after", source: reportSource, git, source_fingerprint: sourceFingerprint, resource_policy: resourcePolicy, primary, cross_year_four_industry: completedCrossYear, c06_external_market_calibration: "not_completed_no_authorized_data" };
  await atomicWrite(path.join(outputDir, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  return manifest;
}

export async function captureSensitivity({ outputDir, exec = realExec, repoRoot = REPO_ROOT, timeoutMs = DEFAULT_TIMEOUT_MS, behaviorMultipliers = SENSITIVITY_MULTIPLIERS, eventMultipliers = SENSITIVITY_MULTIPLIERS, c01Multipliers = SENSITIVITY_MULTIPLIERS, naturalDays = TRADING_DAYS, batchSize = Number.POSITIVE_INFINITY, resume = false, atomicWrite = writeAtomically, resourcePolicy, maximumThreadCount = "auto" }) {
  requireSensitivityMatrix(behaviorMultipliers, "behavior");
  requireSensitivityMatrix(eventMultipliers, "event");
  requireSensitivityMatrix(c01Multipliers, "C01 denominator");
  resourcePolicy ??= await detectK7ResourcePolicy({ maximumThreadCount });
  validateK7ResourcePolicy(resourcePolicy);
  const { git, sourceFingerprint } = await prepareK7Output({ outputDir, resume, exec, repoRoot });
  const dimensions = [];
  const requests = [
    ...behaviorMultipliers.map((multiplier) => ({ dimension: "behavior", multiplier, behavior: multiplier, event: 1, c01: 1 })),
    ...eventMultipliers.map((multiplier) => ({ dimension: "event", multiplier, behavior: 1, event: multiplier, c01: 1 })),
    ...c01Multipliers.map((multiplier) => ({ dimension: "c01_volume_denominator_assumption", multiplier, behavior: 1, event: 1, c01: multiplier })),
  ];
  const canonical = new Map();
  const permit = createExecutionPermit(batchSize);
  for (const request of requests) {
    const key = `${request.behavior}/${request.event}/${request.c01}`;
    let report = canonical.get(key);
    const reused = report !== undefined;
    if (!report) {
      report = await captureK7Matrix({ outputDir, exec, repoRoot, timeoutMs, git, sourceFingerprint, resourcePolicy, scenario: "primary", seeds: MATRIX_SEEDS, naturalDays, behavior: request.behavior, event: request.event, c01: request.c01, permit, resume, atomicWrite });
      canonical.set(key, report);
    }
    dimensions.push({ dimension: request.dimension, multiplier: request.multiplier, reuse: reused ? { canonical_spec: key, validated: true } : { executed_or_resumed: true }, report });
  }
  if (dimensions.some((dimension) => !dimension.report.complete)) return { command: "sensitivity", incomplete: true, dimensions };
  const finalizedDimensions = await finalizeSensitivityReports(dimensions, { exec, repoRoot, timeoutMs, outputDir, permit, atomicWrite });
  if (finalizedDimensions.some((dimension) => !dimension.report.finalized)) return { command: "sensitivity", incomplete: true, dimensions: finalizedDimensions };
  const manifest = { command: "sensitivity", source: "fresh_current_k7_setup", git, source_fingerprint: sourceFingerprint, resource_policy: resourcePolicy, dimensions: finalizedDimensions, c06_external_market_calibration: "not_completed_no_authorized_data" };
  await atomicWrite(path.join(outputDir, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  return manifest;
}

export async function main(argv) {
  const { command, outputDir, scenarios, resume, batchSize, maximumThreadCount } = parseCliArgs(argv);
  if (command === "after") { await captureAfter({ outputDir, resume, batchSize, maximumThreadCount }); return; }
  if (command === "sensitivity") { await captureSensitivity({ outputDir, resume, batchSize, maximumThreadCount }); return; }
  const manifest = await captureBaseline({ outputDir, scenarios, command });
  for (const [scenarioName, scenario] of Object.entries(manifest.scenarios)) {
    const totalMs = scenario.runs.reduce((sum, run) => sum + run.wall_ms, 0);
    console.log(
      `[${scenarioName}] ${scenario.runs.length} 个 seed 全部对账通过，总耗时 ${(totalMs / 1000).toFixed(1)}s`,
    );
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(`基线采集失败：${error.message}`);
    process.exitCode = 1;
  });
}
