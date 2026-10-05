import assert from "node:assert/strict";
import { test } from "node:test";
import { exactMovingAverage } from "./moving-average.ts";
import type { KlinePoint } from "./PriceChart.tsx";
function candles(closes: string[]): KlinePoint[] { return closes.map((close, index) => ({ time: index as KlinePoint["time"], open: 0, high: 0, low: 0, close: 0, rawPrices: { open: close, high: close, low: close, close } })); }
test("MA按精确分累计并仅在完整窗口后出值，展示值不参与计算", { timeout: 10000 }, () => {
  assert.deepEqual(exactMovingAverage(candles(["100", "200", "300", "400"]), 3), [null, null, 2, 3]);
  assert.deepEqual(exactMovingAverage(candles(["101", "102"]), 2), [null, 1.015]);
  assert.deepEqual(exactMovingAverage(candles(["9223372036854775807", "9223372036854775807"]), 2), [null, Number(92233720368547758n) + 0.07]);
});
test("不存在精确分值及非法period显式拒绝，不用近似图表值补齐", { timeout: 10000 }, () => {
  const raw = candles(["100"]); delete raw[0].rawPrices;
  assert.throws(() => exactMovingAverage(raw, 1));
  for (const period of [0, -1, 1.5, NaN, Infinity]) assert.throws(() => exactMovingAverage([], period));
});
