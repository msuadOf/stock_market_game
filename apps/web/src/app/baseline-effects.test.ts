import assert from "node:assert/strict";
import test from "node:test";
import { applyBaselineEffects } from "./baseline-effects.ts";

test("同游标刷新保留自动交易和在途日终保存，只安装视图", { timeout: 10000 }, () => {
  const calls: string[] = [];
  const effects = {
    replaceSession: () => calls.push("replace"),
    installView: () => calls.push("view"),
    refreshOrders: () => calls.push("orders"),
  };
  applyBaselineEffects(true, effects);
  assert.deepEqual(calls, ["view", "orders"]);
  calls.length = 0;
  applyBaselineEffects(false, effects);
  assert.deepEqual(calls, ["replace", "view", "orders"]);
});

test("同游标刷新不取消在途日终保存", { timeout: 10000 }, async () => {
  const { DayEndPersistence } = await import("../save/day-end-persistence.ts");
  const queue = new DayEndPersistence();
  queue.install("1");
  let resolve!: (value: { day: number }) => void;
  const capture = new Promise<{ day: number }>(done => { resolve = done; });
  const writes: number[] = [];
  const pending = queue.completed("1", capture, async slot => { assert.deepEqual(slot, { day: 1 }); writes.push((slot as { day: number }).day); });
  applyBaselineEffects(true, { replaceSession: () => queue.install("1"), installView() {}, refreshOrders() {} });
  resolve({ day: 1 });
  assert.equal(await pending, true);
  assert.deepEqual(writes, [1]);
});
