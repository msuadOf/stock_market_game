import assert from "node:assert/strict";

import { it } from "node:test";

import path from "node:path";

import { buildRustTestExecutionPolicy, executeRustTestBinaries, FULL_REGRESSION_SOURCE_INPUTS, fullRegressionSteps, parseRustTestCases, runFullRegression } from "./run-full-regression.mjs";

import { runBoundedCommand } from "./run-with-deadline.mjs";
import * as regression from "./run-full-regression.mjs";

import { CARGO_BUILD_JOBS_ENV } from "./full-regression-test-support.mjs";

it("根回归纳入完整 scripts 发现入口及其源码指纹", { timeout: 10000 }, () => {
  const steps = fullRegressionSteps(4, process.cwd(), {});
  assert.ok(steps.some((step) => step.kind === "script-test-batch"));
  assert.ok(FULL_REGRESSION_SOURCE_INPUTS.includes("scripts"));
});

it("Cargo 独立 jobs 只改变 build/doctest，保留总 CPU 与其他 step 契约", () => {
  const repoRoot = process.cwd();
  const baseEnv = { CARGO_BUILD_JOBS: "99", RUSTFLAGS: "fixture-rustflags", CARGO_PROFILE_TEST_DEBUG: "2" };
  const original = fullRegressionSteps(128, repoRoot, baseEnv);
  const configured = fullRegressionSteps(128, repoRoot, { ...baseEnv, [CARGO_BUILD_JOBS_ENV]: "8" });
  for (const [index, step] of configured.entries()) {
    const { env: originalEnv, ...originalStep } = original[index];
    const { env: configuredEnv, ...configuredStep } = step;
    assert.deepEqual(configuredStep, originalStep);
    const expectedJobs = ["rust-test-build", "rust-doctests"].includes(step.kind) ? "8" : "99";
    assert.equal(configuredEnv.CARGO_BUILD_JOBS, expectedJobs);
    const { [CARGO_BUILD_JOBS_ENV]: ignored, ...withoutConfiguration } = configuredEnv;
    assert.deepEqual(withoutConfiguration, { ...originalEnv, CARGO_BUILD_JOBS: expectedJobs });
  }
  assert.equal(configured[1].cpuCount, 128);
  assert.equal(configured[4].cpuCount, 128);
  assert.deepEqual(configured[2].args, ["test", "--workspace", "--doc", "--no-fail-fast", "--", "--test-threads=127"]);
  for (const [cpuCount, value, jobs] of [[128, undefined, "128"], [128, "128", "128"], [1, "1", "1"]]) {
    const env = value === undefined ? {} : { [CARGO_BUILD_JOBS_ENV]: value };
    const steps = fullRegressionSteps(cpuCount, repoRoot, env);
    assert.equal(steps[0].env.CARGO_BUILD_JOBS, jobs);
    assert.equal(steps[2].env.CARGO_BUILD_JOBS, jobs);
  }
});

it("Cargo 独立 jobs 拒绝非 canonical 字符串及超过可用 CPU 的值", () => {
  const invalid = ["", "0", "-1", "01", "00", "+8", " 8", "8 ", "8\n", "8\r\n", "8\t", "1.5", "8e0", "0x8", "８", "八", "invalid", "9007199254740992", "129", 8, null, {}, true];
  for (const value of invalid) {
    assert.throws(() => fullRegressionSteps(128, process.cwd(), { [CARGO_BUILD_JOBS_ENV]: value }),
      (error) => error.message.includes(CARGO_BUILD_JOBS_ENV) && error.message.includes("128"),
      `非法配置必须显式拒绝：${JSON.stringify(value)}`);
  }
});

it("all 的两个内部 phase 继承专用配置并由 Cargo steps 消费", { timeout: 10000 }, async () => {
  const runnerUrl = new URL("./run-full-regression.mjs", import.meta.url).href;
  const result = await runBoundedCommand({
    command: process.execPath,
    args: ["--input-type=module", "-e", `
      import assert from "node:assert/strict";
      import { runFullRegression, fullRegressionSteps } from ${JSON.stringify(runnerUrl)};
      const phases = [];
      await runFullRegression({ run: async ({ args, env }) => {
        assert.equal(env.${CARGO_BUILD_JOBS_ENV}, "8");
        const steps = fullRegressionSteps(128, process.cwd(), env);
        assert.equal(steps[0].env.CARGO_BUILD_JOBS, "8");
        assert.equal(steps[2].env.CARGO_BUILD_JOBS, "8");
        phases.push(args[1]);
      } });
      assert.deepEqual(phases, ["--internal-build", "--internal-execute"]);
      console.log("两个 phase 已继承并消费 jobs=8");
    `],
    env: { ...process.env, [CARGO_BUILD_JOBS_ENV]: "8" },
    timeoutMs: 1000,
    captureOutput: true,
  });
  assert.match(result.stdout, /两个 phase 已继承并消费 jobs=8/);
});

it("Rust case discovery rejects malformed output and excludes benchmarks", { timeout: 10000 }, () => {
  assert.deepEqual(parseRustTestCases("suite::case: test\nbench: benchmark\n1 test, 1 benchmark\n"), ["suite::case"]);
  assert.throws(() => parseRustTestCases(undefined), /did not return output/);
  assert.throws(() => parseRustTestCases("unexpected output"), /malformed/);
  assert.throws(() => parseRustTestCases("same: test\nsame: test\n"), /duplicate/);
});

it("Rust ordinary cases in one binary run concurrently with independent deadlines and ignored cases excluded", { timeout: 10000 }, async () => {
  const calls = [];
  let active = 0;
  let peak = 0;
  const result = await executeRustTestBinaries({
    artifacts: [{ executable: "fixture", label: "fixture" }],
    policy: buildRustTestExecutionPolicy(8, 1), cwd: process.cwd(), env: {}, now: Date.now,
    remainingMs: () => 2500,
    run: async (options) => {
      calls.push(options);
      if (options.args.includes("--list")) return { stdout: options.args.includes("--ignored") ? "long: test\n" : "short-a: test\nshort-b: test\nlong: test\n" };
      active += 1;
      peak = Math.max(peak, active);
      await new Promise((resolve) => setTimeout(resolve, 10));
      active -= 1;
    },
  });
  assert.equal(peak, 2);
  assert.equal(result.case_count, 2);
  assert.equal(result.max_concurrent_cases, 2);
  assert.deepEqual(calls.filter((call) => !call.args.includes("--list")).map((call) => call.args[0]), ["short-a", "short-b"]);
  assert.ok(calls.every((call) => call.timeoutMs === 2500));
});

it("真实 actor 成交短时限 case 独立运行后再并发其他 case", async () => {
  const actor = "actor_market_goes_live_produces_trade_events";
  const calls = [];
  let actorDone = false;
  let active = 0;
  let peak = 0;
  await executeRustTestBinaries({
    artifacts: [{ executable: "actor-fixture", label: "path+file:///fixture/apps/server#server@0.1.0 test:actor" }],
    policy: buildRustTestExecutionPolicy(8, 1), cwd: process.cwd(), env: {}, now: Date.now, remainingMs: () => 10000,
    run: async (options) => {
      if (options.args.includes("--list")) return { stdout: options.args.includes("--ignored") ? "" : `other: test\nother2: test\n${actor}: test\n` };
      calls.push(options.args[0]);
      if (options.args[0] === actor) { await new Promise((resolve) => setTimeout(resolve, 10)); actorDone = true; }
      else {
        assert.equal(actorDone, true, "actor case 完成前不得启动其他 case");
        active += 1; peak = Math.max(peak, active);
        await new Promise((resolve) => setTimeout(resolve, 10));
        active -= 1;
      }
      assert.equal(options.timeoutMs, 10000);
    },
  });
  assert.deepEqual(calls, [actor, "other", "other2"]);
  assert.equal(peak, 2);
});

it("合并报表长 fixture 在其他 case 前独立运行", async () => {
  const heavy = "mixed_group_closes_then_publishes_distinct_consolidated_public_report";
  const calls = [];
  let finished = false;
  await executeRustTestBinaries({
    artifacts: [{ executable: "fixture", label: "path+file:///fixture/packages/engine#0.1.0 test:session_company_assembly" }],
    policy: buildRustTestExecutionPolicy(8, 1), cwd: process.cwd(), env: {}, now: Date.now, remainingMs: () => 10000,
    run: async (options) => {
      if (options.args.includes("--list")) return { stdout: options.args.includes("--ignored") ? "" : `other: test\n${heavy}: test\n` };
      calls.push(options.args[0]);
      if (options.args[0] === heavy) { await new Promise((resolve) => setTimeout(resolve, 10)); finished = true; }
      else assert.equal(finished, true);
      assert.equal(options.timeoutMs, 10000);
    },
  });
  assert.deepEqual(calls, [heavy, "other"]);
});

it("actor 独立 case 失败时停止其他测试并保留剩余期限", async () => {
  const calls = [];
  await assert.rejects(executeRustTestBinaries({
    artifacts: [{ executable: "actor-fixture", label: "path+file:///fixture/apps/server#server@0.1.0 test:actor" }],
    policy: buildRustTestExecutionPolicy(8, 1), cwd: process.cwd(), env: {}, now: Date.now, remainingMs: () => 3000,
    run: async (options) => {
      if (options.args.includes("--list")) return { stdout: options.args.includes("--ignored") ? "" : "other: test\nactor_market_goes_live_produces_trade_events: test\n" };
      calls.push(options.args[0]);
      assert.equal(options.timeoutMs, 3000);
      throw new Error("actor fixture failed");
    },
  }), /actor fixture failed/);
  assert.deepEqual(calls, ["actor_market_goes_live_produces_trade_events"]);
});

it("日历重 case 串行且其余 case 保持并发，全部仍有独立期限", async () => {
  const heavy = ["session::calendar_nonzero_plan_parity_tests::a", "session::calendar_final_boundary_tests::b", "session::pipeline::authoritative_tick_tests::extreme_phase_geometry_single_tick_day", "session::pipeline::authoritative_tick_tests::extreme_phase_geometry_no_closing_auction", "session::pipeline::authoritative_tick_tests::extreme_phase_geometry_closing_auction"];
  const ordinary = ["ordinary::a", "ordinary::b"];
  let heavyActive = 0;
  let peakHeavy = 0;
  let overlap = false;
  const seen = [];
  await executeRustTestBinaries({
    artifacts: [{ executable: "fixture", label: "fixture" }],
    policy: buildRustTestExecutionPolicy(8, 1), cwd: process.cwd(), env: {}, now: Date.now,
    remainingMs: () => 10000,
    run: async (options) => {
      if (options.args.includes("--list")) return { stdout: options.args.includes("--ignored") ? "" : [...heavy, ...ordinary].map((name) => `${name}: test\n`).join("") };
      const name = options.args[0];
      seen.push(name);
      assert.equal(options.timeoutMs, 10000);
      if (heavy.includes(name)) { heavyActive += 1; peakHeavy = Math.max(peakHeavy, heavyActive); }
      else if (heavyActive) overlap = true;
      await new Promise((resolve) => setTimeout(resolve, 20));
      if (heavy.includes(name)) heavyActive -= 1;
    },
  });
  assert.deepEqual(seen.sort(), [...heavy, ...ordinary].sort());
  assert.equal(peakHeavy, 1);
  assert.equal(overlap, true);
});

it("日历队列单核覆盖全部 case，失败时取消普通队列", async () => {
  const heavy = "session::calendar_nonzero_plan_parity_tests::a";
  const ordinary = "ordinary::a";
  const seen = [];
  const list = (options) => options.args.includes("--ignored") ? "" : `${heavy}: test\n${ordinary}: test\n`;
  await executeRustTestBinaries({
    artifacts: [{ executable: "fixture", label: "fixture" }], policy: buildRustTestExecutionPolicy(1, 1),
    cwd: process.cwd(), env: {}, now: Date.now, remainingMs: () => 10000,
    run: async (options) => { if (options.args.includes("--list")) return { stdout: list(options) }; seen.push(options.args[0]); },
  });
  assert.deepEqual(seen.sort(), [heavy, ordinary].sort());
  let ready;
  const started = new Promise((resolve) => { ready = resolve; });
  let cancelled = false;
  await assert.rejects(executeRustTestBinaries({
    artifacts: [{ executable: "fixture", label: "fixture" }], policy: buildRustTestExecutionPolicy(4, 1),
    cwd: process.cwd(), env: {}, now: Date.now, remainingMs: () => 10000,
    run: async (options) => {
      if (options.args.includes("--list")) return { stdout: list(options) };
      if (options.args[0] === heavy) { await started; throw new Error("日历失败"); }
      ready();
      await new Promise((resolve, reject) => options.signal.addEventListener("abort", () => { cancelled = true; reject(options.signal.reason); }, { once: true }));
    },
  }), /日历失败/);
  assert.equal(cancelled, true);
});

it("Rust ordinary case watchdog terminates a hanging process and reports the exact case", { timeout: 10000 }, async () => {
  const startedAt = Date.now();
  await assert.rejects(executeRustTestBinaries({
    artifacts: [{ executable: "fixture", label: "fixture-label" }],
    policy: buildRustTestExecutionPolicy(4, 1), cwd: process.cwd(), env: process.env, now: Date.now,
    remainingMs: () => 400,
    run: async (options) => {
      assert.equal(options.timeoutMs, 400);
      if (options.args.includes("--list")) return { stdout: options.args.includes("--ignored") ? "" : "hanging::case: test\n" };
      return runBoundedCommand({ ...options, command: process.execPath, args: ["-e", "setInterval(() => {}, 1000)"], cleanupReserveMs: 100 });
    },
  }), /fixture-label case hanging::case failed:.*400ms deadline/);
  assert.ok(Date.now() - startedAt < 1500);
});

it("Rust ordinary case failure aborts active sibling cases and never launches queued cases", { timeout: 10000 }, async () => {
  let siblingStarted;
  const started = new Promise((resolve) => { siblingStarted = resolve; });
  const executed = [];
  let aborted = false;
  await assert.rejects(executeRustTestBinaries({
    artifacts: [{ executable: "fixture", label: "fixture" }],
    policy: buildRustTestExecutionPolicy(4, 1), cwd: process.cwd(), env: {}, now: Date.now,
    remainingMs: () => 300000,
    run: async ({ args, signal, timeoutMs }) => {
      assert.equal(timeoutMs, 10000);
      if (args.includes("--list")) return { stdout: args.includes("--ignored") ? "" : "fail: test\nsibling: test\nqueued: test\n" };
      executed.push(args[0]);
      if (args[0] === "fail") {
        await started;
        throw new Error("injected case failure");
      }
      siblingStarted();
      await new Promise((resolve, reject) => signal.addEventListener("abort", () => {
        aborted = true;
        reject(signal.reason);
      }, { once: true }));
    },
  }), /fixture case fail failed: injected case failure/);
  assert.equal(aborted, true);
  assert.deepEqual(executed, ["fail", "sibling"]);
});

it("Rust case listing fails explicitly instead of skipping malformed or unknown ignored cases", { timeout: 10000 }, async () => {
  for (const ignoredOutput of ["bad output", "unknown: test\n"]) {
    let executions = 0;
    await assert.rejects(executeRustTestBinaries({
      artifacts: [{ executable: "fixture", label: "fixture" }],
      policy: buildRustTestExecutionPolicy(8, 1), cwd: process.cwd(), env: {}, now: Date.now,
      remainingMs: () => 300000,
      run: async ({ args }) => {
        if (!args.includes("--list")) executions += 1;
        return { stdout: args.includes("--ignored") ? ignoredOutput : "ordinary: test\n" };
      },
    }), /malformed Rust test listing|contains an unknown case/);
    assert.equal(executions, 0);
  }
});
