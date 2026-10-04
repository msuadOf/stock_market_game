import assert from "node:assert/strict";
import test from "node:test";
import { CANDLE_AGGREGATION_STORAGE_KEY, loadCandleAggregationBasis, saveCandleAggregationBasis } from "./candle-aggregation.ts";

test("聚合口径默认自然日历并可保存交易日数量偏好", { timeout: 10000 }, () => {
  const entries = new Map<string, string>();
  const storage = { getItem(key: string) { return entries.has(key) ? entries.get(key)! : null; }, setItem(key: string, value: string) { entries.set(key, value); } };
  assert.equal(loadCandleAggregationBasis(storage), "calendar");
  saveCandleAggregationBasis(storage, "trading-days");
  assert.equal(loadCandleAggregationBasis(storage), "trading-days");
  entries.set(CANDLE_AGGREGATION_STORAGE_KEY, "legacy");
  assert.throws(() => loadCandleAggregationBasis(storage), /聚合设置/);
});

test("设置存储故障不得静默改为默认口径", { timeout: 10000 }, () => {
  const storage = { getItem(): never { throw new Error("禁止读取"); }, setItem(): never { throw new Error("禁止写入"); } };
  assert.throws(() => loadCandleAggregationBasis(storage), /禁止读取/);
  assert.throws(() => saveCandleAggregationBasis(storage, "calendar"), /禁止写入/);
});
