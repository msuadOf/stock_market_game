import assert from "node:assert/strict";
import path from "node:path";
import { test } from "node:test";
import { fullRegressionSteps } from "./run-full-regression.mjs";
import { main as runWebMain, runWebTestBatch, runWebTests } from "./run-web-tests.mjs";

test("sealed regression starts the Web worker directly under its existing external ten-second supervisor", () => {
  const repoRoot = path.resolve("source");
  const workspaceRoot = path.resolve("workspace");
  const step = fullRegressionSteps(4, repoRoot, { FIXTURE_ENV: "preserved" }, workspaceRoot)
    .find((entry) => entry.kind === "ordinary-command");
  assert.equal(step.command, process.execPath);
  assert.deepEqual(step.args, [path.join(repoRoot, "scripts", "run-web-tests.mjs"), "--internal-worker"]);
  assert.equal(step.env.STOCK_GAME_WEB_TEST_INTERNAL_WORKER, "1");
  assert.equal(step.env.FIXTURE_ENV, "preserved");
  assert.equal(step.env.TMP, path.join(workspaceRoot, ".tmp", "process-tmp", "full-regression"));
});

test("standalone Web invocation still creates an external supervisor and rejects an unmarked worker", async () => {
  const calls = [];
  await runWebTests({ run: async (options) => { calls.push(options); } });
  assert.equal(calls.length, 1);
  assert.equal(calls[0].timeoutMs, 10_000);
  assert.equal(calls[0].cleanupReserveMs, 1_000);
  assert.equal(calls[0].env.STOCK_GAME_WEB_TEST_INTERNAL_WORKER, "1");
  assert.equal(calls[0].args.at(-1), "--internal-worker");
  await assert.rejects(runWebMain(["--internal-worker"], {}), /external ten-second supervisor/);
});

test("direct worker retains case deadlines, real process shards and fail-fast sibling cancellation", async () => {
  const calls = [];
  let startSibling;
  const siblingStarted = new Promise((resolve) => { startSibling = resolve; });
  let cancelled = false;
  await assert.rejects(runWebTestBatch({
    webRoot: "/fixture",
    cpuCount: 2,
    discover: async () => ["/fixture/fail.test.ts", "/fixture/sibling.test.ts"],
    run: async (options) => {
      calls.push(options);
      assert.ok(options.timeoutMs <= 10_000);
      assert.ok(options.args.includes("--test-timeout=10000"));
      if (options.args.includes("/fixture/fail.test.ts")) {
        await siblingStarted;
        throw new Error("deliberate shard failure");
      }
      startSibling();
      await new Promise((resolve, reject) => options.signal.addEventListener("abort", () => {
        cancelled = true;
        reject(options.signal.reason);
      }, { once: true }));
    },
  }), /Web test shard failed/);
  assert.equal(calls.length, 2);
  assert.equal(cancelled, true);
});
