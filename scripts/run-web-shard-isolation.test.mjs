import assert from "node:assert/strict";
import { test } from "node:test";
import { buildWebTestShardPolicy, runWebTestBatch } from "./run-web-tests.mjs";

test("isolates the TypeScript compiler and SSR suite instead of loading it with unrelated fixtures", () => {
  for (const separator of ["/", "\\"]) {
    const compilerSuite = ["web", "src", "app", "workspace-grid.test.ts"].join(separator);
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
  const files = ["/web/src/app/workspace-grid.test.ts", "/web/src/ordinary.test.ts"];
  const policy = buildWebTestShardPolicy(1, files);
  assert.equal(policy.shard_count, 1);
  assert.deepEqual(policy.shards, [files]);
  assert.equal(policy.aggregate_process_budget, 1);
});

test("records real shard counts before execution so a timeout does not erase the resource plan", async () => {
  const logs = [];
  const files = ["/web/src/app/workspace-grid.test.ts", "/web/src/ordinary.test.ts"];
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
