import assert from "node:assert/strict";
import { test } from "node:test";
import * as model from "./market-model.ts";
import type { KlinePoint } from "../components/PriceChart.tsx";
import type { IndicatorResultState } from "../components/useIndicatorResults.ts";
const candle = (i: number): KlinePoint => ({ time: i as KlinePoint["time"], open: i + 10, close: i + 11, high: i + 12, low: i + 9, volume: i * 100 });
const pending: IndicatorResultState = { kind: "pending" };

test("K 线空与单根投影不填造行情，容量大于历史时保留固定槽宽", () => {
  assert.equal(typeof model.MobileKlineProjection, "function", "K 线投影必须提供共同窗口与几何行为 owner");
  const empty = model.MobileKlineProjection.fromInputs([], { capacity: 72, offsetFromEnd: 0 }, pending);
  assert.equal(empty.latestSignature, "empty");
  assert.deepEqual(empty.visibleCandles, []);
  assert.deepEqual(empty.volumeMarks(), []);
  assert.equal(empty.movingAverageLine(5), "");
  const p = model.MobileKlineProjection.fromInputs([candle(0)], { capacity: 72, offsetFromEnd: 50 }, pending);
  assert.equal(p.visibleWindow.offsetFromEnd, 0);
  assert.deepEqual(p.slotFor(0), model.chartSlotGeometry(0, 1, 390, 72));
  const shape = p.candleBodyAndWick(0);
  assert.equal(shape.body.top, p.priceY(11));
  assert.equal(shape.body.bottom, p.priceY(10));
  assert.deepEqual(shape.wick.upper, { start: p.priceY(12), end: p.priceY(11) });
  assert.deepEqual(shape.wick.lower, { start: p.priceY(10), end: p.priceY(9) });
  assert.equal(p.volumeMarks()[0].height, 1);
});

test("MA 与 Rust KDJ 先覆盖完整历史再截取窗口，三个图共享槽位", () => {
  const candles = Array.from({ length: 8 }, (_, i) => candle(i));
  const kdj = { k: candles.map((_, i) => i * 10), d: candles.map((_, i) => i * 9), j: candles.map((_, i) => i * 20 - 30) };
  const result: IndicatorResultState = { kind: "ready", value: { macd: { dif: [], dea: [], histogram: [] }, priceKdj: { k: [], d: [], j: [] }, candleKdj: kdj } };
  const before = structuredClone({ candles, result });
  const p = model.MobileKlineProjection.fromInputs(candles, { capacity: 3, offsetFromEnd: 1 }, result);
  assert.deepEqual(p.visibleCandles.map(c => c.time), [4, 5, 6]);
  assert.deepEqual(p.movingAverage(5), [13, 14, 15]);
  assert.deepEqual(p.kdj, { k: [40, 50, 60], d: [36, 45, 54], j: [50, 70, 90] });
  assert.equal(p.latestSignature, "7:17:19:16:18:700");
  assert.equal(p.movingAverageLine(5).split(" ")[0], `${p.slotFor(0).center},${p.priceY(13)}`);
  assert.equal(p.indicatorLine(p.kdj!.k).split(" ")[0], `${p.slotFor(0).center},${68 - 40 / 100 * 64}`);
  assert.equal(p.volumeMarks()[0].slot.center, p.slotFor(0).center);
  assert.deepEqual({ candles, result }, before);
});

test("KDJ 未就绪或失败时不制造指标，ready 时域包含 J 极值", () => {
  for (const state of [pending, { kind: "error", message: "错误详情" }, { kind: "unavailable" }] as IndicatorResultState[]) {
    assert.equal(model.MobileKlineProjection.fromInputs([candle(0)], { capacity: 72, offsetFromEnd: 0 }, state).kdj, null);
  }
  const state: IndicatorResultState = { kind: "ready", value: { macd: { dif: [], dea: [], histogram: [] }, priceKdj: { k: [], d: [], j: [] }, candleKdj: { k: [50, 50], d: [50, 50], j: [-50, 150] } } };
  const p = model.MobileKlineProjection.fromInputs([candle(0), candle(1)], { capacity: 72, offsetFromEnd: 0 }, state);
  assert.equal(p.indicatorLine(p.kdj!.j), `${p.slotFor(0).center},68 ${p.slotFor(1).center},4`);
});

test("周月 K 沿用 5/20 游戏交易日聚合，offset 超界归一与原始 OHLCV 保持不变", () => {
  const candles = Array.from({ length: 21 }, (_, i) => candle(i));
  const before = structuredClone(candles);
  for (const period of ["周K", "月K"] as const) {
    const aggregated = model.aggregateCandles(candles, period);
    const p = model.MobileKlineProjection.fromInputs(aggregated, { capacity: 2, offsetFromEnd: 99 }, pending);
    assert.equal(aggregated.length, period === "周K" ? 5 : 2);
    assert.equal(p.visibleWindow.offsetFromEnd, p.visibleWindow.maxOffset);
    assert.equal(p.visibleCandles[0].open, 10);
    assert.equal(p.visibleCandles[0].close, period === "周K" ? 15 : 30);
  }
  assert.deepEqual(candles, before);
});
