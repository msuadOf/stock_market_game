import assert from "node:assert/strict";
import test from "node:test";
import { MarketChartProjection } from "./market-chart-projection.ts";
import { baseState } from "../host/protocol-test-fixtures.ts";
import type { NormalizedTickFrame } from "../host/protocol/index.ts";
import type { Snapshot } from "../types/engine.ts";

function quote(tick: number, volume: number, code = "600000"): NormalizedTickFrame {
  return { tick, seqFrom: 0, seqTo: 0, events: [], facts: [], markets: {}, closedDailyCandles: {}, activeDailyCandles: {}, auctionPoints: {},
    continuousPoints: { [code]: { tick, phase: "Continuous", last_price: 1200, cumulative_volume: volume, bids: [], asks: [] } } };
}

test("分时跨批次累计量基线、分钟替换与输入不变", { timeout: 10000 }, () => {
  const frames = [quote(901, 100), quote(961, 160), quote(962, 170)];
  const before = structuredClone(frames);
  const owner = new MarketChartProjection();
  owner.upsertFrames(frames.slice(0, 1));
  const peer = owner.history()["600000"];
  owner.upsertFrames([quote(901, 5, "000001")]);
  assert.equal(owner.history()["600000"], peer);
  owner.upsertFrames(frames.slice(1));
  assert.deepEqual(owner.pricePointsFor("600000"), [
    { time: 0, value: 12, volume: 100, buy: true },
    { time: 1, value: 12, volume: 10, buy: true },
  ]);
  assert.deepEqual(frames, before);
  assert.equal(owner.pricePointsFor("missing").length, 0);
  const independent = new MarketChartProjection();
  independent.upsertFrames(frames);
  assert.deepEqual(independent.pricePointsFor("600000"), owner.pricePointsFor("600000"));
  owner.upsertFrames([quote(1021, 0)]);
  assert.equal(owner.pricePointsFor("600000").at(-1)!.volume, 0);
});

test("重建清除旧代码与累计量，竞价保留空指示价并排除收盘竞价", { timeout: 10000 }, () => {
  const owner = new MarketChartProjection();
  owner.upsertFrames([quote(901, 100, "old")]);
  const input: NormalizedTickFrame = { ...quote(901, 20), auctionPoints: { "600000": [
    { key: { phase_rank: 4, entity: { Stock: "600000" }, source: "Sealed", local_event_index: 0 }, tick: 1, kind: "Indication", phase: "CallAuction", indicative_price: null, matched_volume: 0, imbalance: null },
    { key: { phase_rank: 4, entity: { Stock: "600000" }, source: "Sealed", local_event_index: 1 }, tick: 2, kind: "Indication", phase: "ClosingAuction", indicative_price: 1000, matched_volume: 10, imbalance: null },
  ] } };
  const before = structuredClone(input);
  owner.rebuildHistory([input]);
  assert.equal(Object.hasOwn(owner.history(), "old"), false);
  assert.equal(owner.pricePointsFor("600000")[0].volume, 20);
  assert.deepEqual(owner.auctionPointsFor("600000"), [{ time: 0, value: null, volume: 0, buy: false }]);
  assert.deepEqual(input, before);
  owner.rebuildHistory([]);
  assert.deepEqual(owner.history(), {});
});

test("baseline、活动日 K 替换和 reset 保留权威日 K，selected 数组不能反写缓存", { timeout: 10000 }, () => {
  const initial = baseState();
  const snapshot: Snapshot = { ...initial.snapshot, daily_candles: { "600000": [{ time: -1, open: 900, high: 900, low: 900, close: 900, volume: 100 }] } };
  const owner = new MarketChartProjection();
  owner.installBaseline({ ...initial, snapshot, intraday: [quote(901, 100)] });
  const selected = owner.pricePointsFor("600000");
  selected.pop();
  assert.equal(owner.pricePointsFor("600000").length, 1);
  assert.throws(() => { owner.pricePointsFor("600000")[0].value = 99; }, TypeError);
  assert.throws(() => { (owner.history() as Record<string, unknown>)["600000"] = []; }, TypeError);
  assert.throws(() => { (owner.activeCandles() as Record<string, unknown>)["600000"] = null; }, TypeError);
  const completed = owner.candlesFor("600000")[0];
  owner.replaceActiveCandles({ "600000": { ...snapshot.active_daily_candles["600000"], close: 1300 } });
  assert.equal(owner.candlesFor("600000")[0], completed);
  assert.equal(owner.candlesFor("600000").at(-1)!.close, 13);
  owner.replaceActiveCandles({});
  assert.deepEqual(owner.activeCandles(), {});
  owner.reset(snapshot);
  assert.deepEqual(owner.history(), {});
  assert.equal(owner.candlesFor("600000").length, 2);
});
