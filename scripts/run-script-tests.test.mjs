import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import path from "node:path";
import { it } from "node:test";
import { buildScriptTestPolicy, discoverScriptTestFiles, main, prepareScriptTestEnvironment, runScriptTestBatch, runScriptTests } from "./run-script-tests.mjs";
import { runBoundedCommand } from "./run-with-deadline.mjs";
import { resolveWorkspaceRoot } from "./workspace-paths.mjs";

it("完整递归发现新目录与测试自身，忽略非测试并拒绝 symbolic link", { timeout: 10000 }, async () => {
  const root = await mkdtemp(path.join(process.cwd(), ".tmp", "script-discovery-"));
  try {
    const nested = path.join(root, "scripts", "nested", "deeper");
    await mkdir(nested, { recursive: true });
    const filenames = [path.join(root, "scripts", "runner.test.mjs"), path.join(nested, "new.test.mjs")];
    await Promise.all([...filenames, path.join(nested, "not-a-test.mjs")].map((filename) => writeFile(filename, "fixture")));
    assert.deepEqual(await discoverScriptTestFiles(root), filenames.sort());
    await symlink(nested, path.join(root, "scripts", "linked"), process.platform === "win32" ? "junction" : "dir");
    await assert.rejects(discoverScriptTestFiles(root), /symbolic link/);
    const actual = await discoverScriptTestFiles(process.cwd());
    assert.ok(actual.some((filename) => filename.endsWith("run-script-tests.test.mjs")));
    assert.ok(actual.some((filename) => filename.endsWith(path.join("simulation", "baseline-run.test.mjs"))));
  } finally { await rm(root, { recursive: true, force: true }); }
});

it("多核 worker 真实重叠执行，完成后释放预算启动排队文件", { timeout: 10000 }, async () => {
  let active = 0;
  let peak = 0;
  const executed = [];
  const result = await runScriptTestBatch({ cpuCount: 2,
    discover: async () => ["a.test.mjs", "b.test.mjs", "c.test.mjs"],
    run: async ({ args }) => {
      executed.push(args.at(-1));
      active += 1;
      peak = Math.max(peak, active);
      await new Promise((resolve) => setTimeout(resolve, 10));
      active -= 1;
    },
  });
  assert.equal(peak, 2);
  assert.equal(active, 0);
  assert.deepEqual(executed, ["a.test.mjs", "b.test.mjs", "c.test.mjs"]);
  assert.equal(result.files.length, 3);
});

it("普通文件进程外门禁终止真实挂起 Node，不借用长聚合放宽单文件", { timeout: 10000 }, async () => {
  const root = await mkdtemp(path.join(process.cwd(), ".tmp", "script-watchdog-"));
  const hanging = path.join(root, "hanging.test.mjs");
  await writeFile(hanging, 'import { it } from "node:test"; it("hang", async () => { await new Promise(() => {}); }); setInterval(() => {}, 1000);');
  const startedAt = Date.now();
  try {
    await assert.rejects(runScriptTestBatch({ cpuCount: 1, discover: async () => [hanging],
      remainingMs: () => 600,
      run: (options) => runBoundedCommand({ ...options, cleanupReserveMs: 150 }),
    }), /hanging\.test\.mjs.*600ms deadline/);
    assert.ok(Date.now() - startedAt < 1500);
  } finally { await rm(root, { recursive: true, force: true }); }
});

it("清单拒绝空集、重复和非法路径，workers 不超 CPU 预算", { timeout: 10000 }, () => {
  assert.throws(() => buildScriptTestPolicy(2, []), /未发现/);
  assert.throws(() => buildScriptTestPolicy(2, ["a.test.mjs", "a.test.mjs"]), /重复/);
  assert.throws(() => buildScriptTestPolicy(2, ["source.mjs"]), /非法/);
  assert.throws(() => buildScriptTestPolicy(0, ["a.test.mjs"]), /CPU/);
  for (const cpuCount of [1, 2, 4, 128]) {
    const policy = buildScriptTestPolicy(cpuCount, Array.from({ length: 8 }, (_, index) => `${index}.test.mjs`));
    assert.equal(policy.concurrent_file_workers, Math.min(4, cpuCount));
  }
});

it("每文件独立进程恰好一次执行并共享上级期限，不触发 npm test 递归", { timeout: 10000 }, async () => {
  const files = ["a.test.mjs", "b.test.mjs", "c.test.mjs"];
  const calls = [];
  let nowMs = 0;
  const result = await runScriptTestBatch({ cwd: process.cwd(), env: { FIXTURE: "preserved" }, cpuCount: 2,
    discover: async () => files, now: () => nowMs, remainingMs: () => 5000 - nowMs,
    run: async (options) => { calls.push(options); nowMs += 1000; },
  });
  assert.deepEqual(calls.map((call) => call.args.at(-1)).sort(), files);
  assert.deepEqual(calls.map((call) => call.timeoutMs), [5000, 4000, 3000]);
  assert.ok(calls.every((call) => call.command === process.execPath && call.args.includes("--test-timeout=10000") && call.args.includes("--test-isolation=none")));
  assert.ok(calls.every((call) => call.env.FIXTURE === "preserved"));
  assert.equal(result.file_count, 3);
  assert.equal(result.concurrent_file_workers, 2);
});

it("失败取消在途 sibling、阻止队列并聚合真实清理失败", { timeout: 10000 }, async () => {
  let release;
  const started = new Promise((resolve) => { release = resolve; });
  const executed = [];
  await assert.rejects(runScriptTestBatch({ cpuCount: 2,
    discover: async () => ["fail.test.mjs", "cleanup.test.mjs", "queued.test.mjs"],
    run: async ({ args, signal }) => {
      executed.push(args.at(-1));
      if (args.at(-1) === "fail.test.mjs") { await started; throw new Error("真实测试失败"); }
      release();
      await new Promise((resolve, reject) => signal.addEventListener("abort", () => reject(new Error("真实进程树清理失败")), { once: true }));
    },
  }), (error) => error instanceof AggregateError && error.errors.length === 2
    && /真实测试失败/.test(error.message) && /真实进程树清理失败/.test(error.message));
  assert.deepEqual(executed, ["fail.test.mjs", "cleanup.test.mjs"]);
});

it("发现后期限耗尽不启动命令，独立入口为长聚合而非放宽普通单测", { timeout: 10000 }, async () => {
  await assert.rejects(runScriptTestBatch({ discover: async () => ["a.test.mjs"], remainingMs: () => 0,
    run: async () => { assert.fail("期限耗尽不启动测试"); },
  }), /期限耗尽/);
  const calls = [];
  await runScriptTests({ run: async (options) => { calls.push(options); } });
  assert.equal(calls[0].timeoutMs, 300000);
  assert.equal(calls[0].args.at(-1), "--internal-worker");
  assert.equal(calls[0].env.STOCK_GAME_SCRIPT_TEST_INTERNAL_WORKER, "1");
  await assert.rejects(main(["--internal-worker"], {}), /supervisor/);
});

it("独立入口先启动监督进程再在 worker 中规范工作区环境", { timeout: 10000 }, async () => {
  const calls = [];
  const originalEnv = { FIXTURE: "preserved", TMPDIR: "/outside", TMP: "/outside", TEMP: "/outside" };
  await runScriptTests({ env: originalEnv,
    run: async (options) => { calls.push(options); },
  });
  assert.equal(calls[0].timeoutMs, 300000);
  for (const key of ["TMPDIR", "TMP", "TEMP"]) assert.equal(calls[0].env[key], "/outside");
  const workspaceEnv = await prepareScriptTestEnvironment({ env: calls[0].env });
  const expected = path.join(await resolveWorkspaceRoot(process.cwd()), ".tmp", "process-tmp", "script-tests");
  assert.equal(workspaceEnv.FIXTURE, "preserved");
  for (const key of ["TMPDIR", "TMP", "TEMP"]) assert.equal(workspaceEnv[key], expected);
  assert.equal(workspaceEnv.STOCK_GAME_SCRIPT_TEST_INTERNAL_WORKER, "1");
  assert.deepEqual(originalEnv, { FIXTURE: "preserved", TMPDIR: "/outside", TMP: "/outside", TEMP: "/outside" });
});
