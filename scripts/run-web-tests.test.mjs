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
  assert.ok(files.length >= 54);
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
  assert.deepEqual(calls.flatMap((call) => call.args.slice(5)).sort(), [...files].sort());
  assert.equal(result.file_count, 3);
  assert.equal(result.shard_count, 2);
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
