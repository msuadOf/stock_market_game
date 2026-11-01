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
    per_shard_file_concurrency: 1,
    max_concurrent_processes: 8,
    aggregate_process_budget: 8,
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

it("每个文件独享十秒期限", async () => {
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
  assert.deepEqual(calls.map((call) => call.timeoutMs), [10_000, 10_000, 10_000]);
  assert.ok(calls.every((call) => call.timeoutMs <= 10_000));
  assert.ok(calls.every((call) => call.command === process.execPath));
  assert.ok(calls.every((call) => call.args.includes("--test-force-exit")));
  assert.deepEqual(calls.flatMap((call) => call.args.filter((arg) => arg.endsWith(".test.ts"))).sort(), [...files].sort());
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
  assert.equal(calls.length, files.length);
  assert.ok(calls.every((call) => call.args.includes("--test-isolation=none") && call.args.filter((arg) => arg.endsWith(".test.ts")).length === 1));
  assert.ok(policy.max_concurrent_processes <= 16);
});

it("每个文件直接在独立进程运行", async () => {
  const calls = [];
  const files = ["/web/src/app/workspace-grid-types.test.ts", "/web/src/a.test.ts", "/web/src/b.test.ts"];
  await runWebTestBatch({
    webRoot: "/web",
    cpuCount: 2,
    discover: async () => files,
    run: async (options) => { calls.push(options); },
  });
  const single = calls.find((call) => call.args.includes(files[0]));
  const multiple = calls.find((call) => call.args.includes(files[1]));
  assert.ok(single.args.includes("--test-isolation=none"));
  assert.ok(multiple.args.includes("--test-isolation=none"));
  assert.ok(calls.every((call) => call.args.includes("--test-timeout=10000")));
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

it("places the complete Web worker behind an external five-minute gate", async () => {
  const calls = [];
  await runWebTests({
    cwd: REPO_ROOT,
    run: async (options) => { calls.push(options); },
  });
  assert.equal(calls.length, 1);
  assert.equal(calls[0].command, process.execPath);
  assert.equal(calls[0].timeoutMs, 300_000);
  assert.ok(calls[0].args.includes("--internal-worker"));
});

it("rejects direct invocation of an unsupervised internal Web worker", async () => {
  await assert.rejects(main(["--internal-worker"], {}), /external five-minute supervisor/i);
});

it("完整回归分批覆盖全部文件并保持每文件十秒门禁", async () => {
  const files = Array.from({ length: 17 }, (_, index) => `/web/${index}.test.ts`);
  const seen = [];
  for (let index = 0; index < 8; index += 1) {
    await runWebTestBatch({ webRoot: "/web", cpuCount: 4, batchIndex: index, batchCount: 8,
      discover: async () => files, run: async (options) => {
        assert.ok(options.timeoutMs <= 10000);
        seen.push(...options.args.filter((arg) => !arg.startsWith("--")));
      } });
  }
  assert.deepEqual(seen.sort(), [...files].sort());
  await assert.rejects(runWebTestBatch({ batchIndex: 8, batchCount: 8, discover: async () => files }), /batch/i);
});

it("完整回归单独执行启动页完整 App SSR，其余文件恰好执行一次", async () => {
  const startup = "/web/src/app/startup-screen.test.ts";
  const schema = "/web/src/save/complete-save-schema.test.ts";
  const schema2 = "/web/src/save/complete-save-schema-extra.test.ts";
  const files = [startup, schema, schema2, "/web/src/mobile/mobile-color-contrast.test.ts", ...Array.from({ length: 16 }, (_, index) => `/web/src/file-${index}.test.ts`)];
  const seen = [];
  for (let index = 0; index < 12; index += 1) {
    await runWebTestBatch({ webRoot: "/web", cpuCount: 8, batchIndex: index, batchCount: 12,
      discover: async () => files, run: async (options) => { seen.push(...options.args.filter((arg) => !arg.startsWith("--"))); } });
  }
  assert.deepEqual(seen.sort(), [...files].sort());
  const calls = [];
  await runWebTestBatch({ cpuCount: 8, batchIndex: 8, batchCount: 12, discover: async () => files,
    run: async (options) => { calls.push(options); } });
  assert.deepEqual(calls.flatMap((call) => call.args.filter((arg) => !arg.startsWith("--"))), [startup]);
  const schemaCalls = [];
  await runWebTestBatch({ cpuCount: 8, batchIndex: 9, batchCount: 12, discover: async () => files,
    run: async (options) => { schemaCalls.push(options); } });
  assert.deepEqual(schemaCalls.flatMap((call) => call.args.filter((arg) => !arg.startsWith("--"))), [schema]);
  const schema2Calls = [];
  await runWebTestBatch({ cpuCount: 8, batchIndex: 10, batchCount: 12, discover: async () => files,
    run: async (options) => { schema2Calls.push(options); } });
  assert.deepEqual(schema2Calls.flatMap((call) => call.args.filter((arg) => !arg.startsWith("--"))), [schema2]);
});

it("完整回归要求唯一启动页测试，缺失或歧义显式失败", async () => {
  const ordinary = Array.from({ length: 12 }, (_, index) => `/web/src/file-${index}.test.ts`);
  for (const files of [ordinary, [...ordinary, "/a/app/startup-screen.test.ts", "/b/app/startup-screen.test.ts"]]) {
    await assert.rejects(runWebTestBatch({ batchIndex: 0, batchCount: 12, discover: async () => files }), /exactly one startup-screen/);
  }
});

it("完整回归要求两个 schema 文件各唯一，缺失或歧义显式失败", async () => {
  const ordinary = Array.from({ length: 12 }, (_, index) => `/web/src/file-${index}.test.ts`);
  const startup = "/web/src/app/startup-screen.test.ts";
  for (const [filename, other] of [["complete-save-schema.test.ts", "complete-save-schema-extra.test.ts"], ["complete-save-schema-extra.test.ts", "complete-save-schema.test.ts"]]) {
    const otherPath = `/web/src/save/${other}`;
    for (const paths of [[], [`/a/save/${filename}`, `/b/save/${filename}`]]) {
      await assert.rejects(runWebTestBatch({ batchIndex: 0, batchCount: 12,
        discover: async () => [...ordinary, startup, otherPath, ...paths] }), /requires exactly one complete-save-schema/);
    }
  }
});

it("浏览器对比度测试独立成批且全部文件恰好执行一次", async () => {
  const browser = "/web/src/mobile/mobile-color-contrast.test.ts";
  const files = [browser, "/web/src/app/startup-screen.test.ts", "/web/src/save/complete-save-schema.test.ts", "/web/src/save/complete-save-schema-extra.test.ts", ...Array.from({ length: 16 }, (_, index) => `/web/src/file-${index}.test.ts`)];
  const all = [];
  const isolated = [];
  for (let index = 0; index < 12; index += 1) {
    await runWebTestBatch({ cpuCount: 8, batchIndex: index, batchCount: 12, discover: async () => files,
      run: async ({ args }) => { all.push(...args.filter((arg) => !arg.startsWith("--"))); if (index === 11) isolated.push(...args.filter((arg) => !arg.startsWith("--"))); } });
  }
  assert.deepEqual(all.sort(), [...files].sort());
  assert.deepEqual(isolated, [browser]);
});

it("完整回归要求唯一浏览器对比度测试", async () => {
  const base = ["/web/src/app/startup-screen.test.ts", "/web/src/save/complete-save-schema.test.ts", "/web/src/save/complete-save-schema-extra.test.ts", ...Array.from({ length: 12 }, (_, index) => `/web/src/file-${index}.test.ts`)];
  for (const paths of [[], ["/a/mobile/mobile-color-contrast.test.ts", "/b/mobile/mobile-color-contrast.test.ts"]]) {
    await assert.rejects(runWebTestBatch({ batchIndex: 0, batchCount: 12, discover: async () => [...base, ...paths] }), /exactly one mobile-color-contrast/);
  }
});

it("完整回归将 TypeScript 检查独立成批且每个文件恰好执行一次", async () => {
  const compiler = "/web/src/app/workspace-grid-types.test.ts";
  const files = [compiler, "/web/src/app/startup-screen.test.ts", "/web/src/save/complete-save-schema.test.ts", "/web/src/save/complete-save-schema-extra.test.ts", "/web/src/mobile/mobile-color-contrast.test.ts", ...Array.from({ length: 16 }, (_, index) => `/web/src/file-${index}.test.ts`)];
  const seen = [];
  for (let index = 0; index < 13; index += 1) {
    const batch = [];
    await runWebTestBatch({ cpuCount: 8, batchIndex: index, batchCount: 13, discover: async () => files,
      run: async ({ args }) => { batch.push(...args.filter((arg) => !arg.startsWith("--"))); } });
    if (index === 12) assert.deepEqual(batch, [compiler]);
    seen.push(...batch);
  }
  assert.deepEqual(seen.sort(), [...files].sort());
  await assert.rejects(runWebTestBatch({ batchIndex: 12, batchCount: 13, discover: async () => files.filter((file) => file !== compiler) }), /exactly one workspace-grid-types/);
});

it("排队文件从启动时获得独立十秒期限，受共享长验收期限约束", async () => {
  let clock = 0;
  const calls = [];
  const files = Array.from({ length: 3 }, (_, index) => `/web/${index}.test.ts`);
  await runWebTestBatch({ webRoot: "/web", cpuCount: 1, now: () => clock,
    discover: async () => files, run: async (options) => { calls.push(options); clock += 6000; } });
  assert.equal(calls.length, 3);
  assert.deepEqual(calls.map((call) => call.timeoutMs), [10000, 10000, 10000]);
  assert.deepEqual(calls.map((call) => call.args.at(-1)), files);
  assert.ok(calls.every((call) => call.args.includes("--test-isolation=none")));
});

it("共享期限耗尽时停止队列，失败时取消在途文件", async () => {
  let remaining = 15000;
  const calls = [];
  await assert.rejects(runWebTestBatch({ cpuCount: 1, remainingMs: () => remaining,
    discover: async () => ["a.test.ts", "b.test.ts", "c.test.ts"],
    run: async (options) => { calls.push(options); remaining -= 8000; },
  }), /deadline/);
  assert.deepEqual(calls.map((call) => call.timeoutMs), [10000, 7000]);
});

it("父取消停止排队并等待在途文件清理，子进程继承指定环境", async () => {
  const controller = new AbortController();
  const env = { FIXTURE_ENV: "preserved" };
  const seen = [];
  const started = [];
  let release;
  const ready = new Promise((resolve) => { release = resolve; });
  const batch = runWebTestBatch({ cpuCount: 2, env, signal: controller.signal,
    discover: async () => ["a.test.ts", "b.test.ts", "c.test.ts"],
    run: async (options) => {
      seen.push(options.args.at(-1));
      assert.equal(options.env, env);
      started.push(options);
      if (started.length === 2) release();
      await new Promise((_, reject) => options.signal.addEventListener("abort", () => reject(options.signal.reason), { once: true }));
    },
  });
  await ready;
  controller.abort(new Error("父任务取消"));
  await assert.rejects(batch, /父任务取消/);
  assert.equal(seen.length, 2);
  const alreadyAborted = new AbortController();
  alreadyAborted.abort(new Error("提前取消"));
  await assert.rejects(runWebTestBatch({ cpuCount: 1, signal: alreadyAborted.signal,
    discover: async () => ["a.test.ts"], run: async () => { throw new Error("不应启动"); },
  }), /提前取消/);
});
