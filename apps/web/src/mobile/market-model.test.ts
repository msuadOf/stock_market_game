import assert from "node:assert/strict";
import test from "node:test";
import type { EngineEvent } from "../types/engine";
import { MinutePointCollector, formatTradingMinute, tradingDayProgress } from "./market-model.ts";

test("60 个世界 tick 聚合成一根一分钟量柱，跨事件批次持续累计", () => {
  const collector = new MinutePointCollector("600460");
  const firstSecond: EngineEvent[] = [
    { Trade: { seq: 1, code: "600460", price: 3030, qty: 200, maker: 1, taker: 2 } },
    { PriceTick: { seq: 2, code: "600460", last_price: 3030 } },
  ];
  const remainingSeconds: EngineEvent[] = Array.from({ length: 59 }, (_, index) => ({
    PriceTick: { seq: index + 3, code: "600460", last_price: 3031 },
  }));

  assert.deepEqual(collector.collect(firstSecond), []);
  assert.deepEqual(collector.collect(remainingSeconds), [
    { time: 61, value: 30.31, volume: 200, buy: true },
  ]);
});

test("其它股票的成交量不会混入当前股票", () => {
  const collector = new MinutePointCollector("600460", 1);
  const events: EngineEvent[] = [
    { Trade: { seq: 1, code: "000001", price: 1000, qty: 900, maker: 1, taker: 2 } },
    { PriceTick: { seq: 2, code: "000001", last_price: 1000 } },
    { PriceTick: { seq: 3, code: "600460", last_price: 3034 } },
  ];

  assert.deepEqual(collector.collect(events), [
    { time: 3, value: 30.34, volume: 0, buy: false },
  ]);
});

test("一分钟内最后成交价决定量柱方向", () => {
  const collector = new MinutePointCollector("600460", 1);
  const events: EngineEvent[] = [
    { Trade: { seq: 7, code: "600460", price: 3033, qty: 200, maker: 1, taker: 2 } },
    { Trade: { seq: 8, code: "600460", price: 3034, qty: 300, maker: 3, taker: 4 } },
    { PriceTick: { seq: 9, code: "600460", last_price: 3034 } },
  ];

  assert.deepEqual(collector.collect(events), [
    { time: 9, value: 30.34, volume: 500, buy: true },
  ]);
});

test("交易进度限制在 0 到 1，时间跨过午间休市", () => {
  assert.equal(tradingDayProgress(0, 240), 0);
  assert.equal(tradingDayProgress(178, 240), 178 / 240);
  assert.equal(tradingDayProgress(999, 240), 1);
  assert.equal(formatTradingMinute(0), "09:30");
  assert.equal(formatTradingMinute(119), "11:29");
  assert.equal(formatTradingMinute(120), "13:00");
  assert.equal(formatTradingMinute(239), "14:59");
});
