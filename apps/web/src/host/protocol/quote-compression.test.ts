import assert from "node:assert/strict";
import test from "node:test";
import { baseState, dailyCandle, frame, snapshot, tickBatch, timeseries } from "../protocol-test-fixtures.ts";
import { reduceEngineUpdate } from "./reduce.ts";
import { MarketChartProjection } from "../../app/market-chart-projection.ts";
import { AutoOrderManager } from "../../components/auto-order-manager.ts";

const sample = (tick: number, price: string, volume: number, turnover: string) => ({ point: { tick, phase: "Continuous", last_price: price, cumulative_volume: volume, bids: [], asks: [] }, candle: { ...dailyCandle(), close: price, volume, trade_stats: { turnover_cents: turnover, trade_count: 1 } } });
function compressed() {
  const samples = [sample(902, "900", 100, "100000"), sample(961, "1100", 200, "210000")];
  return {
    ...frame(961, 0, []), tick_from: 1, seq_to: 4,
    facts: [{ key: { phase_rank: 4, entity: { Stock: 0 }, source: "Sealed", local_event_index: 0 }, event: { Trade: { seq: 2, code: 0, price: "1200", qty: 100, maker: "0", taker: "1" } } }], fact_ticks: [901],
    quote_coverage: [{ seq_from: 0, seq_to: 1 }, { seq_from: 2, seq_to: 4 }],
    price_observations: [{ code: 0, seq: 1, tick: 901, last_price: "1200" }, { code: 0, seq: 3, tick: 902, last_price: "900" }, { code: 0, seq: 4, tick: 961, last_price: "1100" }],
    timeseries_payload: { ...timeseries(961), continuous_points: { "0": samples[1]!.point }, continuous_samples: { "0": samples } },
  };
}

test("报价压缩保留逐 seq 条件单观察和分钟量额", { timeout: 10000 }, async () => {
  const range = compressed();
  const result = reduceEngineUpdate(baseState(), "generation-1", tickBatch([range], snapshot(961, 4)));
  assert.equal(result.state.cursor.seq, 4);
  assert.equal(result.state.intraday[0]!.facts.length, 1);
  assert.deepEqual(result.effects.filter(effect => effect.kind === "trade").map(effect => effect.kind === "trade" && [effect.event.seq, effect.event.tick]), [[2, 901]]);
  const observed = result.effects.flatMap(effect => effect.kind === "automatic-order" ? effect.points : []);
  assert.deepEqual(observed, range.price_observations.map(({ code: _code, ...point }) => ({ code: "000812", ...point })));
  const submitted: unknown[] = [];
  const manager = new AutoOrderManager(intent => { submitted.push(intent); });
  manager.add({ code: "000812", type: "sellTrigger", triggerPrice: "1150", qty: 100, side: "Sell", enabled: true });
  manager.add({ code: "000812", type: "buyTrigger", triggerPrice: "950", qty: 100, side: "Buy", enabled: true });
  await manager.consumePoints(observed);
  assert.deepEqual(submitted, [
    { PlaceLimit: { code: "000812", side: "Sell", price: { Fixed: "1200" }, qty: 100 } },
    { PlaceLimit: { code: "000812", side: "Buy", price: { Fixed: "900" }, qty: 100 } },
  ]);
  const chart = new MarketChartProjection();
  chart.rebuildHistory(result.state.intraday);
  assert.deepEqual(chart.pricePointsFor("000812").map(point => [point.time, point.value, point.volume, point.cumulativeVolumeShares, point.cumulativeTurnoverCents]), [[0, 9, 100, 100, "100000"], [1, 11, 100, 200, "210000"]]);
  assert.equal(reduceEngineUpdate(result.state, "generation-1", tickBatch([range], snapshot(961, 4))).kind, "exact-retry");
  const changed = [
    ...[{ last_price: "1201" }, { tick: 900 }, { code: 1 }].map(change => ({ ...range, price_observations: range.price_observations.map((point, index) => index === 0 ? { ...point, ...change } : point) })),
    { ...range, timeseries_payload: { ...range.timeseries_payload, continuous_samples: { "0": range.timeseries_payload.continuous_samples["0"].map((entry, index) => index === 0 ? { ...entry, candle: { ...entry.candle, trade_stats: { turnover_cents: "100001", trade_count: 1 } } } : entry) } } },
  ];
  for (const value of changed) assert.throws(() => reduceEngineUpdate(result.state, "generation-1", tickBatch([value], snapshot(961, 4))), /不同规范更新/);
});

test("报价范围与保留事实及价格观察必须一一精确覆盖", { timeout: 10000 }, () => {
  const valid = compressed();
  const invalid = [
    { ...valid, price_observations: undefined },
    { ...valid, quote_coverage: undefined },
    { ...valid, quote_coverage: [{ seq_from: 0, seq_to: 4 }] },
    { ...valid, quote_coverage: [{ seq_from: 0, seq_to: 1 }, { seq_from: 3, seq_to: 4 }] },
    { ...valid, quote_coverage: [{ seq_from: 0, seq_to: 1 }, { seq_from: 2, seq_to: 5 }] },
    { ...valid, quote_coverage: [{ seq_from: 0, seq_to: 1 }, { seq_from: 0, seq_to: 1 }, { seq_from: 2, seq_to: 4 }] },
    { ...valid, price_observations: valid.price_observations.slice(1) },
    { ...valid, price_observations: [valid.price_observations[0], valid.price_observations[0], valid.price_observations[2]] },
    ...[{ seq: 2 }, { tick: 962 }, { tick: 900 }, { code: 999999 }, { last_price: "bad" }].map(change => ({ ...valid, price_observations: valid.price_observations.map((point, index) => index === 1 ? { ...point, ...change } : point) })),
  ];
  for (const value of invalid) assert.throws(() => reduceEngineUpdate(baseState(), "generation-1", { TickBatch: { frames: [value], runtime_snapshot: snapshot(961, 4) } }));
  const { quote_coverage: _coverage, ...withoutCoverage } = valid;
  const { price_observations: _observations, ...withoutObservations } = valid;
  for (const value of [withoutCoverage, withoutObservations]) assert.throws(() => reduceEngineUpdate(baseState(), "generation-1", tickBatch([value], snapshot(961, 4))), /必须同时存在/);
  const empty = { ...frame(1, 0, []), quote_coverage: [], price_observations: [] };
  const emptyResult = reduceEngineUpdate(baseState(), "generation-1", tickBatch([empty], snapshot(1, 0)));
  assert.equal(emptyResult.kind, "applied");
  assert.deepEqual(emptyResult.effects, []);
});

test("同分钟跨交付区间和跨分钟采样与逐拍图表一致", { timeout: 10000 }, () => {
  const source = [sample(901, "1200", 50, "60000"), sample(902, "900", 100, "100000"), sample(930, "1050", 150, "152500"), sample(961, "1100", 200, "210000")];
  const full = source.map((entry, index) => ({ ...frame(entry.point.tick, index, [0]), facts: [{ key: { phase_rank: 4, entity: { Stock: 0 }, source: "PriceTick", local_event_index: 0 }, event: { PriceTick: { seq: index + 1, tick: entry.point.tick, code: 0, last_price: entry.point.last_price, daily_candle: entry.candle, bids: [], asks: [] } } }], tick_from: index === 0 ? 1 : source[index - 1]!.point.tick + 1, fact_ticks: [entry.point.tick], timeseries_payload: { ...timeseries(entry.point.tick), continuous_points: { "0": entry.point }, active_daily_candles: { "0": entry.candle } } }));
  const expected = reduceEngineUpdate(baseState(), "generation-1", tickBatch(full, snapshot(961, 4)));
  const compressedFrames = [
    { ...full[1]!, tick_from: 1, seq_from: 0, seq_to: 2, facts: [], fact_ticks: [], quote_coverage: [{ seq_from: 0, seq_to: 2 }], price_observations: source.slice(0, 2).map((entry, index) => ({ code: 0, seq: index + 1, tick: entry.point.tick, last_price: entry.point.last_price })), timeseries_payload: { ...full[1]!.timeseries_payload, continuous_samples: { "0": [source[1]!] } } },
    { ...full[3]!, tick_from: 903, seq_from: 2, seq_to: 4, facts: [], fact_ticks: [], quote_coverage: [{ seq_from: 2, seq_to: 4 }], price_observations: source.slice(2).map((entry, index) => ({ code: 0, seq: index + 3, tick: entry.point.tick, last_price: entry.point.last_price })), timeseries_payload: { ...full[3]!.timeseries_payload, continuous_samples: { "0": source.slice(2) } } },
  ];
  const first = reduceEngineUpdate(baseState(), "generation-1", tickBatch([compressedFrames[0]!], snapshot(902, 2)));
  const actual = reduceEngineUpdate(first.state, "generation-1", tickBatch([compressedFrames[1]!], snapshot(961, 4)));
  const fullChart = new MarketChartProjection(), compressedChart = new MarketChartProjection();
  fullChart.rebuildHistory(expected.state.intraday);
  compressedChart.upsertFrames(first.state.intraday);
  compressedChart.upsertFrames(actual.update.kind === "tick-batch" ? actual.update.frames : []);
  assert.deepEqual(compressedChart.pricePointsFor("000812"), fullChart.pricePointsFor("000812"));
  assert.deepEqual(compressedChart.pricePointsFor("000812").map(point => [point.volume, point.cumulativeVolumeShares, point.cumulativeTurnoverCents]), [[150, 150, "152500"], [50, 200, "210000"]]);
});

test("保留事实与报价观察按联合 seq 校验原始 tick 顺序", { timeout: 10000 }, () => {
  const valid = compressed();
  const invalid = [
    { ...valid, price_observations: valid.price_observations.map((point, index) => index === 0 ? { ...point, tick: 902 } : point) },
    { ...valid, fact_ticks: [903] },
  ];
  for (const value of invalid) assert.throws(() => reduceEngineUpdate(baseState(), "generation-1", tickBatch([value], snapshot(961, 4))), /seq.*tick/);
});

test("报价观察拒绝负价格并保留零价格合法性", { timeout: 10000 }, () => {
  const original = compressed();
  const withPrice = (price: string) => ({ ...original, price_observations: original.price_observations.map((point, index) => index === 0 ? { ...point, last_price: price } : point) });
  assert.throws(() => reduceEngineUpdate(baseState(), "generation-1", tickBatch([withPrice("-1")], snapshot(961, 4))), /last_price.*非负/);
  const result = reduceEngineUpdate(baseState(), "generation-1", tickBatch([withPrice("0")], snapshot(961, 4)));
  const first = result.effects.flatMap(effect => effect.kind === "automatic-order" ? effect.points : [])[0];
  assert.equal(first?.last_price, "0");
});
