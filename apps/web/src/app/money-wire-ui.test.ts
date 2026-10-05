import assert from "node:assert/strict";
import test from "node:test";
import { valueHeldPosition } from "./position-valuation.ts";
import { AutoOrderManager } from "../components/auto-order-manager.ts";
import { normalizePlayerWorkingOrders } from "../host/player-working-orders.ts";
import { MarketChartProjection } from "./market-chart-projection.ts";

test("Money wire：持仓金额超过 Number 安全范围仍精确到分", { timeout: 10000 }, () => {
  assert.deepEqual(valueHeldPosition({ qty: 2, t1_locked: 0, invested_cents: "18014398509481986", recovered_cents: "0" }, "9007199254740995"), {
    avgCost: "9007199254740993", marketValue: "18014398509481990", pnl: "4",
  });
});

test("Money wire：条件单价格不能按字符串字典序比较", { timeout: 10000 }, async () => {
  const submitted: unknown[] = [];
  const manager = new AutoOrderManager((intent) => { submitted.push(intent); });
  manager.add({ code: "600101", type: "stopProfit", triggerPrice: "99", qty: 100, side: "Sell", enabled: true });
  await manager.consumePoints([{ code: "600101", tick: 1, phase: "Continuous", last_price: "100", cumulative_volume: 0, bids: [], asks: [] }]);
  assert.equal(submitted.length, 1);
});

test("Money wire：活动委托严格拒绝 Number 并保留大金额原值", { timeout: 10000 }, () => {
  const order = { owner: "0", id: 1, code: "600101", side: "Buy", price: "9007199254740993", remainingQty: 100, venue: "continuous", frozen: "cash" };
  assert.equal(normalizePlayerWorkingOrders([order])[0]?.price, "9007199254740993");
  assert.throws(() => normalizePlayerWorkingOrders([{ ...order, price: 1000 }]), /十进制整数分字符串/);
});

test("Money wire：图表坐标相同不能吞掉原始 OHLC 一分变化", { timeout: 10000 }, () => {
  const projection = new MarketChartProjection();
  const candle = { time: 1893456000, open: "9007199254740992", high: "9007199254740993", low: "9007199254740992", close: "9007199254740992", volume: 0, trade_stats: null };
  projection.replaceActiveCandles({ "600101": candle });
  const before = projection.activeCandles()["600101"];
  projection.replaceActiveCandles({ "600101": { ...candle, close: "9007199254740993" } });
  const after = projection.activeCandles()["600101"];
  assert.equal(before.close, after.close);
  assert.notEqual(before, after);
  assert.equal(after.rawPrices?.close, "9007199254740993");
});
