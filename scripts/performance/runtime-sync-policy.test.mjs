import assert from "node:assert/strict";
import test from "node:test";
import fs from "node:fs/promises";
import path from "node:path";
import { PassThrough } from "node:stream";
import { fileURLToPath } from "node:url";
import {
  runRuntimeSyncPolicy,
  validateRuntimeSyncConfig,
  validateRuntimeSyncSample,
  aggregateRuntimeSyncPolicy,
} from "./runtime-sync-policy.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
async function makeTempRoot(prefix) {
  const root = path.join(repoRoot, ".tmp");
  await fs.mkdir(root, { recursive: true });
  return fs.mkdtemp(path.join(root, prefix));
}

const baseConfig = () => ({
  schema: "runtime-sync-policy-config",
  schema_version: 1,
  binary: "/tmp/engine-probe",
  cwd: "/tmp",
  setup_file: "/tmp/setup.json",
  seed: "42",
  trading_days: 2,
  rounds: 5,
  warmup_rounds: 1,
  cpu_affinity: [0, 1],
  sample_interval_ms: 5,
  deadline_ms: 300000,
  source_revision: "a".repeat(40),
});

const setup = {
  ticks_per_day: 3,
  trading_days: 2,
  runtime_sync_policy: "every_tick",
  npc_count: 2,
  stocks: ["AAA"],
  npcs: { retail_count: 1, inst_count: 1, hot_count: 0 },
};

function probe(policy, patch = {}) {
  const origin = (source_tick, install_sequence, stage = "retained_published") => ({ authority_epoch: 1, install_sequence, source_tick, source_generation: 1, day_end_stage: stage, non_tick_revision: 0, mirror_phase: "Continuous" });
  const boundaries = [
    { settled_date: "2026-10-29", next_date: "2026-10-30", tick: 3, combined_boundary_wall_ns: "20", save_candidate_tick: 3, save_candidate_seq: 9, mirror_origin: origin(3, 9) },
    { settled_date: "2026-10-30", next_date: "2026-10-31", tick: 6, combined_boundary_wall_ns: "30", save_candidate_tick: 6, save_candidate_seq: 18, mirror_origin: origin(6, 18) },
  ];
  return {
    schema: "runtime-sync-policy-sample",
    schema_version: 1,
    policy,
    seed: "42",
    setup: { ...setup, runtime_sync_policy: policy },
    trading_days: 2,
    completed_trading_days: 2,
    completed_ticks: 7,
    expected_ticks: 7,
    last_tick: 7,
    civil_updates: 4,
    trading_day_boundaries: boundaries,
    closed_civil_days: 2,
    closed_civil_wall_ns: "40",
    run_wall_ns: "1000",
    intraday_wall_ns: "900",
    boundary_wall_ns: ["20", "30"],
    initialization_wall_ns: "50",
    final_civil_date: "2026-11-02",
    final_phase: "intraday_trading",
    next_day_first_tick_confirmed: true,
    final_seq: 21,
    final_mirror_origin: origin(policy === "every_tick" ? 7 : 6, policy === "every_tick" ? 21 : 18, policy === "every_tick" ? "tick" : "retained_published"),
    events: 10,
    pure_flush_wall_ns: null,
    pure_flush_availability: "unavailable",
    shutdown_confirmed: true,
    shutdown: { runtime: { owned_workers: 2, joined_workers: 2, group_finals: 1, desk_finals: 1, worker_results: [{ worker: "group", result: { Ok: null } }, { worker: "desk", result: { Ok: null } }] } },
    topology: { groups: 1, desks: 1 },
    stocks: 1,
    npc_count: 2,
    ...patch,
  };
}

function measured(sample) {
  return {
    wall_ns: sample.run_wall_ns,
    peak_process_tree_rss_bytes: 4096,
    process_tree_thread_state: {
      schema: "linux-process-tree-thread-state",
      schema_version: 1,
      sampled_state: "R (running or runnable)",
      sample_interval_ms: 5,
      sample_count: 2,
      process_count: { minimum: 1, maximum: 1 },
      total_threads: { minimum: 2, maximum: 2 },
      runnable_threads: { minimum: 1, maximum: 2, mean: 1.5, histogram: { "1": 1, "2": 1 } },
    },
    stdout: JSON.stringify(sample),
    stderr: "",
  };
}

function dependencies({ driftAt, samplePatch, failAt, load = [1, 2, 3] } = {}) {
  let now = 0;
  let index = 0;
  const calls = [];
  const files = new Map([
    ["/tmp/setup.json", JSON.stringify(setup)],
    ["/tmp/engine-probe", "binary contents"],
  ]);
  const hooks = {
    calls,
    files,
    identity: async (file) => `identity:${files.get(file)}`,
    readFile: async (file) => files.get(file),
    writes: [],
    clockValue: 0,
    writeFile: async (file, data, options) => { hooks.writes.push({ file, data, options }); files.set(file, data); if (hooks.advanceOnPartial && file.endsWith(".partial")) hooks.clockValue = 4000; },
    renameFile: async (from, to) => { hooks.renames ??= []; hooks.renames.push({ from, to }); },
    unlinkFile: async (file) => { hooks.unlinks ??= []; hooks.unlinks.push(file); },
    advanceOnPartial: false,
    load: () => [...load],
    clock: () => hooks.clockValue + now,
    sleep: async (ms) => { now += ms; },
    runSample: async (endpoint, options) => {
      calls.push({ endpoint, options });
      const current = index++;
      if (failAt === current) throw new Error("injected sample failure");
      if (driftAt === current) files.set("/tmp/engine-probe", "changed binary");
      const policy = endpoint.command.at(-3);
      const sample = probe(policy, current === samplePatch?.at ? samplePatch.patch : {});
      return measured(sample);
    },
    terminate: async () => { hooks.terminated = true; },
  };
  return hooks;
}

test("配置固定双策略 ABBA 设计并拒绝过少轮数", { timeout: 10_000, concurrency: true }, () => {
  const config = validateRuntimeSyncConfig(baseConfig());
  assert.equal(config.rounds, 5);
  assert.throws(() => validateRuntimeSyncConfig({ ...baseConfig(), warmup_rounds: 2 }), /warmup_rounds/);
  const defaults = { ...baseConfig() };
  delete defaults.rounds; delete defaults.warmup_rounds; delete defaults.sample_interval_ms; delete defaults.deadline_ms;
  assert.deepEqual([validateRuntimeSyncConfig(defaults).rounds, validateRuntimeSyncConfig(defaults).warmup_rounds, validateRuntimeSyncConfig(defaults).sample_interval_ms, validateRuntimeSyncConfig(defaults).deadline_ms], [5, 1, 5, 300000]);
  assert.throws(() => validateRuntimeSyncConfig({ ...baseConfig(), rounds: 4 }), /rounds/);
  assert.throws(() => validateRuntimeSyncConfig({ ...baseConfig(), deadline_ms: 300001 }), /deadline/);
});

test("sample 校验要求真实样本、正 RSS/runnable 和未测 pure flush", { timeout: 10_000, concurrency: true }, () => {
  const config = { ...validateRuntimeSyncConfig(baseConfig()), setup_json: JSON.stringify(setup) };
  const valid = probe("every_tick");
  assert.equal(validateRuntimeSyncSample(measured(valid), config, "every_tick", "sample").sample.policy, "every_tick");
  assert.throws(() => validateRuntimeSyncSample({ ...measured(valid), peak_process_tree_rss_bytes: 0 }, config, "every_tick", "sample"), /RSS/);
  assert.throws(() => validateRuntimeSyncSample(measured(probe("every_tick", { pure_flush_wall_ns: "0", pure_flush_availability: "available" })), config, "every_tick", "sample"), /pure_flush/);
  assert.throws(() => validateRuntimeSyncSample(measured(probe("every_tick", { completed_ticks: 6 })), config, "every_tick", "sample"), /tick/);
  assert.throws(() => validateRuntimeSyncSample(measured(probe("every_tick", { trading_day_boundaries: [] })), config, "every_tick", "sample"), /boundary/);
  const zeroRunnable = measured(probe("every_tick"));
  zeroRunnable.process_tree_thread_state.runnable_threads = { minimum: 0, maximum: 2, mean: 1, histogram: { "0": 1, "2": 1 } };
  assert.equal(validateRuntimeSyncSample(zeroRunnable, config, "every_tick", "sample").sample.policy, "every_tick");
});

test("单批按 warmup 后一轮 ABBA 独立启动六个样本并保留批次窗口", { timeout: 10_000, concurrency: true }, async () => {
  const hooks = dependencies();
  const report = await runRuntimeSyncPolicy(baseConfig(), { ...hooks, batchIndex: 3 });
  assert.deepEqual(hooks.calls.map(({ endpoint }) => endpoint.command.at(-3)), [
    "every_tick", "day_end", "every_tick", "day_end", "day_end", "every_tick",
  ]);
  assert.equal(report.status, "PASS_BATCH");
  assert.equal(report.batch_index, 3);
  assert.deepEqual(report.planned_batches, [1, 2, 3, 4, 5]);
  assert.equal(report.warmup_sample_count, 2);
  assert.equal(report.sample_count, 4);
  assert.equal(report.round_count, 1);
  assert.equal(report.samples.length, 6);
  assert.equal(report.samples[0].peak_process_tree_rss_bytes, 4096);
  assert.equal(report.pure_flush_wall_ns, null);
  assert.equal(report.pure_flush_availability, "unavailable");
  assert.equal(report.combined_boundary_wall_ns.per_policy.every_tick.p95, "30");
  assert.equal(report.policy_summary.every_tick.combined_boundary_wall_ns.sample_count, 4);
  assert.equal(report.policy_summary.every_tick.combined_boundary_wall_ns.percentile_method, "nearest-rank");
  assert.equal(report.policy_summary.every_tick.combined_boundary_wall_ns.p95, "30");
  assert.equal(report.policy_summary.every_tick.runnable_threads.maximum, 1.5);
  assert.equal(report.contract_metrics_complete, false);
  assert.ok(report.measurement_gaps.some((gap) => gap.includes("纯自动 flush")));
  assert.deepEqual(report.cpu_load, { before: [1, 2, 3], after: [1, 2, 3] });
  assert.match(report.interference_declaration, /CPU 百分比/);
  assert.equal(hooks.calls.every(({ endpoint, options }) =>
    endpoint.command[0] === "taskset" && endpoint.command[1] === "-c"
      && endpoint.command[2] === "0,1" && options.rssSampleIntervalMs === 5), true);
});

test("零 intraday wall 明确报告吞吐 unavailable", { timeout: 10_000, concurrency: true }, async () => {
  const report = await runRuntimeSyncPolicy(baseConfig(), {
    ...dependencies({ samplePatch: { at: 2, patch: { intraday_wall_ns: "0" } } }),
    batchIndex: 1,
  });
  assert.deepEqual(report.policy_summary.every_tick.intraday_steps_per_second, { availability: "unavailable", value: null });
});

test("双臂 setup 除 policy 外必须一致，错误策略及跨日/线程缺失被拒绝", { timeout: 10_000, concurrency: true }, () => {
  const config = { ...validateRuntimeSyncConfig(baseConfig()), setup_json: JSON.stringify(setup) };
  assert.throws(() => validateRuntimeSyncSample(measured(probe("on_demand")), config, "every_tick", "sample"), /policy/);
  assert.throws(() => validateRuntimeSyncSample(measured(probe("every_tick", {
    setup: { ...setup, runtime_sync_policy: "every_tick", npc_count: 3 },
  })), config, "every_tick", "sample"), /setup/);
  assert.throws(() => validateRuntimeSyncSample(measured(probe("every_tick", {
    trading_day_boundaries: [probe("every_tick").trading_day_boundaries[0]],
  })), config, "every_tick", "sample"), /boundary/);
  const noThreads = measured(probe("every_tick"));
  noThreads.process_tree_thread_state = undefined;
  assert.throws(() => validateRuntimeSyncSample(noThreads, config, "every_tick", "sample"), /thread/);
  const noRunnable = measured(probe("every_tick"));
  noRunnable.process_tree_thread_state.runnable_threads = { minimum: 0, maximum: 0, mean: 0, histogram: { "0": 2 } };
  assert.equal(validateRuntimeSyncSample(noRunnable, config, "every_tick", "sample").sample.policy, "every_tick");
  assert.throws(() => validateRuntimeSyncSample(measured(probe("every_tick", { civil_updates: 5 })), config, "every_tick", "sample"), /civil update/);
  const oneDaySample = probe("every_tick", {
    trading_days: 1, completed_trading_days: 1, completed_ticks: 4, expected_ticks: 4, last_tick: 4,
    civil_updates: 1, closed_civil_days: 0, trading_day_boundaries: probe("every_tick").trading_day_boundaries.slice(0, 1),
    boundary_wall_ns: ["20"], final_civil_date: "2026-10-30", final_mirror_origin: { ...probe("every_tick").final_mirror_origin, source_tick: 4 },
  });
  const oneDayConfig = { ...config, trading_days: 1, setup_json: JSON.stringify(setup) };
  assert.equal(validateRuntimeSyncSample(measured(oneDaySample), oneDayConfig, "every_tick", "sample").sample.policy, "every_tick");
  assert.throws(() => validateRuntimeSyncSample(measured(probe("every_tick", { shutdown: { runtime: { owned_workers: 2 } } })), config, "every_tick", "sample"), /shutdown/);
});

test("单批 binary 漂移或样本失败均拒绝 PASS_BATCH", { timeout: 10_000, concurrency: true }, async () => {
  const drift = dependencies({ driftAt: 3 });
  await assert.rejects(runRuntimeSyncPolicy(baseConfig(), { ...drift, batchIndex: 1 }), /binary.*changed|identity/);
  assert.equal(drift.calls.length, 6);

  const failure = dependencies({ failAt: 2 });
  await assert.rejects(runRuntimeSyncPolicy(baseConfig(), { ...failure, batchIndex: 1 }), /injected sample failure/);
  assert.equal(failure.calls.length, 3);

  const terminationFailure = dependencies({ failAt: 0 });
  let fireTerminationFailureDeadline;
  terminationFailure.setDeadlineTimer = (callback) => { fireTerminationFailureDeadline = callback; return 1; };
  terminationFailure.clearDeadlineTimer = () => {};
  terminationFailure.runSample = (_endpoint, { signal, spawn }) => new Promise((_resolve, reject) => {
    spawn("fake", [], {});
    signal.addEventListener("abort", () => reject(new Error("sample primary failure")), { once: true });
    setImmediate(() => fireTerminationFailureDeadline());
  });
  terminationFailure.spawnProcess = () => ({});
  terminationFailure.terminate = async () => { throw new Error("tree termination failure"); };
  await assert.rejects(runRuntimeSyncPolicy(baseConfig(), { ...terminationFailure, batchIndex: 1 }), (error) => {
    assert.ok(error instanceof AggregateError);
    assert.match(error.message, /sample primary failure/);
    assert.match(error.message, /tree termination failure/);
    return true;
  });
});

test("共享 deadline 取消当前 sample 并等待进程树清理", { timeout: 10_000, concurrency: true }, async () => {
  const hooks = dependencies();
  let fireDeadline;
  hooks.runSample = (_endpoint, { signal, spawn }) => new Promise((resolve, reject) => {
    spawn("fake", [], {});
    signal.addEventListener("abort", () => reject(signal.reason), { once: true });
    fireDeadline();
  });
  hooks.clock = () => 0;
  hooks.setDeadlineTimer = (callback, ms) => {
    assert.equal(ms, 4000);
    fireDeadline = callback;
    return 1;
  };
  hooks.clearDeadlineTimer = () => {};
  hooks.spawnProcess = () => ({});
  let finishTermination;
  hooks.terminate = () => new Promise((resolve) => { hooks.terminated = true; finishTermination = resolve; });
  let settled = false;
  const running = runRuntimeSyncPolicy({ ...baseConfig(), deadline_ms: 5000 }, { ...hooks, batchIndex: 1 })
    .finally(() => { settled = true; });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(hooks.terminated, true);
  assert.equal(settled, false);
  finishTermination();
  await assert.rejects(running, /deadline|abort/i);
  assert.equal(settled, true);
});

test("仅五个唯一且同配置同输入的 PASS_BATCH 可聚合", { timeout: 10_000, concurrency: true }, async () => {
  const reports = [];
  for (let batchIndex = 1; batchIndex <= 5; batchIndex += 1) {
    reports.push(await runRuntimeSyncPolicy(baseConfig(), { ...dependencies(), batchIndex }));
  }
  const aggregate = aggregateRuntimeSyncPolicy(reports);
  assert.equal(aggregate.status, "COMPLETE_PAIRED_DATA");
  assert.deepEqual(aggregate.completed_batches, [1, 2, 3, 4, 5]);
  assert.equal(aggregate.windows.length, 5);
  assert.equal(aggregate.warmup_sample_count, 10);
  assert.equal(aggregate.measured_sample_count, 20);
  assert.equal(aggregate.campaign_process_count, 30);
  assert.equal(aggregate.windows[0].sample_count, 4);
  assert.equal(aggregate.combined_boundary_wall_ns.per_policy.every_tick.sample_count, 20);
  assert.equal(aggregate.combined_boundary_wall_ns.per_policy.every_tick.p95, "30");
  assert.equal(aggregate.contract_metrics_complete, false);
  assert.throws(() => aggregateRuntimeSyncPolicy(reports.slice(0, 4)), /exactly five/);
  assert.throws(() => aggregateRuntimeSyncPolicy([...reports, reports[0]]), /exactly five/);
  assert.throws(() => aggregateRuntimeSyncPolicy([reports[0], { ...reports[1], source_revision: "b".repeat(40) }, ...reports.slice(2)]), /same campaign/);
  assert.throws(() => aggregateRuntimeSyncPolicy([reports[0], { ...reports[1], status: "FAILED" }, ...reports.slice(2)]), /PASS_BATCH/);
  assert.throws(() => aggregateRuntimeSyncPolicy([reports[0], { ...reports[1], calendar_signature: "different" }, ...reports.slice(2)]), /calendar/);
  assert.throws(() => aggregateRuntimeSyncPolicy([reports[0], { ...reports[1], samples: reports[1].samples.map((sample, index) => index === 0 ? { ...sample, stdout: "not json" } : sample) }, ...reports.slice(2)]), /stdout is not sample JSON/);
  const failedWorkerReport = { ...reports[1], samples: reports[1].samples.map((sample, index) => {
    if (index !== 0) return sample;
    const raw = JSON.parse(sample.stdout);
    raw.shutdown.runtime.worker_results[0].result = { Err: "failed" };
    return { ...sample, stdout: JSON.stringify(raw) };
  }) };
  assert.throws(() => aggregateRuntimeSyncPolicy([reports[0], failedWorkerReport, ...reports.slice(2)]), /worker shutdown/);
  const forgedBoundaryStats = { ...reports[1], samples: reports[1].samples.map((sample, index) => index === 2 ? { ...sample, boundary_wall_ns: ["999999", "999999"] } : sample) };
  assert.throws(() => aggregateRuntimeSyncPolicy([reports[0], forgedBoundaryStats, ...reports.slice(2)]), /boundary wall metadata/);
});

test("失败样本保留先前原始输出和失败回执，成功报告以 wx 写入", { timeout: 10_000, concurrency: true }, async () => {
  const root = await makeTempRoot("runtime-sync-policy-test-");
  try {
    const output = path.join(root, "failed-batch");
    const hooks = dependencies({ failAt: 2 });
    await assert.rejects(runRuntimeSyncPolicy({ ...baseConfig(), output_directory: output }, { ...hooks, batchIndex: 1 }), /injected sample failure/);
    const files = hooks.writes.map((entry) => path.basename(entry.file));
    assert.ok(files.some((file) => file.endsWith(".failure.json")));
    const failedArtifacts = await fs.readdir(path.join(output, "samples"));
    assert.ok(failedArtifacts.some((file) => file.endsWith(".stdout")));
    assert.ok(failedArtifacts.some((file) => file.endsWith(".stderr")));
    assert.equal(hooks.writes.every((entry) => entry.options?.flag === "wx"), true);

    const successOutput = path.join(root, "successful-batch");
    const success = dependencies();
    await runRuntimeSyncPolicy({ ...baseConfig(), output_directory: successOutput }, { ...success, batchIndex: 1 });
    assert.ok(success.writes.some((entry) => entry.file.endsWith("runtime-sync-policy-report.json.partial") && entry.options?.flag === "wx"));
    assert.equal(success.renames.length, 1);
    const actualOutput = path.join(root, "actual-fs-batch");
    const actual = dependencies();
    await runRuntimeSyncPolicy({ ...baseConfig(), output_directory: actualOutput }, {
      ...actual, batchIndex: 1, writeFile: (...args) => fs.writeFile(...args), mkdir: (...args) => fs.mkdir(...args),
      renameFile: (...args) => fs.rename(...args), unlinkFile: (...args) => fs.unlink(...args),
    });
    assert.equal(JSON.parse(await fs.readFile(path.join(actualOutput, "runtime-sync-policy-report.json"), "utf8")).status, "PASS_BATCH");
    assert.equal((await fs.readdir(path.join(actualOutput, "samples"))).filter((name) => name.endsWith(".stdout")).length, 6);
    const rawName = (await fs.readdir(path.join(actualOutput, "samples"))).find((name) => name.endsWith(".stdout"));
    assert.equal((await fs.readFile(path.join(actualOutput, "samples", rawName), "utf8")).startsWith("{\"schema\":\"runtime-sync-policy-sample\""), true);
    await assert.rejects(runRuntimeSyncPolicy({ ...baseConfig(), output_directory: actualOutput }, { ...dependencies(), batchIndex: 1, mkdir: (...args) => fs.mkdir(...args) }), /EEXIST/);
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
});

test("execution deadline 后只保留已开 raw 文件，不新增 failure metadata", { timeout: 10_000, concurrency: true }, async () => {
  const root = await makeTempRoot("runtime-sync-policy-cutoff-");
  try {
    const output = path.join(root, "batch");
    const hooks = dependencies({ failAt: 2 });
    hooks.runSample = async (endpoint, options) => {
      const current = hooks.calls.length;
      hooks.calls.push({ endpoint, options });
      if (current === 3) { hooks.clockValue = 4000; throw new Error("deadline edge failure"); }
      return measured(probe(endpoint.command.at(-3)));
    };
    await assert.rejects(runRuntimeSyncPolicy({ ...baseConfig(), deadline_ms: 5000, output_directory: output }, { ...hooks, batchIndex: 1 }), /deadline edge failure/);
    assert.equal(hooks.writes.some((entry) => entry.file.endsWith(".failure.json")), false);
    const samples = await fs.readdir(path.join(output, "samples"));
    assert.ok(samples.some((entry) => entry.endsWith(".stdout")));
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
});

test("raw stdout 写入失败与被终止的子进程错误同时保留", { timeout: 10_000, concurrency: true }, async () => {
  const root = await makeTempRoot("runtime-sync-policy-raw-error-");
  try {
    const output = path.join(root, "batch");
    const hooks = dependencies();
    hooks.openFile = async () => ({ writeFile: async () => { throw new Error("ENOSPC raw evidence"); }, close: async () => {} });
    hooks.spawnProcess = () => ({ stdout: new PassThrough(), stderr: new PassThrough() });
    hooks.runSample = (_endpoint, { signal, spawn }) => new Promise((resolve, reject) => {
      const child = spawn("fake", [], {});
      child.stdout.write("raw stdout");
      signal.addEventListener("abort", () => reject(new Error("child terminated")), { once: true });
    });
    await assert.rejects(runRuntimeSyncPolicy({ ...baseConfig(), output_directory: output }, { ...hooks, batchIndex: 1 }), (error) => {
      assert.ok(error instanceof AggregateError);
      assert.match(error.message, /child terminated/);
      assert.match(error.message, /ENOSPC raw evidence/);
      return true;
    });
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
});

test("report 暂存写入越过 299 秒执行界时不发布 PASS", { timeout: 10_000, concurrency: true }, async () => {
  const root = await makeTempRoot("runtime-sync-policy-deadline-");
  try {
    const hooks = dependencies();
    hooks.advanceOnPartial = true;
    const output = path.join(root, "late-batch");
    await assert.rejects(runRuntimeSyncPolicy({ ...baseConfig(), deadline_ms: 5000, output_directory: output }, { ...hooks, batchIndex: 1 }), /deadline.*(publication|metadata write)/);
    assert.equal(hooks.renames, undefined);
    assert.ok(hooks.unlinks?.some((file) => file.endsWith("runtime-sync-policy-report.json.partial")));
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
});

test("memory-growth config 固定 schema、策略与预注册闭市自然日", { timeout: 10_000, concurrency: true }, async () => {
  const module = await import("./runtime-sync-policy.mjs");
  assert.equal(typeof module.validateRuntimeSyncMemoryGrowthConfig, "function");
  const config = {
    schema: "runtime-sync-memory-growth-config", schema_version: 1,
    binary: "/tmp/engine-probe", cwd: "/tmp", setup_file: "/tmp/setup.json",
    seed: "42", civil_days_n: 2, policy: "every_tick",
    expected_closed_dates: ["2026-10-30"], cpu_affinity: [0, 1], source_revision: "a".repeat(40),
  };
  assert.equal(module.validateRuntimeSyncMemoryGrowthConfig(config).deadline_ms, 300000);
  assert.throws(() => module.validateRuntimeSyncMemoryGrowthConfig({ ...config, rounds: 1 }), /unknown key|rounds/);
  assert.throws(() => module.validateRuntimeSyncMemoryGrowthConfig({ ...config, policy: "on_demand" }), /policy/);
  assert.equal(module.validateRuntimeSyncMemoryGrowthConfig({ ...config, expected_closed_dates: ["2026-10-30", "2026-10-31", "2026-11-01", "2026-11-02"] }).expected_closed_dates.length, 4);
});

function memoryGrowthConfig(patch = {}) {
  return {
    schema: "runtime-sync-memory-growth-config", schema_version: 1,
    binary: "/tmp/engine-probe", cwd: "/tmp", setup_file: "/tmp/memory-setup.json",
    seed: "42", civil_days_n: 1, policy: "every_tick", expected_closed_dates: ["2030-01-05"],
    cpu_affinity: [0, 1], source_revision: "a".repeat(40), sample_interval_ms: 5, deadline_ms: 300000,
    ...patch,
  };
}

const memorySetup = {
  start_date: "2030-01-04", ticks_per_day: 3, stocks: [{ symbol: "AAA", category: "main_board", initial_price: 1000 }], runtime_sync_policy: "every_tick",
  config: { commission_rate: 0.0003 }, company_system: null,
  npcs: { retail_count: 1, inst_count: 1, hot_count: 0 },
};
function memoryDomain(index) {
  return {
    save_slot: { serialized_bytes: String(100 + index) },
    plans: { serialized_bytes: String(200 + index), entry_count: index, terminal_entry_count: 0, owner_sequence_count: index },
    retained_market_days: { serialized_bytes: String(300 + index), entry_count: index },
    personal_trade_confirmations: { serialized_bytes: String(400 + index), account_count: 1, entry_count: index },
    filled_orders: { serialized_bytes: String(500 + index), stock_count: 1, entry_count: index },
    snapshot: { serialized_bytes: String(600 + index), account_count: 1, market_count: 1, daily_candle_count: index },
    runtime_state: { serialized_bytes: String(700 + index), live_envelope_count: index, retail_projection_seen_count: 1, strategy_state_count: 1 },
  };
}
function memorySample(patch = {}) {
  const boundaries = [
    { settled_date: "2030-01-04", next_date: "2030-01-05", tick: 3, seq: 10, settled_phase: "intraday_trading" },
    { settled_date: "2030-01-05", next_date: "2030-01-06", tick: 3, seq: 11, settled_phase: "closed_day" },
  ];
  const observation = (civil_day_count, boundary, index) => ({
    civil_day_count, settled_date: boundary.settled_date, tick: boundary.tick, seq: boundary.seq,
    completed_trading_days: 1, closed_civil_days: civil_day_count - 1,
    candidate_lifetime: "captured_clone_alive_update_dropped",
    rss_before_serialization_bytes: 4096 + index * 20, rss_after_serialization_bytes: 4100 + index * 20,
    process_tree_process_count: 1, rss_source: "linux_proc_self_status_vmrss_all_tasks_children_checked",
    serialized_bytes_are_resident_allocation: false, domains: memoryDomain(index),
  });
  return {
    schema: "runtime-sync-memory-growth-sample", schema_version: 1, policy: "every_tick", seed: "42",
    setup: memorySetup, effective_setup: { ...memorySetup, serde_default_field: true },
    topology: { groups: 1, desks: 1 }, stocks: 1, npc_count: 2,
    initialization_wall_ns: "50", run_wall_ns: "1000", shutdown_confirmed: true,
    pure_flush_wall_ns: null, pure_flush_availability: "unavailable", coverage_table_peak_count: null,
    coverage_table_peak_availability: "unavailable",
    measurement_effects: "PlanBook serialization temporarily collects owner sequences; candidate RSS snapshots include allocator/serialization effects.",
    shutdown: { runtime: { owned_workers: 2, joined_workers: 2, group_finals: 1, desk_finals: 1, worker_results: [{ result: { Ok: null } }, { result: { Ok: null } }] } },
    civil_days_n: 1, completed_civil_days: 2, completed_trading_days: 1, completed_ticks: 3, last_tick: 3, final_seq: 11,
    final_civil_date: "2030-01-06", final_phase: "closed_day",
    civil_day_boundaries: boundaries, closed_dates: ["2030-01-05"],
    observations: [observation(1, boundaries[0], 0), observation(2, boundaries[1], 1)],
    ...patch,
  };
}

function memoryDependencies({ sample = memorySample(), failure } = {}) {
  const hooks = dependencies();
  hooks.files.set("/tmp/memory-setup.json", JSON.stringify(sample.setup ?? memorySetup));
  hooks.runSample = async (endpoint, options) => {
    hooks.calls.push({ endpoint, options });
    if (failure) throw failure;
    assert.deepEqual(endpoint.command.slice(3), ["/tmp/engine-probe", "memory-growth", "/tmp/memory-setup.json", "every_tick", "42", "1"]);
    return { ...measured(sample), wall_ns: sample.run_wall_ns, stdout: JSON.stringify(sample) };
  };
  return hooks;
}

test("memory-growth 单进程双观察点按真实 delta tick 报告负增长与RSS快照差", { timeout: 10_000, concurrency: true }, async () => {
  const { runRuntimeSyncMemoryGrowth } = await import("./runtime-sync-policy.mjs");
  const hooks = memoryDependencies();
  const report = await runRuntimeSyncMemoryGrowth(memoryGrowthConfig(), hooks);
  assert.equal(hooks.calls.length, 1);
  assert.equal(report.status, "COMPLETE_MEMORY_GROWTH_DIAGNOSTIC");
  assert.equal(report.domain_serialized_growth.save_slot.delta, "1");
  assert.equal(report.domain_serialized_growth.save_slot.delta_ticks, 0);
  assert.deepEqual(report.domain_serialized_growth.save_slot.per_tick_slope, { availability: "unavailable", value: null });
  assert.equal(report.candidate_rss_snapshot_delta_bytes, 20);
  assert.equal(report.pure_flush_availability, "unavailable");
  assert.equal(report.private_coverage_peak_availability, "unavailable");
  assert.equal(report.contract_metrics_complete, false);
  assert.ok(report.measurement_gaps.some((gap) => gap.includes("不相加解释 RSS")));
});

test("memory-growth 接受负序列化delta、按真实tick差计算斜率及全休市零tick", { timeout: 10_000, concurrency: true }, async () => {
  const { runRuntimeSyncMemoryGrowth } = await import("./runtime-sync-policy.mjs");
  const declining = memorySample({
    completed_trading_days: 2, completed_ticks: 6, last_tick: 6, final_seq: 12, closed_dates: [], final_civil_date: "2030-01-05",
    setup: { ...memorySetup, start_date: "2030-01-03" },
    effective_setup: { ...memorySetup, start_date: "2030-01-03", serde_default_field: true },
    civil_day_boundaries: [
      { settled_date: "2030-01-03", next_date: "2030-01-04", tick: 3, seq: 10, settled_phase: "intraday_trading" },
      { settled_date: "2030-01-04", next_date: "2030-01-05", tick: 6, seq: 12, settled_phase: "intraday_trading" },
    ],
  });
  declining.observations[0].settled_date = "2030-01-03";
  declining.observations[0].closed_civil_days = 0;
  declining.observations[1].settled_date = "2030-01-04";
  declining.observations[1].completed_trading_days = 2;
  declining.observations[1].closed_civil_days = 0;
  declining.observations[1].tick = 6;
  declining.observations[1].seq = 12;
  declining.observations[1].domains.save_slot.serialized_bytes = "95";
  const report = await runRuntimeSyncMemoryGrowth(memoryGrowthConfig({ expected_closed_dates: [] }), memoryDependencies({ sample: declining }));
  assert.deepEqual(report.domain_serialized_growth.save_slot, {
    delta: "-5", delta_ticks: 3, per_tick_slope: { availability: "available", value: "-5/3" },
  });

  const allClosed = memorySample({
    setup: { ...memorySetup, start_date: "2030-01-05" },
    effective_setup: { ...memorySetup, start_date: "2030-01-05", serde_default_field: true },
    shutdown: { runtime: null }, completed_trading_days: 0, completed_ticks: 0,
    final_phase: "intraday_trading", final_civil_date: "2030-01-07", closed_dates: ["2030-01-05", "2030-01-06"],
    last_tick: 0, final_seq: 0,
    civil_day_boundaries: [
      { settled_date: "2030-01-05", next_date: "2030-01-06", tick: 0, seq: 0, settled_phase: "closed_day" },
      { settled_date: "2030-01-06", next_date: "2030-01-07", tick: 0, seq: 0, settled_phase: "closed_day" },
    ],
  });
  for (const observation of allClosed.observations) {
    observation.settled_date = allClosed.civil_day_boundaries[observation.civil_day_count - 1].settled_date;
    observation.completed_trading_days = 0;
    observation.closed_civil_days = observation.civil_day_count;
    observation.tick = 0;
    observation.seq = 0;
  }
  const zeroReport = await runRuntimeSyncMemoryGrowth(memoryGrowthConfig({ expected_closed_dates: ["2030-01-05", "2030-01-06"] }), memoryDependencies({ sample: allClosed }));
  assert.deepEqual(zeroReport.domain_serialized_growth.save_slot.per_tick_slope, { availability: "unavailable", value: null });
});

test("memory-growth deadline在启动前触发时不运行probe，输入变更拒绝报告", { timeout: 10_000, concurrency: true }, async () => {
  const { runRuntimeSyncMemoryGrowth } = await import("./runtime-sync-policy.mjs");
  const early = memoryDependencies();
  early.setDeadlineTimer = (callback) => { callback(); return 1; };
  early.clearDeadlineTimer = () => {};
  early.runSample = async () => { throw new Error("probe must not start"); };
  await assert.rejects(runRuntimeSyncMemoryGrowth(memoryGrowthConfig(), early), /deadline|abort/i);

  const drift = memoryDependencies();
  const run = drift.runSample;
  drift.runSample = async (...args) => {
    const result = await run(...args);
    drift.files.set("/tmp/engine-probe", "changed binary");
    return result;
  };
  await assert.rejects(runRuntimeSyncMemoryGrowth(memoryGrowthConfig(), drift), /binary or setup content changed/);
});

test("memory-growth publication deadline丢弃报告并保留raw与失败回执", { timeout: 10_000, concurrency: true }, async () => {
  const { runRuntimeSyncMemoryGrowth } = await import("./runtime-sync-policy.mjs");
  const hooks = memoryDependencies();
  const files = new Map();
  let now = 0;
  hooks.clock = () => now;
  hooks.mkdir = async () => {};
  hooks.openFile = async (file) => ({
    writeFile: async (chunk) => { files.set(file, (files.get(file) ?? "") + chunk.toString()); },
    close: async () => {},
  });
  hooks.writeFile = async (file, content, options) => { hooks.writes.push({ file, content, options }); files.set(file, content); };
  hooks.renameFile = async (from, to) => { files.set(to, files.get(from)); files.delete(from); now = 4000; };
  hooks.unlinkFile = async (file) => { hooks.unlinks ??= []; hooks.unlinks.push(file); files.delete(file); };
  await assert.rejects(runRuntimeSyncMemoryGrowth(memoryGrowthConfig({ deadline_ms: 5000, output_directory: "/tmp/memory-growth-output" }), hooks), /deadline.*publication/);
  assert.ok([...files.keys()].some((file) => file.endsWith(".stdout")));
  assert.ok(hooks.unlinks.some((file) => file.endsWith("runtime-sync-memory-growth-report.json")));
  assert.equal(hooks.writes.some(({ file }) => file.endsWith(".failure.json")), false);
});

test("memory-growth rename失败清理partial并保留stdout/stderr/exit证据", { timeout: 10_000, concurrency: true }, async () => {
  const { runRuntimeSyncMemoryGrowth } = await import("./runtime-sync-policy.mjs");
  const hooks = memoryDependencies();
  const files = new Map();
  const output = path.join(repoRoot, ".tmp", "memory-growth-rename-failure");
  hooks.mkdir = async () => {};
  hooks.openFile = async (file) => ({
    writeFile: async (chunk) => { files.set(file, (files.get(file) ?? "") + chunk.toString()); },
    close: async () => {},
  });
  hooks.writeFile = async (file, content) => { files.set(file, content); };
  hooks.renameFile = async () => { throw new Error("rename report failed"); };
  hooks.unlinkFile = async (file) => { hooks.unlinks ??= []; hooks.unlinks.push(file); files.delete(file); };
  await assert.rejects(runRuntimeSyncMemoryGrowth(memoryGrowthConfig({ output_directory: output }), hooks), /rename report failed/);
  assert.ok((hooks.unlinks ?? []).some((file) => file.endsWith("runtime-sync-memory-growth-report.json.partial")));
  assert.equal([...files.keys()].some((file) => file.endsWith(".partial")), false);
  const failure = [...files.entries()].find(([file]) => file.endsWith(".failure.json"));
  assert.ok(failure);
  assert.equal(JSON.parse(failure[1]).stdout, JSON.stringify(memorySample()));
  assert.equal(JSON.parse(failure[1]).stderr, "");
  assert.equal([...files.keys()].some((file) => file.endsWith("runtime-sync-memory-growth-report.json")), false);

  const cleanupFailure = memoryDependencies();
  cleanupFailure.mkdir = async () => {};
  cleanupFailure.openFile = async () => ({ writeFile: async () => {}, close: async () => {} });
  cleanupFailure.writeFile = async () => {};
  cleanupFailure.renameFile = async () => { throw new Error("rename primary"); };
  cleanupFailure.unlinkFile = async () => { throw new Error("partial unlink failed"); };
  await assert.rejects(runRuntimeSyncMemoryGrowth(memoryGrowthConfig({ output_directory: output }), cleanupFailure), (error) => {
    assert.ok(error instanceof AggregateError);
    assert.equal(error.cause.message, "rename primary");
    assert.ok(error.errors.some((cause) => cause.message === "partial unlink failed"));
    return true;
  });
});

test("memory-growth raw写入失败与进程终止失败共同保留primary cause", { timeout: 10_000, concurrency: true }, async () => {
  const { runRuntimeSyncMemoryGrowth } = await import("./runtime-sync-policy.mjs");
  const hooks = memoryDependencies();
  hooks.mkdir = async () => {};
  hooks.writeFile = async () => {};
  hooks.openFile = async () => ({ writeFile: async () => { throw new Error("ENOSPC memory raw"); }, close: async () => {} });
  hooks.spawnProcess = () => ({ stdout: new PassThrough(), stderr: new PassThrough() });
  hooks.runSample = (_endpoint, { signal, spawn }) => new Promise((_resolve, reject) => {
    const child = spawn("fake", [], {});
    child.stdout.write("partial sample");
    signal.addEventListener("abort", () => reject(new Error("memory child terminated")), { once: true });
  });
  hooks.terminate = async () => { throw new Error("memory tree termination failed"); };
  await assert.rejects(runRuntimeSyncMemoryGrowth(memoryGrowthConfig({ output_directory: "/tmp/memory-growth-error" }), hooks), (error) => {
    assert.ok(error instanceof AggregateError);
    assert.match(error.message, /memory child terminated/);
    assert.match(error.message, /memory tree termination failed/);
    assert.equal(error.cause.message, "memory child terminated");
    return true;
  });
});

test("memory-growth stderr raw打开失败且stdout关闭失败保留两个cause", { timeout: 10_000, concurrency: true }, async () => {
  const { runRuntimeSyncMemoryGrowth } = await import("./runtime-sync-policy.mjs");
  const hooks = memoryDependencies();
  hooks.mkdir = async () => {};
  hooks.writeFile = async () => {};
  let opens = 0;
  hooks.openFile = async () => {
    opens += 1;
    if (opens === 1) return { close: async () => { throw new Error("stdout close failed"); }, writeFile: async () => {} };
    throw new Error("stderr open failed");
  };
  await assert.rejects(runRuntimeSyncMemoryGrowth(memoryGrowthConfig({ output_directory: "/tmp/open-close-error" }), hooks), (error) => {
    assert.ok(error instanceof AggregateError);
    assert.equal(error.cause.message, "stderr open failed");
    assert.ok(error.errors.some((cause) => cause.message === "stdout close failed"));
    assert.match(error.message, /stdout close failed/);
    return true;
  });
});

test("memory-growth partial报告写入失败会清理暂存文件并保留原错误", { timeout: 10_000, concurrency: true }, async () => {
  const { runRuntimeSyncMemoryGrowth } = await import("./runtime-sync-policy.mjs");
  const hooks = memoryDependencies();
  const files = new Map();
  hooks.mkdir = async () => {};
  hooks.openFile = async () => ({ writeFile: async () => {}, close: async () => {} });
  hooks.writeFile = async (file, content) => {
    if (file.endsWith(".partial")) {
      files.set(file, String(content).slice(0, 17));
      throw new Error("ENOSPC partial report");
    }
    files.set(file, content);
  };
  hooks.unlinkFile = async (file) => { hooks.unlinks ??= []; hooks.unlinks.push(file); files.delete(file); };
  await assert.rejects(runRuntimeSyncMemoryGrowth(memoryGrowthConfig({ output_directory: "/tmp/memory-growth-partial" }), hooks), (error) => {
    assert.match(error.message, /ENOSPC partial report/);
    assert.equal(error.cause, undefined);
    assert.ok((hooks.unlinks ?? []).some((file) => file.endsWith("runtime-sync-memory-growth-report.json.partial")));
    assert.equal([...files.keys()].some((file) => file.endsWith(".partial")), false);
    return true;
  });
});

test("memory-growth拒绝effective setup配置事实漂移与无效plan计数，接受省略None", { timeout: 10_000, concurrency: true }, async () => {
  const { runRuntimeSyncMemoryGrowth } = await import("./runtime-sync-policy.mjs");
  const base = memorySample({ setup: { ...memorySetup, company_system: null }, effective_setup: { ...memorySetup, serde_default_field: true } });
  const hooks = memoryDependencies({ sample: base });
  const report = await runRuntimeSyncMemoryGrowth(memoryGrowthConfig(), hooks);
  assert.equal(report.status, "COMPLETE_MEMORY_GROWTH_DIAGNOSTIC");

  const changedEffective = structuredClone(base);
  changedEffective.effective_setup.config = { ...changedEffective.effective_setup.config, commission_rate: 0.0004 };
  await assert.rejects(runRuntimeSyncMemoryGrowth(memoryGrowthConfig(), memoryDependencies({ sample: changedEffective })), /effective_setup/);
  for (const [field, patch] of [
    ["stocks", { ...changedEffective.effective_setup.stocks[0], initial_price: 2000 }],
    ["start_date", "2030-01-05"],
  ]) {
    const drifted = structuredClone(base);
    if (field === "stocks") drifted.effective_setup.stocks = [patch];
    else drifted.effective_setup[field] = patch;
    await assert.rejects(runRuntimeSyncMemoryGrowth(memoryGrowthConfig(), memoryDependencies({ sample: drifted })), /effective_setup/);
  }

  const badPlans = structuredClone(base);
  badPlans.observations[0].domains.plans.entry_count = 1;
  badPlans.observations[0].domains.plans.terminal_entry_count = 2;
  await assert.rejects(runRuntimeSyncMemoryGrowth(memoryGrowthConfig(), memoryDependencies({ sample: badPlans })), /terminal entries/);
});

test("memory-growth partial写入与partial清理双错保留两个cause", { timeout: 10_000, concurrency: true }, async () => {
  const { runRuntimeSyncMemoryGrowth } = await import("./runtime-sync-policy.mjs");
  const hooks = memoryDependencies();
  hooks.mkdir = async () => {};
  hooks.openFile = async () => ({ writeFile: async () => {}, close: async () => {} });
  hooks.writeFile = async (file) => { if (file.endsWith(".partial")) throw new Error("partial primary"); hooks.writes.push({ file }); };
  hooks.unlinkFile = async () => { throw new Error("partial cleanup failure"); };
  await assert.rejects(runRuntimeSyncMemoryGrowth(memoryGrowthConfig({ output_directory: "/tmp/partial-double-failure" }), hooks), (error) => {
    assert.ok(error instanceof AggregateError);
    assert.equal(error.cause.message, "partial primary");
    assert.ok(error.errors.some((cause) => cause.message === "partial cleanup failure"));
    return true;
  });
});
