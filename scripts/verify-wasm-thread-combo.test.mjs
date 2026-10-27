import assert from "node:assert/strict";
import { test } from "node:test";
import { main, combineGateResults, LOCKED_WASM_BINDGEN, LOCKED_TOOLCHAIN, LOCKED_WASM_PACK, parseWasmBindgenPin } from "./verify-wasm-thread-combo.mjs";

// R3-2 组合验证入口·纯逻辑合同（重执行面由入口本体在长验收预算内承载）。

test("锁定矩阵常量与设计 §6.3 R3-1 一致", () => {
  assert.equal(LOCKED_TOOLCHAIN, "nightly-2026-09-05");
  assert.equal(LOCKED_WASM_PACK, "wasm-pack 0.13.1");
  assert.equal(LOCKED_WASM_BINDGEN, "0.2.93");
});

test("wasm-bindgen 锁版解析接受精确锁定并拒绝浮动/未锁形态", () => {
  assert.equal(parseWasmBindgenPin('wasm-bindgen = "=0.2.93"\n'), "0.2.93");
  // 浮动版本（^0.2 / "0.2"）与未声明都不满足锁定矩阵——返回 null/异值由门 1 判红。
  assert.equal(parseWasmBindgenPin('wasm-bindgen = "0.2"\n'), null);
  assert.equal(parseWasmBindgenPin('wasm-bindgen = "^0.2.93"\n'), null);
  assert.equal(parseWasmBindgenPin("serde = \"1\"\n"), null);
});

test("组合结果聚合：未登记红门使整体失败，登记红/跳过显式呈现", () => {
  const summary = combineGateResults([
    { gate: "lock-matrix", status: "green", detail: "" },
    { gate: "shared-memory-gate", status: "red", detail: "threads-xform 阻断（已登记）" },
    { gate: "browser-combo", status: "skipped", detail: "阶段 2" },
  ]);
  assert.equal(summary.green, false);
  assert.deepEqual(summary.red, ["shared-memory-gate"]);
  assert.deepEqual(summary.skipped, ["browser-combo"]);
  const allGreen = combineGateResults([{ gate: "lock-matrix", status: "green", detail: "" }]);
  assert.equal(allGreen.green, true);
});


test("组合长入口由进程外共享300000ms supervisor驱动且显式jobs", async () => {
  const calls = [];
  await main(["--jobs", "16"], { run: async (options) => { calls.push(options); }, env: {}, nodeExecutable: "/node" });
  assert.equal(calls.length, 1);
  assert.equal(calls[0].timeoutMs, 300000);
  assert.equal(calls[0].cleanupReserveMs, 1000);
  assert.equal(calls[0].command, "/node");
  assert.ok(calls[0].args.includes("--internal-worker"));
  assert.deepEqual(calls[0].args.slice(-2), ["--jobs", "16"]);
  assert.equal(calls[0].env.STOCK_GAME_WASM_COMBO_INTERNAL_WORKER, "1");
  await assert.rejects(main(["--internal-worker"], { env: {} }), /supervisor/);
});
