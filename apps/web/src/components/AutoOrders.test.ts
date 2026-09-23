import assert from "node:assert/strict";
import test from "node:test";
import { AutoOrderManager } from "./auto-order-manager.ts";
function continuousPoint(price: number, tick = 1) {
  return { code: "600101", tick, phase: "Continuous" as const, last_price: price, cumulative_volume: 0, bids: [], asks: [] };
}

function addSellTrigger(manager: AutoOrderManager) {
  return manager.add({
    code: "600101",
    type: "sellTrigger",
    triggerPrice: 1_000,
    qty: 100,
    side: "Sell",
    enabled: true,
  });
}

test("条件单使用 manager 返回的唯一 ID，停用后不会触发", async () => {
  const submitted: unknown[] = [];
  const manager = new AutoOrderManager((intent) => { submitted.push(intent); });
  const order = addSellTrigger(manager);

  manager.toggle(order.id);
  await manager.consumePoints([continuousPoint(1_000)]);

  assert.equal(submitted.length, 0);
  assert.equal(manager.list()[0]?.enabled, false);
});

test("删除条件单后满足价格也不会提交", async () => {
  const submitted: unknown[] = [];
  const manager = new AutoOrderManager((intent) => { submitted.push(intent); });
  const order = addSellTrigger(manager);

  manager.remove(order.id);
  await manager.consumePoints([continuousPoint(1_000)]);

  assert.equal(submitted.length, 0);
  assert.deepEqual(manager.list(), []);
});

test("条件单正常触发一次并把同一 ID 通知 UI", async () => {
  const submitted: unknown[] = [];
  const triggered: string[] = [];
  const manager = new AutoOrderManager(
    (intent) => { submitted.push(intent); },
    (id) => triggered.push(id),
  );
  const order = addSellTrigger(manager);

  await manager.consumePoints([continuousPoint(1_000), continuousPoint(1_001, 2)]);

  assert.equal(submitted.length, 1);
  assert.deepEqual(triggered, [order.id]);
  assert.equal(manager.list()[0]?.triggered, true);
});

test("条件单提交失败时保持未触发并允许下一次行情重试", async () => {
  let attempts = 0;
  const failures: string[] = [];
  const manager = new AutoOrderManager(
    async () => {
      attempts += 1;
      if (attempts === 1) throw new Error("host rejected");
    },
    () => {},
    (id) => failures.push(id),
  );
  const order = addSellTrigger(manager);

  await manager.consumePoints([continuousPoint(1_000)]);
  assert.equal(manager.list()[0]?.triggered, false);
  assert.deepEqual(failures, [order.id]);

  await manager.consumePoints([continuousPoint(1_001, 2)]);
  assert.equal(attempts, 2);
  assert.equal(manager.list()[0]?.triggered, true);
});
