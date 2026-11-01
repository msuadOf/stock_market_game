import assert from "node:assert/strict";
import test from "node:test";
import { parseWasmRuntimeOptions } from "./wasm-runtime-options.ts";
import { drain, fixture } from "./worker-host-w1.test-support.ts";

const WAIT_SETTINGS = { spinUs: 500, quietUs: 50_000, fineUs: 500, coarseUs: 500 };

for (const [search, expected] of [
  ["", { runtimeOverride: true, waitSettings: WAIT_SETTINGS }],
  ["?theme=dark", { runtimeOverride: true, waitSettings: WAIT_SETTINGS }],
  ["?wasmRuntime=1", { runtimeOverride: true, waitSettings: WAIT_SETTINGS }],
] as const) {
  test(`WASM runtime query ${search || "缺省"} 解析为显式契约`, { timeout: 10000 }, () => {
    assert.deepEqual(parseWasmRuntimeOptions(search), expected);
  });
}

for (const search of ["?wasmRuntime=0", "?wasmRuntime=1&wasmRuntime=1", "?wasmRuntime=0&wasmRuntime=1", "?wasmRuntime=true", "?wasmRuntime=2", "?wasmRuntime="]) {
  test(`拒绝不明确的 WASM runtime query ${search}`, { timeout: 10000 }, () => {
    assert.throws(() => parseWasmRuntimeOptions(search), /wasmRuntime/);
  });
}

for (const [search, expected] of [
  ["", { runtimeOverride: true, waitSettings: WAIT_SETTINGS }],
  ["?wasmRuntime=1", { runtimeOverride: true, waitSettings: WAIT_SETTINGS }],
] as const) {
  test(`Worker init 对 ${search || "缺省"} query 保留真实 payload 语义`, { timeout: 10000 }, async () => {
    const f = fixture({ initError: new Error("测试在真实 init 传输后停止 Worker 初始化") });
    try {
      const creating = f.launch({
        onSpawnFailure() {},
        ...parseWasmRuntimeOptions(search),
      });
      const rejected = assert.rejects(creating);
      await drain();
      await rejected;
      const init = f.workers[0]?.sent.find((entry) => entry.type === "init");
      assert.ok(init);
      for (const [key, value] of Object.entries(expected)) assert.deepEqual(init[key], value);

      assert.equal(f.workers[0]?.terminations, 1, "bootstrap 失败必须真实终止已创建的 Worker");
    } finally {
      await f.cleanup();
    }
  });
}

 test("直接创建 WorkerHost 缺省也发送 runtime 参数", { timeout: 10000 }, async () => {
  const f = fixture({ initError: new Error("测试初始化") });
  try {
    const rejected = assert.rejects(f.launch({ onSpawnFailure() {} }));
    await drain(); await rejected;
    const init = f.workers[0]?.sent.find((entry) => entry.type === "init");
    assert.equal(init?.runtimeOverride, true);
    assert.deepEqual(init?.waitSettings, WAIT_SETTINGS);
  } finally { await f.cleanup(); }
});

test("直接创建 WorkerHost 不静默忽略未配对 waitSettings", { timeout: 10000 }, async () => {
  const f = fixture();
  try {
    await assert.rejects(f.launch({ onSpawnFailure() {}, waitSettings: WAIT_SETTINGS }), /waitSettings/);
    assert.equal(f.workers.length, 0);
  } finally { await f.cleanup(); }
});

test("WorkerHost 拒绝关闭 runtime 且不分配 Worker", { timeout: 10000 }, async () => {
  const f = fixture();
  try {
    await assert.rejects(f.launch({ onSpawnFailure() {}, runtimeOverride: false }), /runtime/);
    assert.equal(f.workers.length, 0);
  } finally { await f.cleanup(); }
});
