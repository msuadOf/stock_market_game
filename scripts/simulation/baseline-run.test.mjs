import assert from "node:assert/strict";
import { mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { after, describe, it } from "node:test";

import {
  MATRIX_SEEDS,
  SCENARIOS,
  TRADING_DAYS,
  buildExampleArgs,
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
      return { code: 0, stdout: "6ad461e7f735ee1a0b4090497c58380af3422761\n", stderr: "" };
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
    if (file === "pnpm" || file === "corepack") {
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
    assert.throws(() => parseCliArgs(["after", "--output", "x"]), /after/);
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
    assert.equal(manifest.toolchain.pnpm.available, false, "pnpm 不可用必须显式记录");
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
