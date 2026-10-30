import assert from "node:assert/strict";
import test from "node:test";
import { parseWasmRuntimeOptions } from "./wasm-runtime-options.ts";
import { drain, fixture } from "./worker-host-w1.test-support.ts";

const WAIT_SETTINGS = { spinUs: 500, quietUs: 50_000, fineUs: 500, coarseUs: 500 };

for (const [search, expected] of [
  ["", {}],
  ["?theme=dark", {}],
  ["?wasmRuntime=0", { runtimeOverride: false }],
  ["?wasmRuntime=1", { runtimeOverride: true, waitSettings: WAIT_SETTINGS }],
] as const) {
  test(`WASM runtime query ${search || "缺省"} 解析为显式契约`, { timeout: 10000 }, () => {
    assert.deepEqual(parseWasmRuntimeOptions(search), expected);
  });
}

for (const search of ["?wasmRuntime=1&wasmRuntime=1", "?wasmRuntime=0&wasmRuntime=1", "?wasmRuntime=true", "?wasmRuntime=2", "?wasmRuntime="]) {
  test(`拒绝不明确的 WASM runtime query ${search}`, { timeout: 10000 }, () => {
    assert.throws(() => parseWasmRuntimeOptions(search), /wasmRuntime/);
  });
}

for (const [search, expected] of [
  ["", {}],
  ["?wasmRuntime=0", { runtimeOverride: false }],
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
      if (search === "" || search === "?wasmRuntime=0") {
        assert.equal(Object.hasOwn(init, "runtimeOverride"), search !== "");
        assert.equal(Object.hasOwn(init, "waitSettings"), false);
      }
      assert.equal(f.workers[0]?.terminations, 1, "bootstrap 失败必须真实终止已创建的 Worker");
    } finally {
      await f.cleanup();
    }
  });
}
