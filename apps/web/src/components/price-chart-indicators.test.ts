import assert from "node:assert/strict";
import test from "node:test";
import type { UTCTimestamp } from "lightweight-charts";
import type { IndicatorResults } from "./indicator-results.ts";
import { buildPriceChartIndicatorSource, priceChartIndicatorData, priceChartVolumeData } from "./price-chart-indicators.ts";

const intraday = [{ time: 1, value: 9, volume: 100, buy: true }, { time: 2, value: 8, volume: 200, buy: false }];
const daily = [
  { time: 100, open: 10, high: 13, low: 9, close: 12, volume: 1000 },
  { time: 200, open: 12, high: 15, low: 11, close: 14, volume: 2000 },
  { time: 300, open: 14, high: 16, low: 10, close: 11, volume: 3000 },
].map((candle) => ({ ...candle, time: candle.time as UTCTimestamp }));
const results: IndicatorResults = {
  macd: { dif: [1, 2, 3], dea: [2, 1, 4], histogram: [-2, 2, -2] },
  priceKdj: { k: [8, 8, 8], d: [9, 9, 9], j: [6, 6, 6] },
  candleKdj: { k: [20, 30, 40], d: [10, 20, 30], j: [40, 50, 60] },
};

test("daily indicators use all authoritative closes and OHLC, never intraday prices", () => {
  const source = buildPriceChartIndicatorSource("日K", intraday, daily);
  assert.deepEqual(source.input, { prices: [12, 14, 11], candles: [
    { high: 13, low: 9, close: 12 }, { high: 15, low: 11, close: 14 }, { high: 16, low: 10, close: 11 },
  ] });
  const plotted = priceChartIndicatorData(source, results, 2);
  assert.deepEqual(plotted.dif, [{ time: 200, value: 2 }, { time: 300, value: 3 }]);
  assert.deepEqual(plotted.k, [{ time: 200, value: 30 }, { time: 300, value: 40 }]);
  assert.deepEqual(plotted.histogram.map(({ time, value }) => ({ time, value })), [{ time: 200, value: 2 }, { time: 300, value: -2 }]);
  assert.deepEqual(priceChartVolumeData(source, 2).map(({ time, value }) => ({ time, value })), [{ time: 200, value: 20 }, { time: 300, value: 30 }]);
});

test("intraday indicators and volumes keep the intraday timeline and close-only KDJ", () => {
  const source = buildPriceChartIndicatorSource("分时", intraday, daily);
  assert.deepEqual(source.input, { prices: [9, 8] });
  const twoPoints: IndicatorResults = {
    macd: { dif: [1, 2], dea: [2, 1], histogram: [-2, 2] },
    priceKdj: { k: [8, 9], d: [7, 8], j: [10, 11] },
    candleKdj: { k: [], d: [], j: [] },
  };
  assert.deepEqual(priceChartIndicatorData(source, twoPoints, 1).k, [{ time: 1, value: 8 }, { time: 2, value: 9 }]);
  assert.deepEqual(priceChartVolumeData(source, 1).map(({ time, value }) => ({ time, value })), [{ time: 1, value: 1 }, { time: 2, value: 2 }]);
});

test("empty daily history does not substitute intraday candles and invalid windows fail explicitly", () => {
  const source = buildPriceChartIndicatorSource("日K", intraday, []);
  assert.deepEqual(source.input, { prices: [], candles: [] });
  assert.deepEqual(priceChartVolumeData(source, 20), []);
  assert.throws(() => priceChartIndicatorData(source, results, 20), /长度/);
  assert.throws(() => priceChartVolumeData(source, 0), /窗口/);
});
