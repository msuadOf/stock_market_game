import assert from "node:assert/strict";
import test from "node:test";
import { WasmRestartGate } from "./wasm-restart-gate.ts";

test("已排队恢复重启在catalog失败后失效，微任务drain后仍停机", async () => {
  const gate = new WasmRestartGate();
  const token = gate.begin();
  let running = false;
  gate.schedule(token, () => true, () => { running = true; });
  gate.invalidate();
  await Promise.resolve();
  assert.equal(running, false);
});

test("仅完整baseline通过且handle/generation仍一致才重启", async () => {
  const gate = new WasmRestartGate();
  let current = true;
  let starts = 0;
  gate.schedule(gate.begin(), () => current, () => { starts++; });
  current = false;
  await Promise.resolve();
  assert.equal(starts, 0);
  current = true;
  gate.schedule(gate.begin(), () => current, () => { starts++; });
  await Promise.resolve();
  assert.equal(starts, 1);
});
