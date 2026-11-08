import assert from "node:assert/strict";
import test from "node:test";
import { baseState, dailyCandle, frame, snapshot, tickBatch, timeseries } from "../protocol-test-fixtures.ts";
import { reduceEngineUpdate } from "./reduce.ts";
import { MarketChartProjection } from "../../app/market-chart-projection.ts";

test("累计区间保持连续游标、逐分钟 candle 和事实", { timeout: 10000 }, () => {
  const sample = (tick: number, volume: number, turnover: string) => ({ point: { tick, phase: "Continuous", last_price: "1000", cumulative_volume: volume, bids: [], asks: [] }, candle: { ...dailyCandle(), volume, trade_stats: { turnover_cents: turnover, trade_count: 1 } } });
  const samples = [sample(901, 10, "10000"), sample(961, 20, "20000")];
  const range = { ...frame(961, 0, [0]), tick_from: 1, fact_ticks: [901], timeseries_payload: { ...timeseries(961), continuous_points: { "0": samples[1]!.point }, continuous_samples: { "0": samples } } };
  const result = reduceEngineUpdate(baseState(), "generation-1", tickBatch([range], snapshot(961, 1)));
  assert.equal(result.state.cursor.tick, 961);
  assert.equal(result.state.intraday[0]!.facts.length, 1);
  assert.deepEqual(result.effects.filter(effect => effect.kind === "automatic-order").map(effect => effect.kind === "automatic-order" && effect.points[0]!.tick), [901, 961]);
  assert.throws(() => reduceEngineUpdate(baseState(), "generation-1", tickBatch([{ ...range, fact_ticks: [] }], snapshot(961, 1))), /fact_ticks/);
  const chart = new MarketChartProjection();
  chart.rebuildHistory(result.state.intraday);
  assert.deepEqual(chart.pricePointsFor("000812").map(p => [p.time, p.volume, p.cumulativeTurnoverCents]), [[0, 10, "10000"], [1, 10, "20000"]]);
  assert.throws(() => reduceEngineUpdate(baseState(), "generation-1", tickBatch([{ ...range, tick_from: 2 }], snapshot(961, 1))), /不连续/);
});

test("相同稳定键允许跨 tick 重用并保持事实与时间对应", { timeout: 10000 }, async () => {
  const { fact } = await import("../protocol-test-fixtures.ts");
  const first = fact(1, 0), second = fact(2, 0);
  const range = { ...frame(2, 0, [0, 0]), tick_from: 1, fact_ticks: [1, 2], facts: [first, second] };
  const result = reduceEngineUpdate(baseState(), "generation-1", tickBatch([range], snapshot(2, 2)));
  assert.deepEqual(result.state.intraday[0]!.factTicks, [1, 2]);
  assert.throws(() => reduceEngineUpdate(baseState(), "generation-1", tickBatch([{ ...range, facts: [second, first], fact_ticks: [2, 1] }], snapshot(2, 2))), /fact_ticks/);
  assert.throws(() => reduceEngineUpdate(baseState(), "generation-1", tickBatch([{ ...range, fact_ticks: [1, 1] }], snapshot(2, 2))), /事实身份/);
});

test("跨 tick 同键成交保留各自原始 tick", { timeout: 10000 }, () => {
  const trade = (seq: number) => ({ key: { phase_rank: 4, entity: { Stock: 0 }, source: "Sealed", local_event_index: 0 }, event: { Trade: { seq, code: 0, price: "1000", qty: 100, maker: "0", taker: "1" } } });
  const range = { ...frame(2, 0, [0, 0]), tick_from: 1, fact_ticks: [1, 2], facts: [trade(1), trade(2)] };
  const result = reduceEngineUpdate(baseState(), "generation-1", tickBatch([range], snapshot(2, 2)));
  assert.deepEqual(result.effects.filter(effect => effect.kind === "trade").map(effect => effect.kind === "trade" && [effect.event.seq, effect.event.tick]), [[1, 1], [2, 2]]);
});


test("同 tick 内重排事实仍识别为重放", { timeout: 10000 }, async () => {
  const { fact } = await import("../protocol-test-fixtures.ts");
  const first = fact(1, 0), second = fact(2, 1);
  const range = { ...frame(2, 0, [0, 0]), tick_from: 1, fact_ticks: [2, 2], facts: [first, second] };
  const result = reduceEngineUpdate(baseState(), "generation-1", tickBatch([range], snapshot(2, 2)));
  const retry = { ...range, facts: [second, first], fact_ticks: [2, 2] };
  assert.equal(reduceEngineUpdate(result.state, "generation-1", tickBatch([retry], snapshot(2, 2))).kind, "exact-retry");
});
