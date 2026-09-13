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

export function parseCliArgs(argv) {
  const [command, ...rest] = argv;
  if (!new Set(["before", "after", "sensitivity"]).has(command)) {
    throw new Error(`未知命令 ${JSON.stringify(command ?? "(空)")}；仅支持 before、after、sensitivity`);
  }
  let outputDir;
  let scenarioArg = "all";
  for (let index = 0; index < rest.length; index += 1) {
    const flag = rest[index];
    const value = rest[index + 1];
    if (value === undefined) {
      throw new Error(`参数 ${flag} 缺少取值；用法：before --output <目录> [--scenario name|all]`);
    }
    if (flag === "--output") {
      outputDir = value;
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
  if (command !== "before") return { command, outputDir, scenarios: ["primary"] };
  const scenarios = scenarioArg === "all" ? Object.keys(SCENARIOS) : scenarioArg.split(",");
  for (const name of scenarios) {
    if (!Object.hasOwn(SCENARIOS, name)) {
      throw new Error(`未知场景 ${name}；可选：${Object.keys(SCENARIOS).join("、")} 或 all`);
    }
  }
  return { command, outputDir, scenarios };
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
function realExec(file, args, { cwd, timeoutMs }) {
  return new Promise((resolve, reject) => {
    const child = spawn(file, args, { cwd, windowsHide: true });
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
      version: (await mustSucceed(exec, repoRoot, "pnpm", ["--version"])).trim(),
    };
  } catch (error) {
    toolchain.pnpm = { available: false, error: error.message };
    blocked.push(`pnpm --version 不可用：${error.message}`);
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

async function captureK7Run({ outputDir, exec, repoRoot, timeoutMs, scenario, seed, naturalDays, behavior, event, c01, write }) {
  const args = buildK7ExampleArgs(scenario, seed, naturalDays, behavior, event, c01);
  const startedAt = Date.now();
  const { code, stdout, stderr } = await exec("cargo", args, { cwd: repoRoot, timeoutMs });
  if (code !== 0) throw new Error(`${scenario} seed ${seed} failed with exit ${code}: ${stderr.trim()}`);
  let parsed;
  try { parsed = JSON.parse(stdout); } catch (error) { throw new Error(`${scenario} seed ${seed} output is not JSON: ${error.message}`); }
  validateK7Output({ scenario, seed, naturalDays, behavior, event, c01, parsed });
  const buffer = Buffer.from(stdout, "utf8");
  const sha256 = createHash("sha256").update(buffer).digest("hex");
  if (write) await fsp.writeFile(path.join(outputDir, `seed-${seed}.json`), buffer);
  return { seed, argv: ["cargo", ...args], exit_code: code, wall_ms: Date.now() - startedAt, sha256, raw: parsed };
}

async function captureK7Matrix({ outputDir, exec, repoRoot, timeoutMs, scenario, seeds, naturalDays, behavior, event, c01 }) {
  const dir = path.join(outputDir, `${scenario}-b${behavior}-e${event}-c${c01}`);
  await fsp.mkdir(dir, { recursive: true });
  const runs = [];
  for (const seed of seeds) runs.push(await captureK7Run({ outputDir: dir, exec, repoRoot, timeoutMs, scenario, seed, naturalDays, behavior, event, c01, write: true }));
  return { scenario, seeds: [...seeds], natural_days: naturalDays, multipliers: { behavior, event, c01_denominator_assumption: c01 }, runs, quantiles_and_extremes: summarizeK7Runs(runs) };
}

export async function captureAfter({ outputDir, exec = realExec, repoRoot = REPO_ROOT, timeoutMs = DEFAULT_TIMEOUT_MS, seeds = MATRIX_SEEDS, reportSource = "fresh_current_k7_setup", primaryNaturalDays = TRADING_DAYS, crossYearNaturalDays = 400 }) {
  if (reportSource !== "fresh_current_k7_setup") throw new Error("after report source must be fresh_current_k7_setup");
  if (!sameSeedList(seeds, MATRIX_SEEDS)) throw new Error("after seed list must exactly match Task 1 seed matrix");
  await ensureFreshOutputDir(outputDir);
  await fsp.mkdir(outputDir, { recursive: true });
  const primary = await captureK7Matrix({ outputDir, exec, repoRoot, timeoutMs, scenario: "primary", seeds, naturalDays: primaryNaturalDays, behavior: 1, event: 1, c01: 1 });
  const crossYear = await captureK7Matrix({ outputDir, exec, repoRoot, timeoutMs, scenario: "cross-year", seeds: CROSS_YEAR_SEEDS, naturalDays: crossYearNaturalDays, behavior: 1, event: 1, c01: 1 });
  const manifest = { command: "after", source: reportSource, primary, cross_year_four_industry: crossYear, c06_external_market_calibration: "not_completed_no_authorized_data" };
  await fsp.writeFile(path.join(outputDir, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  return manifest;
}

export async function captureSensitivity({ outputDir, exec = realExec, repoRoot = REPO_ROOT, timeoutMs = DEFAULT_TIMEOUT_MS, behaviorMultipliers = SENSITIVITY_MULTIPLIERS, eventMultipliers = SENSITIVITY_MULTIPLIERS, c01Multipliers = SENSITIVITY_MULTIPLIERS, naturalDays = TRADING_DAYS }) {
  requireSensitivityMatrix(behaviorMultipliers, "behavior");
  requireSensitivityMatrix(eventMultipliers, "event");
  requireSensitivityMatrix(c01Multipliers, "C01 denominator");
  await ensureFreshOutputDir(outputDir);
  await fsp.mkdir(outputDir, { recursive: true });
  const dimensions = [];
  for (const multiplier of behaviorMultipliers) dimensions.push({ dimension: "behavior", multiplier, report: await captureK7Matrix({ outputDir, exec, repoRoot, timeoutMs, scenario: "primary", seeds: MATRIX_SEEDS, naturalDays, behavior: multiplier, event: 1, c01: 1 }) });
  for (const multiplier of eventMultipliers) dimensions.push({ dimension: "event", multiplier, report: await captureK7Matrix({ outputDir, exec, repoRoot, timeoutMs, scenario: "primary", seeds: MATRIX_SEEDS, naturalDays, behavior: 1, event: multiplier, c01: 1 }) });
  for (const multiplier of c01Multipliers) dimensions.push({ dimension: "c01_volume_denominator_assumption", multiplier, report: await captureK7Matrix({ outputDir, exec, repoRoot, timeoutMs, scenario: "primary", seeds: MATRIX_SEEDS, naturalDays, behavior: 1, event: 1, c01: multiplier }) });
  const manifest = { command: "sensitivity", source: "fresh_current_k7_setup", dimensions, c06_external_market_calibration: "not_completed_no_authorized_data" };
  await fsp.writeFile(path.join(outputDir, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  return manifest;
}

export async function main(argv) {
  const { command, outputDir, scenarios } = parseCliArgs(argv);
  if (command === "after") { await captureAfter({ outputDir }); return; }
  if (command === "sensitivity") { await captureSensitivity({ outputDir }); return; }
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
