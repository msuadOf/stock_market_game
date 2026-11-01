import assert from "node:assert/strict";
import { test } from "node:test";
import { buildWebTestShardPolicy, runWebTestBatch } from "./run-web-tests.mjs";

test("isolates the TypeScript compiler and SSR suite instead of loading it with unrelated fixtures", () => {
  for (const separator of ["/", "\\"]) {
    const compilerSuite = ["web", "src", "app", "workspace-grid-types.test.ts"].join(separator);
    const ordinaryFiles = Array.from({ length: 100 }, (_, index) => `/web/src/ordinary-${index}.test.ts`);
    const files = [ordinaryFiles[0], compilerSuite, ...ordinaryFiles.slice(1)];
    const policy = buildWebTestShardPolicy(4, files);
    assert.equal(policy.shard_count, 4);
    assert.deepEqual(policy.shards[0], [compilerSuite]);
    assert.deepEqual(policy.shards.flat().sort(), [...files].sort());
    assert.deepEqual(policy.shards.slice(1).map((shard) => shard.length), [34, 33, 33]);
    assert.equal(policy.aggregate_process_budget, 4);
  }
});

test("single-CPU execution never creates an extra process or drops the compiler suite", () => {
  const files = ["/web/src/app/workspace-grid-types.test.ts", "/web/src/ordinary.test.ts"];
  const policy = buildWebTestShardPolicy(1, files);
  assert.equal(policy.shard_count, 1);
  assert.deepEqual(policy.shards, [files]);
  assert.equal(policy.aggregate_process_budget, 1);
});

test("records real shard counts before execution so a timeout does not erase the resource plan", async () => {
  const logs = [];
  const files = ["/web/src/app/workspace-grid-types.test.ts", "/web/src/ordinary.test.ts"];
  await runWebTestBatch({
    webRoot: "/web", cpuCount: 2, discover: async () => files,
    log: (line) => logs.push(JSON.parse(line)),
    run: async () => {
      assert.equal(logs[0].status, "started");
      assert.equal(logs[0].available_cpu_count, 2);
      assert.deepEqual(logs[0].shard_file_counts, [1, 1]);
    },
  });
  assert.equal(logs.at(-1).file_count, 2);
});

test("限制文件进程并发，并在启动前让事件循环处理已有任务", async () => {
  const files = Array.from({ length: 32 }, (_, index) => `/web/src/${index}.test.ts`);
  const policy = buildWebTestShardPolicy(64, files);
  assert.equal(policy.max_concurrent_processes, 8);
  let active = 0;
  let peak = 0;
  let yielded = false;
  setImmediate(() => { yielded = true; });
  const result = await runWebTestBatch({
    webRoot: "/web", cpuCount: 64, discover: async () => files,
    run: async () => {
      assert.equal(yielded, true);
      active += 1;
      peak = Math.max(peak, active);
      await new Promise((resolve) => setImmediate(resolve));
      active -= 1;
    },
  });
  assert.equal(peak, 8);
  assert.equal(result.files.length, files.length);
});


test("同步完成的文件也在每次启动前让出事件循环", async () => {
  let tick = 0;
  let running = true;
  const advance = () => { tick += 1; if (running) setImmediate(advance); };
  setImmediate(advance);
  const ticks = [];
  try {
    await runWebTestBatch({
      cpuCount: 1, discover: async () => ["a.test.ts", "b.test.ts", "c.test.ts"],
      run: async () => { ticks.push(tick); },
    });
  } finally { running = false; }
  assert.equal(ticks.length, 3);
  assert.ok(ticks[0] > 0 && ticks[1] > ticks[0] && ticks[2] > ticks[1]);
});
