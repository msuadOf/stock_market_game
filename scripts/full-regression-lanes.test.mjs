import assert from "node:assert/strict";
import { it } from "node:test";
import * as regression from "./run-full-regression.mjs";
const executeRegressionLanes = (...args) => regression.executeRegressionLanes(...args);
const regressionLaneBudget = (...args) => regression.regressionLaneBudget(...args);

it("双链并行，失败取消另一链并等待清理", async () => {
  let started;
  const ready = new Promise((resolve) => { started = resolve; });
  let cleaned = false;
  await assert.rejects(executeRegressionLanes(8, async (signal) => {
    started();
    await new Promise((resolve) => signal.addEventListener("abort", resolve, { once: true }));
    await new Promise((resolve) => setTimeout(resolve, 10));
    cleaned = true;
  }, async () => { await ready; throw new Error("Web 失败"); }), /Web 失败/);
  assert.equal(cleaned, true);
});

it("单核串行，多核预算相加不超过可用 CPU", async () => {
  assert.deepEqual(regressionLaneBudget(128), { rust: 96, web: 32 });
  assert.deepEqual(regressionLaneBudget(1), { rust: 1, web: 1 });
  const order = [];
  await executeRegressionLanes(1, async () => { order.push("rust"); }, async () => { order.push("web"); });
  assert.deepEqual(order, ["rust", "web"]);
});

it("真实 Rust supervisor 取消后确认 case 进程退出", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  const { mkdtemp, writeFile, chmod, readFile, rm } = await import("node:fs/promises");
  const { existsSync, readFileSync } = await import("node:fs");
  const os = await import("node:os");
  const path = await import("node:path");
  const { executeSupervisedRustTestBinaries } = await import("./rust-test-case-supervisor.mjs");
  const dir = await mkdtemp(path.join(os.tmpdir(), "regression-abort-"));
  const binary = path.join(dir, "fixture.sh");
  const pidFile = path.join(dir, "pid");
  await writeFile(binary, `#!/bin/sh\nif [ "$1" = "--list" ]; then\n  if [ "$3" != "--ignored" ]; then printf 'hang: test\\n'; fi\nelse\n  echo $$ > '${pidFile}'\n  exec sleep 30\nfi\n`);
  await chmod(binary, 0o755);
  const controller = new AbortController();
  try {
    const promise = executeSupervisedRustTestBinaries({ artifacts: [{ executable: binary, label: "fixture" }],
      policy: regression.buildRustTestExecutionPolicy(2, 1), cwd: dir, env: process.env,
      remainingMs: () => 8000, signal: controller.signal });
    let settled = false;
    promise.then(() => { settled = true; }, () => { settled = true; });
    while (!existsSync(pidFile) && !settled) await new Promise((resolve) => setTimeout(resolve, 10));
    assert.equal(existsSync(pidFile), true);
    const pid = Number(await readFile(pidFile, "utf8"));
    controller.abort(new Error("取消测试"));
    await assert.rejects(promise, /取消测试/);
    try { assert.match(readFileSync(`/proc/${pid}/stat`, "utf8").split(") ")[1], /^[ZX] /); }
    catch (error) { if (error.code !== "ENOENT") throw error; }
  } finally { await rm(dir, { recursive: true, force: true }); }
});

it("Rust 失败时取消 Web 链并等待清理", async () => {
  let started;
  const ready = new Promise((resolve) => { started = resolve; });
  let cleaned = false;
  await assert.rejects(executeRegressionLanes(8, async () => { await ready; throw new Error("Rust 失败"); }, async (signal) => {
    started();
    await new Promise((resolve) => signal.addEventListener("abort", resolve, { once: true }));
    await new Promise((resolve) => setTimeout(resolve, 10));
    cleaned = true;
  }), /Rust 失败/);
  assert.equal(cleaned, true);
});

it("上级取消 scripts 批次时停止队列并等待在途文件", async () => {
  const { runScriptTestBatch } = await import("./run-script-tests.mjs");
  const controller = new AbortController();
  const calls = [];
  let cleaned = false;
  const batch = runScriptTestBatch({ cpuCount: 1, signal: controller.signal,
    discover: async () => ["first.test.mjs", "second.test.mjs"],
    run: async ({ args, signal }) => {
      calls.push(args.at(-1));
      controller.abort(new Error("上级失败"));
      assert.equal(signal.aborted, true);
      await new Promise((resolve) => setTimeout(resolve, 10));
      cleaned = true;
      throw signal.reason;
    },
  });
  await assert.rejects(batch, /上级失败/);
  assert.deepEqual(calls, ["first.test.mjs"]);
  assert.equal(cleaned, true);
});
