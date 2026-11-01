import assert from "node:assert/strict";
import { it } from "node:test";

import path from "node:path";

import {
  buildWebTestShardPolicy,
  discoverWebTestFiles,
  main,
  runWebTestBatch,
  runWebTests,
} from "./run-web-tests.mjs";

const REPO_ROOT = path.resolve(import.meta.dirname, "..");

it("discovers every Web test file deterministically and rejects non-tests", async () => {
  const files = await discoverWebTestFiles(path.join(REPO_ROOT, "apps", "web"));
  assert.ok(files.length > 0);
  assert.deepEqual(files, [...files].sort());
  assert.equal(new Set(files).size, files.length);
  assert.ok(files.every((file) => /\.test\.tsx?$/.test(file)));
  assert.ok(files.some((file) => file.endsWith("src/host/worker-host.test.ts")));
});

it("partitions files across real processes without overselling the CPU budget", () => {
  const files = Array.from({ length: 17 }, (_, index) => `/web/src/group/test-${index}.test.ts`);
  assert.deepEqual(buildWebTestShardPolicy(128, files), {
    available_cpu_count: 128,
    file_count: 17,
    shard_count: 8,
    per_shard_file_concurrency: 8,
    max_concurrent_processes: 64,
    aggregate_process_budget: 64,
    shards: [
      [files[0], files[8], files[16]],
      [files[1], files[9]],
      [files[2], files[10]],
      [files[3], files[11]],
      [files[4], files[12]],
      [files[5], files[13]],
      [files[6], files[14]],
      [files[7], files[15]],
    ],
  });
  for (let cpuCount = 1; cpuCount <= 32; cpuCount += 1) {
    const policy = buildWebTestShardPolicy(cpuCount, files);
    assert.ok(policy.aggregate_process_budget <= cpuCount);
    assert.deepEqual(policy.shards.flat().sort(), [...files].sort());
  }
});

it("rejects an empty Web test inventory", () => {
  assert.throws(() => buildWebTestShardPolicy(8, []), /no Web test files/i);
});

it("shares one ten-second deadline across every shard", async () => {
  let nowMs = 1_000;
  const calls = [];
  const files = ["/web/a.test.ts", "/web/b.test.ts", "/web/c.test.ts"];
  const result = await runWebTestBatch({
    webRoot: "/web",
    cpuCount: 2,
    discover: async () => files,
    now: () => nowMs,
    run: async (options) => {
      calls.push(options);
      nowMs += 1_000;
      return { stdout: "", stderr: "" };
    },
  });
  assert.deepEqual(calls.map((call) => call.timeoutMs), [10_000, 9_000]);
  assert.ok(calls.every((call) => call.timeoutMs <= 10_000));
  assert.ok(calls.every((call) => call.command === process.execPath));
  assert.ok(calls.every((call) => call.args.includes("--test-force-exit")));
  assert.deepEqual(calls.flatMap((call) => call.args.slice(4)).sort(), [...files].sort());
  assert.equal(result.file_count, 3);
  assert.equal(result.shard_count, 2);
});

it("每个测试文件独立进程隔离，禁止同进程共享导致的 mock 状态串扰", async () => {
  const calls = [];
  const files = Array.from({ length: 16 }, (_, index) => `/web/src/file-${index}.test.ts`);
  const policy = await runWebTestBatch({
    webRoot: "/web",
    cpuCount: 16,
    discover: async () => files,
    run: async (options) => {
      calls.push(options);
      return { stdout: "", stderr: "" };
    },
  });
  assert.equal(policy.shard_count, 8);
  assert.ok(calls.every((call) => !call.args.includes("--test-isolation=none")),
    "禁止 --test-isolation=none：同进程共享已证实造成 player-proposals 与 wasm-worker 系列串扰");
  for (const call of calls) {
    const concurrency = Number(/--test-concurrency=(\d+)/.exec(call.args.join(" "))?.[1]);
    assert.ok(Number.isInteger(concurrency) && concurrency >= 1, "每分片必须显式声明每文件并发");
  }
  const aggregate = calls.length * Number(/--test-concurrency=(\d+)/.exec(calls[0].args.join(" "))?.[1]);
  assert.ok(aggregate <= 16, "分片×每文件并发不得超卖 CPU 预算");
});

it("fails fast and aborts in-flight sibling shards", async () => {
  let releaseSibling;
  const siblingStarted = new Promise((resolve) => { releaseSibling = resolve; });
  let siblingAborted = false;
  await assert.rejects(runWebTestBatch({
    webRoot: "/web",
    cpuCount: 2,
    discover: async () => ["/web/fail.test.ts", "/web/sibling.test.ts"],
    run: async (options) => {
      if (options.args.includes("/web/fail.test.ts")) {
        await siblingStarted;
        throw new Error("injected Web failure");
      }
      releaseSibling();
      await new Promise((_, reject) => options.signal.addEventListener("abort", () => {
        siblingAborted = true;
        reject(options.signal.reason);
      }, { once: true }));
    },
  }), /Web test shard failed/i);
  assert.equal(siblingAborted, true);
});

it("places the complete Web worker behind an external ten-second gate", async () => {
  const calls = [];
  await runWebTests({
    cwd: REPO_ROOT,
    run: async (options) => { calls.push(options); },
  });
  assert.equal(calls.length, 1);
  assert.equal(calls[0].command, process.execPath);
  assert.equal(calls[0].timeoutMs, 10_000);
  assert.ok(calls[0].args.includes("--internal-worker"));
});

it("rejects direct invocation of an unsupervised internal Web worker", async () => {
  await assert.rejects(main(["--internal-worker"], {}), /external ten-second supervisor/i);
});

it("完整回归分批覆盖全部文件并保持单批十秒门禁", async () => {
  const files = Array.from({ length: 17 }, (_, index) => `/web/${index}.test.ts`);
  const seen = [];
  for (let index = 0; index < 8; index += 1) {
    await runWebTestBatch({ webRoot: "/web", cpuCount: 4, batchIndex: index, batchCount: 8,
      discover: async () => files, run: async (options) => {
        assert.ok(options.timeoutMs <= 10000);
        seen.push(...options.args.slice(4));
      } });
  }
  assert.deepEqual(seen.sort(), [...files].sort());
  await assert.rejects(runWebTestBatch({ batchIndex: 8, batchCount: 8, discover: async () => files }), /batch/i);
});

it("完整回归单独执行启动页完整 App SSR，其余文件恰好执行一次", async () => {
  const startup = "/web/src/app/startup-screen.test.ts";
  const files = [startup, ...Array.from({ length: 16 }, (_, index) => `/web/src/file-${index}.test.ts`)];
  const seen = [];
  for (let index = 0; index < 9; index += 1) {
    await runWebTestBatch({ webRoot: "/web", cpuCount: 8, batchIndex: index, batchCount: 9,
      discover: async () => files, run: async (options) => { seen.push(...options.args.slice(4)); } });
  }
  assert.deepEqual(seen.sort(), [...files].sort());
  const calls = [];
  await runWebTestBatch({ cpuCount: 8, batchIndex: 8, batchCount: 9, discover: async () => files,
    run: async (options) => { calls.push(options); } });
  assert.deepEqual(calls.flatMap((call) => call.args.slice(4)), [startup]);
});

it("完整回归要求唯一启动页测试，缺失或歧义显式失败", async () => {
  const ordinary = Array.from({ length: 9 }, (_, index) => `/web/src/file-${index}.test.ts`);
  for (const files of [ordinary, [...ordinary, "/a/app/startup-screen.test.ts", "/b/app/startup-screen.test.ts"]]) {
    await assert.rejects(runWebTestBatch({ batchIndex: 0, batchCount: 9, discover: async () => files }), /exactly one startup-screen/);
  }
});
