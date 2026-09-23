import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdtemp, mkdir, readFile, readdir, rm, stat, symlink, unlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { after, describe, it } from "node:test";

import {
  MATRIX_SEEDS,
  CROSS_YEAR_SEEDS,
  SENSITIVITY_MULTIPLIERS,
  SCENARIOS,
  TRADING_DAYS,
  buildExampleArgs,
  buildK7ExampleArgs,
  buildK7ResourcePolicy,
  detectK7ResourcePolicy,
  captureAfter,
  captureSensitivity,
  captureBaseline,
  parseCliArgs,
  validateFixtureOutput,
  validateSeedMatrix,
} from "./baseline-run.mjs";

const tempDirs = [];
async function newTempDir() {
  const dir = await mkdtemp(path.join(tmpdir(), "baseline-run-test-"));
  tempDirs.push(dir);
  return dir;
}
after(async () => {
  await Promise.all(tempDirs.map((dir) => rm(dir, { force: true, recursive: true })));
});

const MATRIX = SCENARIOS.matrix;
const SORTED_CODES = [...MATRIX.stockCodes].sort();

/** 构造一个能通过全部对账校验的最小合法 fixture 输出。 */
function fakeFixtureJson(scenarioName, seed, overrides = {}) {
  const scenario = SCENARIOS[scenarioName];
  const stocks = Object.fromEntries(
    scenario.stockCodes.map((code) => [
      code,
      {
        completed_days: TRADING_DAYS,
        traded_days: TRADING_DAYS,
        zero_volume_days: 0,
        max_zero_volume_streak: 0,
        daily_returns_bps: Array.from({ length: TRADING_DAYS }, () => 0.5),
        total_daily_volume: "1000",
        trade_event_volume: "1000",
        total_daily_turnover_cents: "1000000",
        trade_event_turnover_cents: "1000000",
        auction_volume: "100",
        continuous_volume: "900",
      },
    ]),
  );
  return JSON.stringify({
    tool: "baseline_fixture",
    scenario: scenarioName,
    seed: String(seed),
    trading_days: TRADING_DAYS,
    config: {
      stock_codes: SORTED_CODES,
      retail_count: scenario.retailCount,
      inst_count: scenario.instCount,
      hot_count: scenario.hotCount,
      ticks_per_day: String(scenario.ticksPerDay),
      auction_ticks: String(scenario.auctionTicks),
      closing_auction_ticks: String(scenario.closingAuctionTicks),
      history_len: scenario.historyLen,
      t1_enabled: true,
    },
    report: {
      trading_days: TRADING_DAYS,
      ticks_per_day: String(scenario.ticksPerDay),
      runs: [
        {
          seed: String(seed),
          final_tick: String(scenario.ticksPerDay * TRADING_DAYS),
          trade_events: "50",
          rejection_events: "0",
          engine_error_events: "0",
          retail_behavior: {
            observed_decisions: "10",
            desired_buy_shares: "100",
            desired_sell_shares: "50",
            executable_buy_shares: "100",
            executable_sell_shares: "50",
            action_counts: { buy: "5", sell: "2", hold: "3" },
            reason_counts: { dip: "5" },
          },
          retail_execution: {
            submitted_orders: "10",
            submitted_shares: "1000",
            filled_shares: "900",
            canceled_shares: "0",
            aborted_shares: "0",
            open_shares: "100",
            rejected_intents: 0,
            rejection_reason_counts: {},
          },
          participant_execution: {
            two_sided_participant_shares: String(scenario.stockCodes.length * 1000 * 2),
            two_sided_participant_shares_by_profile: { player: "1000", retail_noise: "9000" },
          },
          stocks,
        },
      ],
      stocks: Object.fromEntries(
        scenario.stockCodes.map((code) => [
          code,
          { seed_count: 1, mean_daily_volume: { sample_count: 1, minimum: 1000, maximum: 1000 } },
        ]),
      ),
      extreme_cases: scenario.stockCodes.map((code) => ({
        code,
        metric: "terminal_return_bps",
        seed: String(seed),
        value: 1.5,
      })),
    },
    ...overrides,
  });
}

/** 在合法输出基础上改写 report.runs[0] 的字段，便于负向用例复用。 */
function withRun0Field(scenarioName, seed, field, value) {
  const parsed = JSON.parse(fakeFixtureJson(scenarioName, seed));
  parsed.report.runs[0][field] = value;
  return JSON.stringify(parsed);
}

/** 可注入的 exec 桩：按命令分发，模拟真实子进程边界；fixture 调用可按需覆写。 */
function fakeExec(fixtureOverride = () => undefined) {
  return async (file, args) => {
    if (file === "git" && args[0] === "rev-parse") {
      if (args[1] === "HEAD^{tree}") {
        return { code: 0, stdout: "d6e7c4a051f231a789ef589c4ec33580ad082109\n", stderr: "" };
      }
      return { code: 0, stdout: "6ad461e7f735ee1a0b4090497c58380af3422761\n", stderr: "" };
    }
    if (file === "git" && args[0] === "diff") {
      return { code: 0, stdout: "diff --git a/packages/engine/examples/k7_baseline_fixture.rs b/packages/engine/examples/k7_baseline_fixture.rs\n", stderr: "" };
    }
    if (file === "git" && args[0] === "ls-files") {
      return { code: 0, stdout: "", stderr: "" };
    }
    if (file === "git" && args[0] === "status") {
      return { code: 0, stdout: "?? .omo/\n", stderr: "" };
    }
    if (file === "git" && args[0] === "branch") {
      return { code: 0, stdout: "codex/feat/web-ui-polish\n", stderr: "" };
    }
    if (file === "node" && args[0] === "--version") {
      return { code: 0, stdout: "v24.18.0\n", stderr: "" };
    }
    if (file === "cargo" && args[0] === "--version") {
      return { code: 0, stdout: "cargo 1.96.1\n", stderr: "" };
    }
    if (file === "corepack") {
      throw new Error(`spawn ${file} ENOENT`);
    }
    if (file === "cargo" && args.includes("baseline_fixture")) {
      const scenarioName = args[args.indexOf("--") + 1];
      const seed = Number(args[args.indexOf("--") + 2]);
      const stdout = fixtureOverride(scenarioName, seed);
      if (stdout !== undefined) {
        return { code: 0, stdout: `${stdout}\n`, stderr: "" };
      }
      return { code: 0, stdout: `${fakeFixtureJson(scenarioName, seed)}\n`, stderr: "" };
    }
    throw new Error(`fakeExec: 未预期的调用 ${file} ${args.join(" ")}`);
  };
}

function fakeK7Exec({ failAfter = Number.POSITIVE_INFINITY, calls = new Map() } = {}) {
  const base = fakeExec();
  let completed = 0;
  return async (file, args, options) => {
    if (file !== "cargo" || !args.includes("k7_baseline_fixture")) return base(file, args, options);
    const marker = args.indexOf("--");
    const scenario = args[marker + 1];
    const seed = Number(args[marker + 2]);
    const executionKey = `${scenario}:${seed}`;
    calls.set(executionKey, (calls.get(executionKey) ?? 0) + 1);
    if (completed >= failAfter) return { code: 137, stdout: "", stderr: "injected child interruption" };
    completed += 1;
    const days = Number(args[marker + 3]);
    const parsed = JSON.parse(fakeFixtureJson("matrix", seed));
    parsed.tool = "k7_baseline_fixture";
    parsed.source = "fresh_current_k7_setup";
    parsed.scenario = args[marker + 1];
    parsed.natural_days = days;
    parsed.calendar = { natural_days: days, trading_days: days, closed_days: 0, policy_id: "a-share-simulation-v1" };
    parsed.multipliers = { behavior: Number(args[marker + 4]), event: Number(args[marker + 5]), c01_denominator_assumption: Number(args[marker + 6]) };
    parsed.price_volume = parsed.report;
    parsed.causal = { ratio_absent_reason: null };
    return { code: 0, stdout: JSON.stringify(parsed), stderr: "" };
  };
}

function k7CallCount(calls) {
  return [...calls.values()].reduce((total, count) => total + count, 0);
}

function runGit(args, cwd) {
  return new Promise((resolve, reject) => {
    execFile("git", args, { cwd }, (error, stdout, stderr) => {
      if (error) {
        reject(error);
        return;
      }
      resolve({ code: 0, stdout, stderr });
    });
  });
}

async function createK7SourceRepo() {
  const repoRoot = await newTempDir();
  await Promise.all([
    mkdir(path.join(repoRoot, "scripts", "simulation"), { recursive: true }),
    mkdir(path.join(repoRoot, "packages", "engine", "examples"), { recursive: true }),
    mkdir(path.join(repoRoot, "packages", "engine", "src"), { recursive: true }),
  ]);
  await Promise.all([
    writeFile(path.join(repoRoot, "Cargo.toml"), "[workspace]\nmembers = [\"packages/engine\"]\n"),
    writeFile(path.join(repoRoot, "Cargo.lock"), "version = 4\n"),
    writeFile(path.join(repoRoot, ".gitignore"), "packages/engine/ignored-k7-input.txt\n"),
    writeFile(path.join(repoRoot, "scripts", "simulation", "baseline-run.mjs"), "export const fixture = 'initial';\n"),
    writeFile(path.join(repoRoot, "packages", "engine", "Cargo.toml"), "[package]\nname = \"engine\"\nversion = \"0.1.0\"\n"),
    writeFile(path.join(repoRoot, "packages", "engine", "examples", "k7_baseline_fixture.rs"), "fn main() { println!(\"initial\"); }\n"),
    writeFile(path.join(repoRoot, "packages", "engine", "src", "lib.rs"), "pub fn fixture() {}\n"),
  ]);
  await runGit(["init", "--quiet"], repoRoot);
  await runGit(["config", "user.email", "baseline@example.test"], repoRoot);
  await runGit(["config", "user.name", "Baseline Test"], repoRoot);
  await runGit(["add", "."], repoRoot);
  await runGit(["commit", "--quiet", "-m", "fixture"], repoRoot);
  return repoRoot;
}

function sourceAwareK7Exec(k7Exec) {
  return async (file, args, options) => {
    if (file === "git") return runGit(args, options.cwd);
    return k7Exec(file, args, options);
  };
}

describe("seed 矩阵校验", () => {
  it("接受规范矩阵（10 个互异 seed）", () => {
    validateSeedMatrix(MATRIX_SEEDS);
    assert.equal(new Set(MATRIX_SEEDS).size, 10);
  });

  it("拒绝重复 seed 并点名重复值", () => {
    assert.throws(() => validateSeedMatrix([1, 2, 2, 3]), /重复 seed：2/);
    assert.throws(() => validateSeedMatrix(MATRIX_SEEDS.concat(7)), /重复 seed：7/);
  });

  it("拒绝空矩阵", () => {
    assert.throws(() => validateSeedMatrix([]), /seed 矩阵不能为空/);
  });
});

describe("cargo example 参数构造", () => {
  it("生成精确的 release example argv", () => {
    assert.deepEqual(buildExampleArgs("matrix", 7, 30), [
      "run",
      "-p",
      "engine",
      "--release",
      "--example",
      "baseline_fixture",
      "--",
      "matrix",
      "7",
      "30",
    ]);
  });

  it("为 K7 自然日 fixture 传递明确模式、seed 与三类倍率", () => {
    assert.deepEqual(buildK7ExampleArgs("primary", 7, 30, 1, 1, 1), [
      "run", "-p", "engine", "--release", "--features", "simulation-diagnostics", "--example", "k7_baseline_fixture", "--",
      "primary", "7", "30", "1", "1", "1",
    ]);
  });

  it("derives a serial K7 policy that assigns every process-available CPU to one child", () => {
    assert.deepEqual(buildK7ResourcePolicy(3, "node_available_parallelism"), {
      schema: "k7-resource-policy-v3",
      available_cpu_count: 3,
      available_cpu_source: "node_available_parallelism",
      maximum_thread_count: "auto",
      max_concurrent_seed_processes: 1,
      rayon_threads_per_seed: 3,
    });
  });

  it("caps the serial Rayon budget at an explicit positive maximum without claiming saturation", () => {
    assert.deepEqual(buildK7ResourcePolicy(12, "node_available_parallelism", 3), {
      schema: "k7-resource-policy-v3",
      available_cpu_count: 12,
      available_cpu_source: "node_available_parallelism",
      maximum_thread_count: 3,
      max_concurrent_seed_processes: 1,
      rayon_threads_per_seed: 3,
    });
  });

  it("prefers the process-aware Node count and tightens it with an observable cgroup CPU quota", async () => {
    const constrained = await detectK7ResourcePolicy({
      availableParallelism: () => 12,
      logicalCpuCount: () => 64,
      readCpuMax: async () => "250000 100000\n",
    });
    assert.deepEqual(constrained, {
      schema: "k7-resource-policy-v3",
      available_cpu_count: 2,
      available_cpu_source: "node_available_parallelism+cgroup_cpu_max",
      maximum_thread_count: "auto",
      max_concurrent_seed_processes: 1,
      rayon_threads_per_seed: 2,
    });
    const fallback = await detectK7ResourcePolicy({
      availableParallelism: () => 0,
      logicalCpuCount: () => 7,
      readCpuMax: async () => { throw Object.assign(new Error("missing"), { code: "ENOENT" }); },
    });
    assert.equal(fallback.available_cpu_count, 7);
    assert.equal(fallback.available_cpu_source, "host_logical_cpu_count");
  });

  it("uses the cgroup-and-affinity-aware detected count as an upper bound for an explicit maximum", async () => {
    const policy = await detectK7ResourcePolicy({
      availableParallelism: () => 6,
      logicalCpuCount: () => 64,
      readCpuMax: async () => "250000 100000\n",
      maximumThreadCount: 9,
    });
    assert.equal(policy.available_cpu_count, 2);
    assert.equal(policy.maximum_thread_count, 9);
    assert.equal(policy.rayon_threads_per_seed, 2);
  });

  it("does not consult the host fallback when Node reports a process-available count", async () => {
    const policy = await detectK7ResourcePolicy({
      availableParallelism: () => 4,
      logicalCpuCount: () => { throw new Error("fallback must stay unused"); },
      readCpuMax: async () => "max 100000\n",
    });
    assert.equal(policy.available_cpu_count, 4);
    assert.equal(policy.available_cpu_source, "node_available_parallelism");
  });
});

describe("CLI 参数解析", () => {
  it("解析 before 命令与 --output", () => {
    const parsed = parseCliArgs(["before", "--output", "E/before"]);
    assert.equal(parsed.command, "before");
    assert.equal(parsed.outputDir, "E/before");
    assert.deepEqual(parsed.scenarios, Object.keys(SCENARIOS));
  });

  it("支持 --scenario 过滤并拒绝未知场景", () => {
    assert.deepEqual(parseCliArgs(["before", "--output", "x", "--scenario", "matrix"]).scenarios, [
      "matrix",
    ]);
    assert.throws(() => parseCliArgs(["before", "--output", "x", "--scenario", "nope"]), /nope/);
  });

  it("缺少 --output 或未知命令时显式报错", () => {
    assert.throws(() => parseCliArgs(["before"]), /--output/);
    assert.deepEqual(parseCliArgs(["after", "--output", "x"]).command, "after");
    assert.deepEqual(parseCliArgs(["sensitivity", "--output", "x"]).command, "sensitivity");
  });

  it("parses explicit K7 resume and bounded-batch controls", () => {
    assert.deepEqual(parseCliArgs(["after", "--output", "x", "--resume", "--batch-size", "1"]), {
      command: "after", outputDir: "x", scenarios: ["primary"], resume: true, batchSize: 1, maximumThreadCount: "auto",
    });
    assert.throws(() => parseCliArgs(["after", "--output", "x", "--batch-size", "0"]), /positive integer/);
  });

  it("parses auto or an explicit K7 maximum-thread cap and rejects invalid values", () => {
    assert.equal(parseCliArgs(["after", "--output", "x", "--max-threads", "auto"]).maximumThreadCount, "auto");
    assert.equal(parseCliArgs(["sensitivity", "--output", "x", "--max-threads", "3"]).maximumThreadCount, 3);
    assert.throws(() => parseCliArgs(["after", "--output", "x", "--max-threads", "0"]), /positive integer or auto/);
    assert.throws(() => parseCliArgs(["after", "--output", "x", "--max-threads", "3.5"]), /positive integer or auto/);
    assert.throws(() => parseCliArgs(["before", "--output", "x", "--max-threads", "2"]), /未知参数/);
  });
});

describe("Task 38 K7 capture contracts", () => {
  it("rejects a non-fresh or save-backed after report", async () => {
    await assert.rejects(
      captureAfter({ outputDir: "unused", reportSource: "save_slot" }),
      /fresh_current_k7_setup/,
    );
  });

  it("rejects primary seeds that differ from the before matrix", async () => {
    await assert.rejects(
      captureAfter({ outputDir: "unused", seeds: MATRIX_SEEDS.slice(1) }),
      /seed.*Task 1|Task 1.*seed/,
    );
  });

  it("rejects an incomplete sensitivity multiplier matrix", async () => {
    await assert.rejects(
      captureSensitivity({ outputDir: "unused", behaviorMultipliers: [0.5, 1] }),
      /0.5.*1.*2/,
    );
    assert.equal(CROSS_YEAR_SEEDS.length, 5);
    assert.deepEqual(SENSITIVITY_MULTIPLIERS, [0.5, 1, 2]);
  });

  it("retains actual zero-trade raw records and summaries instead of calling them zero-valued samples", async () => {
    const outputDir = await newTempDir();
    const exec = fakeExec();
    const k7Exec = async (file, args, options) => {
      if (file === "cargo" && args.includes("k7_baseline_fixture")) {
        const marker = args.indexOf("--");
        const seed = Number(args[marker + 2]);
        const days = Number(args[marker + 3]);
        const parsed = JSON.parse(fakeFixtureJson("matrix", seed));
        parsed.tool = "k7_baseline_fixture";
        parsed.source = "fresh_current_k7_setup";
        parsed.scenario = args[marker + 1];
        parsed.natural_days = days;
        parsed.calendar = { natural_days: days, trading_days: days, closed_days: 0 };
        parsed.multipliers = { behavior: Number(args[marker + 4]), event: Number(args[marker + 5]), c01_denominator_assumption: Number(args[marker + 6]) };
        parsed.price_volume = parsed.report;
        parsed.price_volume.runs[0].retail_execution.filled_share_ratio = null;
        parsed.causal = { ratio_absent_reason: "no_submissions" };
        return { code: 0, stdout: JSON.stringify(parsed), stderr: "" };
      }
      return exec(file, args, options);
    };
    const report = await captureAfter({ outputDir: path.join(outputDir, "after"), exec: k7Exec, repoRoot: "D:/repo", primaryNaturalDays: TRADING_DAYS, crossYearNaturalDays: TRADING_DAYS });
    assert.equal(report.primary.runs[0].raw.price_volume.runs[0].retail_execution.filled_share_ratio, null);
    assert.equal(report.primary.runs[0].raw.causal.ratio_absent_reason, "no_submissions");
    assert.equal(report.primary.quantiles_and_extremes[SORTED_CODES[0]].raw_seed_count, MATRIX_SEEDS.length);
  });

  it("runs K7 seeds serially with the recorded full process CPU budget", async () => {
    const outputDir = await newTempDir();
    const resourcePolicy = buildK7ResourcePolicy(4, "node_available_parallelism");
    const base = fakeK7Exec();
    let activeChildren = 0;
    let maxActiveChildren = 0;
    const observedRayonThreads = new Set();
    const exec = async (file, args, options) => {
      if (file !== "cargo" || !args.includes("k7_baseline_fixture")) {
        return base(file, args, options);
      }
      activeChildren += 1;
      maxActiveChildren = Math.max(maxActiveChildren, activeChildren);
      observedRayonThreads.add(options.env.RAYON_NUM_THREADS);
      await new Promise((resolve) => setImmediate(resolve));
      try {
        return await base(file, args, options);
      } finally {
        activeChildren -= 1;
      }
    };

    const manifest = await captureAfter({
      outputDir: path.join(outputDir, "after"),
      exec,
      repoRoot: "D:/repo",
      primaryNaturalDays: TRADING_DAYS,
      crossYearNaturalDays: TRADING_DAYS,
      resourcePolicy,
    });

    assert.equal(maxActiveChildren, 1);
    assert.deepEqual(observedRayonThreads, new Set([String(resourcePolicy.rayon_threads_per_seed)]));
    assert.deepEqual(manifest.resource_policy, resourcePolicy);
    assert.deepEqual(manifest.primary.resource_policy, resourcePolicy);
  });

  it("preserves byte-identical K7 raw reports across auto and explicit thread budgets", async () => {
    const outputDir = await newTempDir();
    const autoPolicy = buildK7ResourcePolicy(4, "node_available_parallelism");
    const cappedPolicy = buildK7ResourcePolicy(4, "node_available_parallelism", 1);
    const common = {
      exec: fakeK7Exec(),
      repoRoot: "D:/repo",
      primaryNaturalDays: TRADING_DAYS,
      crossYearNaturalDays: TRADING_DAYS,
      batchSize: 1,
    };
    const auto = await captureAfter({ ...common, outputDir: path.join(outputDir, "auto"), resourcePolicy: autoPolicy });
    const capped = await captureAfter({ ...common, outputDir: path.join(outputDir, "capped"), resourcePolicy: cappedPolicy });

    assert.deepEqual(auto.primary.runs[0].raw, capped.primary.runs[0].raw);
    assert.equal(auto.primary.runs[0].sha256, capped.primary.runs[0].sha256);
    assert.equal(capped.primary.resource_policy.rayon_threads_per_seed, 1);
  });

  it("resumes an interrupted exact-spec matrix without rewriting the completed seed", async () => {
    const outputDir = await newTempDir();
    const target = path.join(outputDir, "after");
    const calls = new Map();
    const interrupted = fakeK7Exec({ failAfter: 1, calls });
    await assert.rejects(
      captureAfter({ outputDir: target, exec: interrupted, repoRoot: "D:/repo", primaryNaturalDays: TRADING_DAYS, crossYearNaturalDays: TRADING_DAYS }),
      /injected child interruption/,
    );
    const completedPath = path.join(target, "primary-b1-e1-c1", "seed-1.json");
    const completedBytes = await readFile(completedPath);
    const completedStat = await stat(completedPath);
    const executionsBeforeResume = calls.get("primary:1");
    await captureAfter({ outputDir: target, exec: fakeK7Exec({ calls }), repoRoot: "D:/repo", primaryNaturalDays: TRADING_DAYS, crossYearNaturalDays: TRADING_DAYS, resume: true });
    assert.deepEqual(await readFile(completedPath), completedBytes);
    assert.equal((await stat(completedPath)).mtimeMs, completedStat.mtimeMs);
    assert.equal(calls.get("primary:1"), executionsBeforeResume, "resume must not execute an already checkpointed seed");
    const manifest = JSON.parse(await readFile(path.join(target, "manifest.json"), "utf8"));
    assert.equal(manifest.primary.runs.length, MATRIX_SEEDS.length);
  });

  it("rejects resume with a precise error when only the maximum-thread policy changes", async () => {
    const outputDir = await newTempDir();
    const target = path.join(outputDir, "after");
    const autoPolicy = buildK7ResourcePolicy(4, "node_available_parallelism");
    await captureAfter({
      outputDir: target,
      exec: fakeK7Exec(),
      repoRoot: "D:/repo",
      primaryNaturalDays: TRADING_DAYS,
      crossYearNaturalDays: TRADING_DAYS,
      batchSize: 1,
      resourcePolicy: autoPolicy,
    });
    await assert.rejects(
      captureAfter({
        outputDir: target,
        exec: fakeK7Exec(),
        repoRoot: "D:/repo",
        primaryNaturalDays: TRADING_DAYS,
        crossYearNaturalDays: TRADING_DAYS,
        resume: true,
        resourcePolicy: buildK7ResourcePolicy(4, "node_available_parallelism", 1),
      }),
      /resource policy mismatch/,
    );
  });

  it("reuses an exact completed seed when only the runner's ignored evidence output dirties Git status", async () => {
    const repoRoot = await createK7SourceRepo();
    const target = path.join(repoRoot, ".omo", "k7-after");
    const calls = new Map();

    await captureAfter({
      outputDir: target,
      exec: sourceAwareK7Exec(fakeK7Exec({ calls })),
      repoRoot,
      primaryNaturalDays: TRADING_DAYS,
      crossYearNaturalDays: TRADING_DAYS,
      batchSize: 1,
    });
    const executionsBeforeResume = calls.get("primary:1");

    await captureAfter({
      outputDir: target,
      exec: sourceAwareK7Exec(fakeK7Exec({ calls })),
      repoRoot,
      primaryNaturalDays: TRADING_DAYS,
      crossYearNaturalDays: TRADING_DAYS,
      resume: true,
    });

    assert.equal(calls.get("primary:1"), executionsBeforeResume, "evidence-output status must not invalidate an unchanged K7 source fingerprint");
  });

  it("rejects resume when dirty K7 fixture bytes change under the same dirty path", async () => {
    const repoRoot = await createK7SourceRepo();
    const outputDir = await newTempDir();
    const fixturePath = path.join(repoRoot, "packages", "engine", "examples", "k7_baseline_fixture.rs");
    await writeFile(fixturePath, "fn main() { println!(\"dirty-first\"); }\n");
    await captureAfter({
      outputDir,
      exec: sourceAwareK7Exec(fakeK7Exec()),
      repoRoot,
      primaryNaturalDays: TRADING_DAYS,
      crossYearNaturalDays: TRADING_DAYS,
      batchSize: 1,
    });
    await writeFile(fixturePath, "fn main() { println!(\"dirty-second\"); }\n");
    await assert.rejects(
      captureAfter({
        outputDir,
        exec: sourceAwareK7Exec(fakeK7Exec()),
        repoRoot,
        primaryNaturalDays: TRADING_DAYS,
        crossYearNaturalDays: TRADING_DAYS,
        resume: true,
      }),
      /source fingerprint|identity/i,
    );
    await assert.rejects(readFile(path.join(outputDir, "manifest.json"), "utf8"), /ENOENT/);
  });

  it("rejects resume when ignored K7 source bytes change", async () => {
    const repoRoot = await createK7SourceRepo();
    const outputDir = await newTempDir();
    const ignoredPath = path.join(repoRoot, "packages", "engine", "ignored-k7-input.txt");
    await writeFile(ignoredPath, "ignored-first\n");
    await captureAfter({
      outputDir,
      exec: sourceAwareK7Exec(fakeK7Exec()),
      repoRoot,
      primaryNaturalDays: TRADING_DAYS,
      crossYearNaturalDays: TRADING_DAYS,
      batchSize: 1,
    });
    await writeFile(ignoredPath, "ignored-second\n");
    await assert.rejects(
      captureAfter({
        outputDir,
        exec: sourceAwareK7Exec(fakeK7Exec()),
        repoRoot,
        primaryNaturalDays: TRADING_DAYS,
        crossYearNaturalDays: TRADING_DAYS,
        resume: true,
      }),
      /source fingerprint|identity/i,
    );
  });

  it("rejects an unsupported symlink in a declared K7 source root", async () => {
    const repoRoot = await createK7SourceRepo();
    const outputDir = await newTempDir();
    const targetPath = path.join(repoRoot, "Cargo.toml");
    await symlink(targetPath, path.join(repoRoot, "packages", "engine", "k7-source-link"));
    await assert.rejects(
      captureAfter({
        outputDir,
        exec: sourceAwareK7Exec(fakeK7Exec()),
        repoRoot,
        primaryNaturalDays: TRADING_DAYS,
        crossYearNaturalDays: TRADING_DAYS,
        batchSize: 1,
      }),
      /unsupported filesystem entry/i,
    );
  });

  it("recovers a completed seed receipt when interruption happens before aggregate checkpoint publication", async () => {
    const outputDir = await newTempDir();
    const target = path.join(outputDir, "after");
    const calls = new Map();
    await captureAfter({ outputDir: target, exec: fakeK7Exec({ calls }), repoRoot: "D:/repo", primaryNaturalDays: TRADING_DAYS, crossYearNaturalDays: TRADING_DAYS, batchSize: 1 });
    const primaryDir = path.join(target, "primary-b1-e1-c1");
    const seedPath = path.join(primaryDir, "seed-1.json");
    const before = await readFile(seedPath);
    const executionsBeforeResume = calls.get("primary:1");
    await unlink(path.join(primaryDir, "checkpoint.json"));
    await captureAfter({ outputDir: target, exec: fakeK7Exec({ calls }), repoRoot: "D:/repo", primaryNaturalDays: TRADING_DAYS, crossYearNaturalDays: TRADING_DAYS, resume: true });
    assert.deepEqual(await readFile(seedPath), before);
    assert.equal(calls.get("primary:1"), executionsBeforeResume);
  });

  for (const boundary of ["raw", "receipt", "aggregate"]) {
    it(`never reuses an incomplete seed when ${boundary} publication fails`, async () => {
      const outputDir = await newTempDir();
      const target = path.join(outputDir, "after");
      const primaryDir = path.join(target, "primary-b1-e1-c1");
      let injected = false;
      const atomicWrite = async (filePath, content) => {
        const isRaw = filePath.endsWith("seed-1.json");
        const isReceipt = filePath.endsWith("seed-1.checkpoint.json");
        const isAggregate = filePath.endsWith("checkpoint.json") && !isReceipt;
        if (!injected && ((boundary === "raw" && isRaw) || (boundary === "receipt" && isReceipt) || (boundary === "aggregate" && isAggregate))) {
          injected = true;
          throw new Error(`injected ${boundary} publication failure`);
        }
        await writeFile(filePath, content);
      };
      await assert.rejects(
        captureAfter({
          outputDir: target,
          exec: fakeK7Exec(),
          repoRoot: "D:/repo",
          primaryNaturalDays: TRADING_DAYS,
          crossYearNaturalDays: TRADING_DAYS,
          batchSize: 1,
          atomicWrite,
        }),
        new RegExp(`injected ${boundary} publication failure`),
      );
      await assert.rejects(readFile(path.join(target, "manifest.json"), "utf8"), /ENOENT/);
      if (boundary === "raw") {
        await assert.rejects(readFile(path.join(primaryDir, "seed-1.json"), "utf8"), /ENOENT/);
      }
      if (boundary === "receipt") {
        await assert.rejects(readFile(path.join(primaryDir, "seed-1.checkpoint.json"), "utf8"), /ENOENT/);
      }
      if (boundary === "aggregate") {
        assert.ok(await readFile(path.join(primaryDir, "seed-1.checkpoint.json"), "utf8"));
      }
      const resume = captureAfter({
        outputDir: target,
        exec: fakeK7Exec(),
        repoRoot: "D:/repo",
        primaryNaturalDays: TRADING_DAYS,
        crossYearNaturalDays: TRADING_DAYS,
        resume: true,
      });
      if (boundary === "receipt") {
        await assert.rejects(resume, /exact-spec checkpoint/);
        return;
      }
      await resume;
      assert.ok(await readFile(path.join(target, "manifest.json"), "utf8"));
    });
  }

  it("rejects malformed, stale, digest-mismatched, duplicate, and legacy checkpoints instead of recomputing them", async () => {
    const outputDir = await newTempDir();
    const target = path.join(outputDir, "after");
    const partial = fakeK7Exec();
    await captureAfter({ outputDir: target, exec: partial, repoRoot: "D:/repo", primaryNaturalDays: TRADING_DAYS, crossYearNaturalDays: TRADING_DAYS, batchSize: 1 });
    const checkpointPath = path.join(target, "primary-b1-e1-c1", "checkpoint.json");
    const checkpoint = JSON.parse(await readFile(checkpointPath, "utf8"));
    await writeFile(checkpointPath, "{not json");
    await assert.rejects(captureAfter({ outputDir: target, exec: fakeK7Exec(), repoRoot: "D:/repo", primaryNaturalDays: TRADING_DAYS, crossYearNaturalDays: TRADING_DAYS, resume: true }), /checkpoint.*JSON|JSON.*checkpoint/i);
    await writeFile(checkpointPath, `${JSON.stringify({ ...checkpoint, schema: "legacy-save-v0" })}\n`);
    await assert.rejects(captureAfter({ outputDir: target, exec: fakeK7Exec(), repoRoot: "D:/repo", primaryNaturalDays: TRADING_DAYS, crossYearNaturalDays: TRADING_DAYS, resume: true }), /legacy|schema/i);
    checkpoint.identity.git.revision = "stale-revision";
    await writeFile(checkpointPath, `${JSON.stringify(checkpoint)}\n`);
    await assert.rejects(captureAfter({ outputDir: target, exec: fakeK7Exec(), repoRoot: "D:/repo", primaryNaturalDays: TRADING_DAYS, crossYearNaturalDays: TRADING_DAYS, resume: true }), /revision|identity/i);
    checkpoint.identity.git.revision = "6ad461e7f735ee1a0b4090497c58380af3422761";
    checkpoint.completed.push({ ...checkpoint.completed[0] });
    await writeFile(checkpointPath, `${JSON.stringify(checkpoint)}\n`);
    await assert.rejects(captureAfter({ outputDir: target, exec: fakeK7Exec(), repoRoot: "D:/repo", primaryNaturalDays: TRADING_DAYS, crossYearNaturalDays: TRADING_DAYS, resume: true }), /digest|duplicate/i);
  });

  for (const mutation of [
    ["missing per-seed receipt", async (dir) => unlink(path.join(dir, "seed-1.checkpoint.json")), /receipt/i],
    ["missing raw output", async (dir) => unlink(path.join(dir, "seed-1.json")), /raw artifact/i],
    ["bad raw SHA", async (dir) => writeFile(path.join(dir, "seed-1.json"), "{}\n"), /digest|raw artifact/i],
  ]) {
    it(`rejects a checkpoint with ${mutation[0]} before reuse`, async () => {
      const outputDir = await newTempDir();
      const target = path.join(outputDir, "after");
      await captureAfter({ outputDir: target, exec: fakeK7Exec(), repoRoot: "D:/repo", primaryNaturalDays: TRADING_DAYS, crossYearNaturalDays: TRADING_DAYS, batchSize: 1 });
      const primaryDir = path.join(target, "primary-b1-e1-c1");
      await mutation[1](primaryDir);
      await assert.rejects(
        captureAfter({ outputDir: target, exec: fakeK7Exec(), repoRoot: "D:/repo", primaryNaturalDays: TRADING_DAYS, crossYearNaturalDays: TRADING_DAYS, resume: true }),
        mutation[2],
      );
      await assert.rejects(readFile(path.join(target, "manifest.json"), "utf8"), /ENOENT/);
    });
  }

  for (const [name, alter, expected] of [
    ["argv", (checkpoint) => { checkpoint.completed[0].argv = ["cargo", "wrong"]; }, /argv|digest/i],
    ["scenario", (checkpoint) => { checkpoint.identity.scenario = "cross-year"; }, /identity|scenario/i],
    ["natural days", (checkpoint) => { checkpoint.identity.natural_days = TRADING_DAYS + 1; }, /identity|natural/i],
    ["multipliers", (checkpoint) => { checkpoint.identity.multipliers.event = 2; }, /identity|multiplier/i],
  ]) {
    it(`rejects a checkpoint with changed ${name} before reuse`, async () => {
      const outputDir = await newTempDir();
      const target = path.join(outputDir, "after");
      await captureAfter({ outputDir: target, exec: fakeK7Exec(), repoRoot: "D:/repo", primaryNaturalDays: TRADING_DAYS, crossYearNaturalDays: TRADING_DAYS, batchSize: 1 });
      const checkpointPath = path.join(target, "primary-b1-e1-c1", "checkpoint.json");
      const checkpoint = JSON.parse(await readFile(checkpointPath, "utf8"));
      alter(checkpoint);
      await writeFile(checkpointPath, `${JSON.stringify(checkpoint)}\n`);
      await assert.rejects(
        captureAfter({ outputDir: target, exec: fakeK7Exec(), repoRoot: "D:/repo", primaryNaturalDays: TRADING_DAYS, crossYearNaturalDays: TRADING_DAYS, resume: true }),
        expected,
      );
    });
  }

  it("rejects a save-backed raw report before checkpoint publication", async () => {
    const outputDir = await newTempDir();
    const exec = fakeK7Exec();
    const saveBacked = async (file, args, options) => {
      const result = await exec(file, args, options);
      if (file === "cargo" && args.includes("k7_baseline_fixture") && result.code === 0) {
        const raw = JSON.parse(result.stdout);
        raw.source = "save_slot";
        raw.save_path = "legacy.json";
        return { ...result, stdout: JSON.stringify(raw) };
      }
      return result;
    };
    await assert.rejects(
      captureAfter({ outputDir: path.join(outputDir, "after"), exec: saveBacked, repoRoot: "D:/repo", primaryNaturalDays: TRADING_DAYS, crossYearNaturalDays: TRADING_DAYS, batchSize: 1 }),
      /fresh_current_k7_setup|save/i,
    );
  });

  it("reuses the validated canonical 1x sensitivity report rather than executing it three times", async () => {
    const outputDir = await newTempDir();
    const calls = new Map();
    const report = await captureSensitivity({ outputDir: path.join(outputDir, "sensitivity"), exec: fakeK7Exec({ calls }), repoRoot: "D:/repo" });
    assert.equal(calls.get("primary:1"), 7, "the 1x identity is executed once, not once per sensitivity dimension");
    const canonicalDimensions = report.dimensions.filter((dimension) => dimension.multiplier === 1);
    assert.equal(canonicalDimensions.length, 3);
    assert.equal(canonicalDimensions.filter((dimension) => dimension.reuse.validated).length, 2);
    assert.ok(canonicalDimensions.every((dimension) => dimension.report.runs.length === MATRIX_SEEDS.length));
    assert.ok(report.dimensions.every((dimension) => dimension.report.determinism_check?.identical === true));
  });

  it("returns incomplete rather than throwing when the bounded budget ends between K7 matrices", async () => {
    const outputDir = await newTempDir();
    const after = await captureAfter({
      outputDir: path.join(outputDir, "after"),
      exec: fakeK7Exec(),
      repoRoot: "D:/repo",
      primaryNaturalDays: TRADING_DAYS,
      crossYearNaturalDays: TRADING_DAYS,
      batchSize: MATRIX_SEEDS.length + 1,
    });
    assert.equal(after.incomplete, true);
    assert.equal(after.primary.complete, true);
    assert.equal(after.cross_year_four_industry.complete, false);
    await assert.rejects(readFile(path.join(outputDir, "after", "manifest.json"), "utf8"), /ENOENT/);

    const sensitivity = await captureSensitivity({
      outputDir: path.join(outputDir, "sensitivity"),
      exec: fakeK7Exec(),
      repoRoot: "D:/repo",
      batchSize: MATRIX_SEEDS.length + 1,
    });
    assert.equal(sensitivity.incomplete, true);
    assert.equal(sensitivity.dimensions[0].report.complete, true);
    assert.equal(sensitivity.dimensions.find((dimension) => dimension.dimension === "behavior" && dimension.multiplier === 2).report.complete, false);
    await assert.rejects(readFile(path.join(outputDir, "sensitivity", "manifest.json"), "utf8"), /ENOENT/);
  });

  it("charges every primary finalizer child against the hard after budget", async () => {
    const outputDir = await newTempDir();
    const calls = new Map();
    const report = await captureAfter({
      outputDir: path.join(outputDir, "after"),
      exec: fakeK7Exec({ calls }),
      repoRoot: "D:/repo",
      primaryNaturalDays: TRADING_DAYS,
      crossYearNaturalDays: TRADING_DAYS,
      batchSize: MATRIX_SEEDS.length,
    });

    assert.equal(k7CallCount(calls), MATRIX_SEEDS.length, "the finalizer must not launch after the final seed exhausts the batch");
    assert.equal(report.incomplete, true);
    assert.equal(report.primary.complete, true);
    assert.equal(report.primary.finalized, false);
    await assert.rejects(readFile(path.join(outputDir, "after", "manifest.json"), "utf8"), /ENOENT/);
  });

  it("charges every cross-year finalizer child and resumes only the unfinished finalizer", async () => {
    const outputDir = await newTempDir();
    const target = path.join(outputDir, "after");
    const calls = new Map();
    const first = await captureAfter({
      outputDir: target,
      exec: fakeK7Exec({ calls }),
      repoRoot: "D:/repo",
      primaryNaturalDays: TRADING_DAYS,
      crossYearNaturalDays: TRADING_DAYS,
      batchSize: MATRIX_SEEDS.length + 1 + CROSS_YEAR_SEEDS.length,
    });

    assert.equal(k7CallCount(calls), MATRIX_SEEDS.length + 1 + CROSS_YEAR_SEEDS.length);
    assert.equal(first.incomplete, true);
    assert.equal(first.primary.finalized, true);
    assert.equal(first.cross_year_four_industry.complete, true);
    assert.equal(first.cross_year_four_industry.finalized, false);
    const primaryFinalizerCalls = calls.get("primary:31");

    const resumed = await captureAfter({
      outputDir: target,
      exec: fakeK7Exec({ calls }),
      repoRoot: "D:/repo",
      primaryNaturalDays: TRADING_DAYS,
      crossYearNaturalDays: TRADING_DAYS,
      batchSize: 1,
      resume: true,
    });

    assert.equal(k7CallCount(calls), MATRIX_SEEDS.length + 1 + CROSS_YEAR_SEEDS.length + 1);
    assert.equal(calls.get("primary:31"), primaryFinalizerCalls, "a persisted primary finalizer must not repeat on resume");
    assert.equal(resumed.incomplete, undefined);
    assert.equal(resumed.cross_year_four_industry.finalized, true);
  });

  it("charges all sensitivity finalizers against the hard batch budget", async () => {
    const outputDir = await newTempDir();
    const target = path.join(outputDir, "sensitivity");
    const calls = new Map();
    const report = await captureSensitivity({
      outputDir: target,
      exec: fakeK7Exec({ calls }),
      repoRoot: "D:/repo",
      batchSize: 70,
    });

    assert.equal(k7CallCount(calls), 70, "seven unique sensitivity matrices must not start their unbudgeted finalizers");
    assert.equal(report.incomplete, true);
    assert.ok(report.dimensions.every((dimension) => dimension.report.complete));
    assert.ok(report.dimensions.every((dimension) => dimension.report.finalized === false));
    await assert.rejects(readFile(path.join(target, "manifest.json"), "utf8"), /ENOENT/);

    const resumed = await captureSensitivity({
      outputDir: target,
      exec: fakeK7Exec({ calls }),
      repoRoot: "D:/repo",
      batchSize: 7,
      resume: true,
    });
    assert.equal(k7CallCount(calls), 77, "resume spends permits only on the seven persisted-matrix finalizers");
    assert.ok(resumed.dimensions.every((dimension) => dimension.report.finalized));
    assert.ok(await readFile(path.join(target, "manifest.json"), "utf8"));
  });

  it("publishes the complete cryptographic source identity in final manifests and matrix records", async () => {
    const outputDir = await newTempDir();
    const target = path.join(outputDir, "after");
    const manifest = await captureAfter({
      outputDir: target,
      exec: fakeK7Exec(),
      repoRoot: "D:/repo",
      primaryNaturalDays: TRADING_DAYS,
      crossYearNaturalDays: TRADING_DAYS,
    });

    assert.equal(manifest.source_fingerprint.algorithm, "k7-simulation-source-v1");
    assert.match(manifest.source_fingerprint.digest, /^[a-f0-9]{64}$/);
    assert.deepEqual(manifest.primary.source_fingerprint, manifest.source_fingerprint);
    const receipt = JSON.parse(await readFile(path.join(target, "primary-b1-e1-c1", "seed-1.checkpoint.json"), "utf8"));
    assert.deepEqual(receipt.identity.source_fingerprint, manifest.source_fingerprint);
    const onDisk = JSON.parse(await readFile(path.join(target, "manifest.json"), "utf8"));
    assert.deepEqual(onDisk.source_fingerprint, manifest.source_fingerprint);
  });

  it("rejects a tampered deterministic receipt before manifest publication", async () => {
    const outputDir = await newTempDir();
    const target = path.join(outputDir, "after");
    await captureAfter({
      outputDir: target,
      exec: fakeK7Exec(),
      repoRoot: "D:/repo",
      primaryNaturalDays: TRADING_DAYS,
      crossYearNaturalDays: TRADING_DAYS,
      batchSize: MATRIX_SEEDS.length + 1,
    });
    const receiptPath = path.join(target, "primary-b1-e1-c1", "determinism.checkpoint.json");
    const receipt = JSON.parse(await readFile(receiptPath, "utf8"));
    delete receipt.identity.source_fingerprint;
    await writeFile(receiptPath, `${JSON.stringify(receipt)}\n`);

    await assert.rejects(
      captureAfter({
        outputDir: target,
        exec: fakeK7Exec(),
        repoRoot: "D:/repo",
        primaryNaturalDays: TRADING_DAYS,
        crossYearNaturalDays: TRADING_DAYS,
        resume: true,
      }),
      /determinism receipt.*identity|identity.*source fingerprint/i,
    );
    await assert.rejects(readFile(path.join(target, "manifest.json"), "utf8"), /ENOENT/);
  });

  it("reuses a persisted deterministic receipt after final-manifest publication fails", async () => {
    const outputDir = await newTempDir();
    const target = path.join(outputDir, "after");
    const calls = new Map();
    let manifestFailureInjected = false;
    const atomicWrite = async (filePath, content) => {
      if (!manifestFailureInjected && filePath === path.join(target, "manifest.json")) {
        manifestFailureInjected = true;
        throw new Error("injected final manifest publication failure");
      }
      await writeFile(filePath, content);
    };
    await assert.rejects(
      captureAfter({
        outputDir: target,
        exec: fakeK7Exec({ calls }),
        repoRoot: "D:/repo",
        primaryNaturalDays: TRADING_DAYS,
        crossYearNaturalDays: TRADING_DAYS,
        atomicWrite,
      }),
      /injected final manifest publication failure/,
    );
    const executionsBeforeResume = k7CallCount(calls);
    const resumed = await captureAfter({
      outputDir: target,
      exec: fakeK7Exec({ calls }),
      repoRoot: "D:/repo",
      primaryNaturalDays: TRADING_DAYS,
      crossYearNaturalDays: TRADING_DAYS,
      resume: true,
    });

    assert.equal(k7CallCount(calls), executionsBeforeResume, "resume must reconstruct finalization from its persisted receipts");
    assert.equal(resumed.primary.finalized, true);
    assert.equal(resumed.cross_year_four_industry.finalized, true);
  });
});

describe("单份报告对账校验", () => {
  it("接受合法报告", () => {
    validateFixtureOutput("matrix", 7, TRADING_DAYS, JSON.parse(fakeFixtureJson("matrix", 7)));
  });

  it("拒绝 seed 不匹配", () => {
    const parsed = JSON.parse(fakeFixtureJson("matrix", 7));
    parsed.seed = "8";
    assert.throws(
      () => validateFixtureOutput("matrix", 7, TRADING_DAYS, parsed),
      /seed.*期望.*7.*实际.*8|实际.*8.*期望.*7/,
    );
  });

  it("拒绝交易日数与请求配置不符", () => {
    const parsed = JSON.parse(fakeFixtureJson("matrix", 7));
    parsed.report.trading_days = 29;
    assert.throws(() => validateFixtureOutput("matrix", 7, TRADING_DAYS, parsed), /trading_days/);
  });

  it("拒绝逐笔成交量与日 K 成交量不一致", () => {
    const parsed = JSON.parse(fakeFixtureJson("matrix", 7));
    const code = SORTED_CODES[0];
    parsed.report.runs[0].stocks[code].trade_event_volume = "999";
    assert.throws(
      () => validateFixtureOutput("matrix", 7, TRADING_DAYS, parsed),
      /trade_event_volume/,
    );
  });

  it("拒绝非十进制字符串的 engine_error_events", () => {
    assert.throws(
      () =>
        validateFixtureOutput(
          "matrix",
          7,
          TRADING_DAYS,
          JSON.parse(withRun0Field("matrix", 7, "engine_error_events", 3)),
        ),
      /engine_error_events/,
    );
    assert.throws(
      () =>
        validateFixtureOutput(
          "matrix",
          7,
          TRADING_DAYS,
          JSON.parse(withRun0Field("matrix", 7, "engine_error_events", "oops")),
        ),
      /engine_error_events/,
    );
  });

  it("非零 engine_error_events 是当前行为的合法观测：如实采集并在日志与 manifest 显式呈现", async () => {
    const outputDir = await newTempDir();
    const logs = [];
    const manifest = await captureBaseline({
      outputDir: path.join(outputDir, "b"),
      exec: fakeExec((scenarioName, seed) =>
        withRun0Field(scenarioName, seed, "engine_error_events", "5"),
      ),
      repoRoot: "D:/repo",
      log: (message) => logs.push(message),
    });
    for (const scenarioName of Object.keys(SCENARIOS)) {
      const flagged = manifest.scenarios[scenarioName].runs.filter(
        (run) => run.engine_error_events !== "0",
      );
      assert.equal(flagged.length, MATRIX_SEEDS.length, "每个 run 记录都必须携带该指标");
      assert.ok(flagged.every((run) => run.engine_error_events === "5"));
    }
    assert.ok(
      logs.some((message) => /engine_error_events=5/.test(message)),
      "非零错误事件必须出现在日志中，绝不静默",
    );
  });

  it("拒绝缺失某只股票的极端样本", () => {
    const parsed = JSON.parse(fakeFixtureJson("matrix", 7));
    parsed.report.extreme_cases = parsed.report.extreme_cases.slice(0, 4);
    assert.throws(() => validateFixtureOutput("matrix", 7, TRADING_DAYS, parsed), /extreme_cases/);
  });

  it("拒绝非十进制字符串的 u64 量纲字段并点名场景与 seed", () => {
    const parsed = JSON.parse(fakeFixtureJson("matrix", 7));
    const code = SORTED_CODES[0];
    parsed.report.runs[0].stocks[code].auction_volume = 100;
    assert.throws(
      () => validateFixtureOutput("matrix", 7, TRADING_DAYS, parsed),
      (error) => {
        assert.match(error.message, /auction_volume/);
        assert.match(error.message, /matrix/);
        assert.match(error.message, /seed 7/);
        return true;
      },
    );
  });

  it("拒绝双边参与量与成交量两倍不一致", () => {
    const parsed = JSON.parse(fakeFixtureJson("matrix", 7));
    parsed.report.runs[0].participant_execution.two_sided_participant_shares = "7";
    assert.throws(
      () => validateFixtureOutput("matrix", 7, TRADING_DAYS, parsed),
      /two_sided_participant_shares/,
    );
  });
});

describe("captureBaseline 端到端（注入 exec）", () => {
  it("happy path：逐 seed 写盘、写 manifest、记录对账摘要并复核确定性", async () => {
    const outputDir = await newTempDir();
    const target = path.join(outputDir, "before");
    const manifest = await captureBaseline({
      outputDir: target,
      exec: fakeExec(),
      repoRoot: "D:/workplace/stock_market_game",
    });

    assert.equal(manifest.git.revision, "6ad461e7f735ee1a0b4090497c58380af3422761");
    assert.deepEqual(manifest.git.dirty_paths, ["?? .omo/"]);
    assert.equal(manifest.toolchain.node.version, "v24.18.0");
    assert.equal(manifest.toolchain.cargo.version, "cargo 1.96.1");
    assert.equal(manifest.toolchain.pnpm.available, false, "Corepack pnpm 不可用必须显式记录");
    assert.ok(manifest.toolchain.pnpm.error.length > 0);

    for (const scenarioName of Object.keys(SCENARIOS)) {
      const files = (await readdir(path.join(target, scenarioName))).sort();
      const expected = MATRIX_SEEDS.map((seed) => `seed-${seed}.json`).sort();
      assert.deepEqual(files, expected);
      const scenario = manifest.scenarios[scenarioName];
      assert.equal(scenario.runs.length, MATRIX_SEEDS.length);
      assert.ok(scenario.runs.every((run) => run.exit_code === 0));
      assert.ok(scenario.runs.every((run) => run.sha256.length === 64));
      assert.equal(scenario.determinism_check.identical, true);
      assert.equal(
        scenario.determinism_check.rerun_digest,
        scenario.runs.find((run) => run.seed === MATRIX_SEEDS[0]).sha256,
      );
      const firstSeedRaw = JSON.parse(
        await readFile(path.join(target, scenarioName, `seed-${MATRIX_SEEDS[0]}.json`), "utf8"),
      );
      assert.equal(firstSeedRaw.seed, String(MATRIX_SEEDS[0]));
      assert.equal(firstSeedRaw.report.runs.length, 1, "单 seed 报告不得合并其他 seed");
    }
    const manifestOnDisk = JSON.parse(await readFile(path.join(target, "manifest.json"), "utf8"));
    assert.equal(manifestOnDisk.command, "before");
  });

  it("子进程非零退出码必须显式上报，绝不静默吞掉", async () => {
    const outputDir = await newTempDir();
    const target = path.join(outputDir, "before");
    const exec = fakeExec((scenarioName, seed) => {
      if (seed === 11) {
        return null; // 占位，下方用自定义分支处理
      }
      return undefined;
    });
    const failingExec = async (file, args) => {
      if (file === "cargo" && args.includes("baseline_fixture") && args.includes("11")) {
        return { code: 2, stdout: "", stderr: "量价基线生成失败：seed 11 演示失败\n" };
      }
      return exec(file, args);
    };
    await assert.rejects(
      captureBaseline({ outputDir: target, exec: failingExec, repoRoot: "D:/repo" }),
      (error) => {
        assert.match(error.message, /seed 11/);
        assert.match(error.message, /退出码 2/);
        assert.match(error.message, /baseline_fixture/);
        return true;
      },
    );
    await assert.rejects(
      readFile(path.join(target, "manifest.json"), "utf8"),
      /ENOENT/,
      "失败路径不得留下成功 manifest",
    );
  });

  it("子进程成功但未输出报告时拒绝", async () => {
    const outputDir = await newTempDir();
    const exec = fakeExec(() => "");
    await assert.rejects(
      captureBaseline({ outputDir: path.join(outputDir, "b"), exec, repoRoot: "D:/repo" }),
      /未输出报告/,
    );
  });

  it("报告与请求配置不符时拒绝并点名字段", async () => {
    const outputDir = await newTempDir();
    const exec = fakeExec((scenarioName, seed) => {
      const parsed = JSON.parse(fakeFixtureJson(scenarioName, seed));
      if (seed === MATRIX_SEEDS[0]) {
        parsed.config.retail_count = 19999;
      }
      return JSON.stringify(parsed);
    });
    await assert.rejects(
      captureBaseline({ outputDir: path.join(outputDir, "b"), exec, repoRoot: "D:/repo" }),
      /retail_count/,
    );
  });

  it("同 seed 两次运行摘要不一致时拒绝（确定性破坏）", async () => {
    const outputDir = await newTempDir();
    const seen = new Map();
    const exec = fakeExec((scenarioName, seed) => {
      const key = `${scenarioName}:${seed}`;
      const calls = seen.get(key) ?? 0;
      seen.set(key, calls + 1);
      const parsed = JSON.parse(fakeFixtureJson(scenarioName, seed));
      parsed.report.runs[0].trade_events = String(50 + calls); // 第二次运行内容漂移
      return JSON.stringify(parsed);
    });
    await assert.rejects(
      captureBaseline({ outputDir: path.join(outputDir, "b"), exec, repoRoot: "D:/repo" }),
      /摘要不一致|digest/,
    );
  });

  it("输出目录已存在且非空时拒绝，避免混入旧证据", async () => {
    const outputDir = await newTempDir();
    const target = path.join(outputDir, "b");
    await captureBaseline({ outputDir: target, exec: fakeExec(), repoRoot: "D:/repo" });
    await assert.rejects(
      captureBaseline({ outputDir: target, exec: fakeExec(), repoRoot: "D:/repo" }),
      /非空/,
    );
  });
});
