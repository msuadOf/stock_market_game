import assert from "node:assert/strict";
import test from "node:test";
import { aggregateCandles } from "./market-model.ts";
import type { KlinePoint } from "../components/PriceChart.tsx";

function candle(date: string, index: number): KlinePoint {
  return { time: (Date.parse(`${date}T00:00:00Z`) / 1000) as KlinePoint["time"], open: index + 10, high: index + 12, low: index + 9, close: index + 11, volume: 100 };
}

test("自然周跨年沿用周一边界，休市缺口不补成交且未满周保留", { timeout: 10000 }, () => {
  const candles = ["2029-12-28", "2029-12-31", "2030-01-02", "2030-01-04", "2030-01-07"].map(candle);
  const weekly = aggregateCandles(candles, "周K");
  assert.equal(weekly.length, 3);
  assert.deepEqual(weekly.map(({ time, volume }) => [time, volume]), [[candles[0].time, 100], [candles[1].time, 300], [candles[4].time, 100]]);
  assert.deepEqual(weekly[1], { time: candles[1].time, open: 11, high: 15, low: 10, close: 14, volume: 300 });
});

test("自然月把虚拟前史与开局成交按同一个公历月份聚合", { timeout: 10000 }, () => {
  const candles = ["2029-12-28", "2029-12-31", "2030-01-02", "2030-01-03", "2030-02-01"].map(candle);
  const monthly = aggregateCandles(candles, "月K");
  assert.deepEqual(monthly.map(({ volume }) => volume), [200, 200, 100]);
  assert.equal(monthly[1].open, 12);
  assert.equal(monthly[1].close, 14);
});

test("交易日数量模式保留固定5/20根口径而不是自然周月", { timeout: 10000 }, () => {
  const candles = Array.from({ length: 21 }, (_, index) => candle(`2030-01-${String(index + 2).padStart(2, "0")}`, index));
  assert.deepEqual(aggregateCandles(candles, "周K", "trading-days").map(({ volume }) => volume), [500, 500, 500, 500, 100]);
  assert.deepEqual(aggregateCandles(candles, "月K", "trading-days").map(({ volume }) => volume), [2000, 100]);
});

test("聚合拒绝乱序、非日期秒数及成交量溢出", { timeout: 10000 }, () => {
  const first = candle("2030-01-02", 0);
  assert.throws(() => aggregateCandles([first, first], "周K"), /严格递增/);
  assert.throws(() => aggregateCandles([{ ...first, time: (first.time + 1) as KlinePoint["time"] }], "周K"), /UTC零点/);
  assert.throws(() => aggregateCandles([{ ...first, volume: Number.MAX_SAFE_INTEGER }, candle("2030-01-03", 1)], "周K"), /成交量/);
});

test("聚合原始报价保留精确高低价而不依据图表浮点误选", { timeout: 10000 }, () => {
  const first = { ...candle("2030-01-02", 0), high: 90071992547410, low: 90071992547410, rawPrices: { open: "9007199254740993", high: "9007199254740993", low: "9007199254740993", close: "9007199254740993" } };
  const second = { ...candle("2030-01-03", 1), high: first.high, low: first.low, rawPrices: { open: "9007199254740993", high: "9007199254740994", low: "9007199254740992", close: "9007199254740993" } };
  assert.deepEqual(aggregateCandles([first, second], "周K")[0].rawPrices, { open: "9007199254740993", high: "9007199254740994", low: "9007199254740992", close: "9007199254740993" });
});
