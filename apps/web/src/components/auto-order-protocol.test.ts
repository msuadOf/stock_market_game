import assert from "node:assert/strict";
import test from "node:test";
import { AutoOrderManager } from "./auto-order-manager.ts";

test("Given normalized continuous points, when an automatic order crosses its trigger, then it submits exactly once", async () => {
  const submitted: unknown[] = [];
  const manager = new AutoOrderManager((intent) => { submitted.push(intent); });
  manager.add({
    code: "600101",
    type: "sellTrigger",
    triggerPrice: 1_000,
    qty: 100,
    side: "Sell",
    enabled: true,
  });

  await manager.consumePoints([
    { code: "600101", tick: 1, phase: "Continuous", last_price: 1_000, cumulative_volume: 100, bids: [], asks: [] },
    { code: "600101", tick: 2, phase: "Continuous", last_price: 1_001, cumulative_volume: 100, bids: [], asks: [] },
  ]);

  assert.equal(submitted.length, 1);
});
