import assert from "node:assert/strict";
import test from "node:test";
import { DEFAULT_SETUP } from "../config/defaults.ts";
import { MarketChartProjection } from "./market-chart-projection.ts";
import { baseState } from "../host/protocol-test-fixtures.ts";
import type { NormalizedTickFrame } from "../host/protocol/index.ts";
import type { Snapshot } from "../types/engine.ts";

function quote(tick: number, volume: number, code = "600000"): NormalizedTickFrame {
  return { tick, seqFrom: 0, seqTo: 0, events: [], facts: [], markets: {}, closedDailyCandles: {}, activeDailyCandles: {}, auctionPoints: {},
    continuousPoints: { [code]: { tick, phase: "Continuous", last_price: "1200", cumulative_volume: volume, bids: [], asks: [] } } };
}

function quoteWithDailyStats(
  tick: number,
  cumulativeVolume: number,
  volumeShares: number,
  turnoverCents: string,
  tradeCount: number,
  lastPrice = "1200",
  ohlc = { open: "1200", high: "1200", low: "1200", close: "1200" },
): NormalizedTickFrame {
  const source = quote(tick, cumulativeVolume);
  return {
    ...source,
    continuousPoints: {
      ...source.continuousPoints,
      "600000": { ...source.continuousPoints["600000"], last_price: lastPrice },
    },
    activeDailyCandles: { "600000": {
      time: 1893456000, ...ohlc, volume: volumeShares,
      trade_stats: { turnover_cents: turnoverCents, trade_count: tradeCount },
    } },
  };
}

test("短交易日按 SessionSetup 投影阶段、收盘与跨日量基线，不借用默认 tick 长度", { timeout: 10000 }, () => {
  const owner = new MarketChartProjection({ ...DEFAULT_SETUP, ticks_per_day: 30, auction_ticks: 9, closing_auction_ticks: 3 });
  owner.upsertFrames([quote(10, 100), quote(27, 180), quote(30, 200)]);
  assert.deepEqual(owner.pricePointsFor("600000").map(point => [point.time, point.volume]), [[0, 100], [223, 80], [239, 20]]);
  owner.upsertFrames([quote(40, 5)]);
  assert.deepEqual(owner.pricePointsFor("600000"), [{ time: 0, value: 12, volume: 5, buy: true }]);
});

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
    { time: 1, value: 12, volume: 70, buy: true },
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
    { key: { phase_rank: 4, entity: { Stock: "600000" }, source: "Sealed", local_event_index: 1 }, tick: 2, kind: "Indication", phase: "ClosingAuction", indicative_price: "1000", matched_volume: 10, imbalance: null },
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
  const snapshot: Snapshot = { ...initial.snapshot, daily_candles: { "600000": [{ time: 1893369600, open: "900", high: "900", low: "900", close: "900", volume: 100 }] } };
  const owner = new MarketChartProjection();
  owner.installBaseline({ ...initial, snapshot, intraday: [quote(901, 100)] });
  const selected = owner.pricePointsFor("600000");
  assert.throws(() => { (selected as unknown as unknown[]).pop(); }, TypeError);
  assert.equal(owner.pricePointsFor("600000").length, 1);
  assert.throws(() => { owner.pricePointsFor("600000")[0].value = 99; }, TypeError);
  assert.throws(() => { (owner.history() as Record<string, unknown>)["600000"] = []; }, TypeError);
  assert.throws(() => { (owner.activeCandles() as Record<string, unknown>)["600000"] = null; }, TypeError);
  const completed = owner.candlesFor("600000")[0];
  owner.replaceActiveCandles({ "600000": { ...snapshot.active_daily_candles["600000"], close: "1300" } });
  assert.equal(owner.candlesFor("600000")[0], completed);
  assert.equal(owner.candlesFor("600000").at(-1)!.close, 13);
  owner.replaceActiveCandles({});
  assert.deepEqual(owner.activeCandles(), {});
  owner.reset(snapshot);
  assert.deepEqual(owner.history(), {});
  assert.equal(owner.candlesFor("600000").length, 2);
});

test("竞价成交量不混入连续分钟量，新日只保留新日采样且方向取相邻有效价格", { timeout: 10000 }, () => {
  const owner = new MarketChartProjection();
  const auction = { ...quote(600, 0), continuousPoints: {}, auctionPoints: { "600000": [
    { key: { phase_rank: 4, entity: { Stock: "600000" } as const, source: "Sealed" as const, local_event_index: 0 }, tick: 600, kind: "Completion" as const, phase: "CallAuction" as const, indicative_price: "1300", matched_volume: 100, imbalance: null },
  ] } };
  owner.upsertFrames([auction, quote(901, 120), quote(902, 130)]);
  assert.deepEqual(owner.pricePointsFor("600000"), [{ time: 0, value: 12, volume: 30, buy: false }]);
  owner.upsertFrames([quote(961, 160)]);
  const next = quoteWithDailyStats(16201, 5, 5, "6000", 1);
  const later = quoteWithDailyStats(16261, 8, 8, "9600", 2);
  const oldDay = quoteWithDailyStats(15300, 200, 200, "240000", 10);
  owner.upsertFrames([oldDay, next, later]);
  assert.deepEqual(owner.pricePointsFor("600000"), [
    { time: 0, value: 12, volume: 5, buy: true, cumulativeTurnoverCents: "6000", cumulativeVolumeShares: 5 },
    { time: 1, value: 12, volume: 3, buy: true, cumulativeTurnoverCents: "9600", cumulativeVolumeShares: 8 },
  ]);
  assert.deepEqual(owner.auctionPointsFor("600000"), []);
});

test("同分钟涨跌以最后价对前分钟价判定，不被分钟内部反弹或平价误标", { timeout: 10000 }, () => {
  const owner = new MarketChartProjection();
  const falling = quote(961, 120);
  const rebound = quote(962, 130);
  falling.continuousPoints["600000"].last_price = "1100";
  rebound.continuousPoints["600000"].last_price = "1150";
  owner.upsertFrames([quote(901, 100), falling, rebound]);
  assert.deepEqual(owner.pricePointsFor("600000").at(-1), { time: 1, value: 11.5, volume: 30, buy: false });
});

test("分时采样附着同帧权威日累计成交额与股数，含基线与同分钟更新", { timeout: 10000 }, () => {
  const owner = new MarketChartProjection();
  const first = quoteWithDailyStats(901, 100, 100, "100000", 2, "1000", { open: "1000", high: "1000", low: "1000", close: "1000" });
  owner.installBaseline({ ...baseState(), intraday: [first] });
  assert.deepEqual(owner.pricePointsFor("600000"), [{
    time: 0, value: 10, volume: 100, buy: true,
    cumulativeTurnoverCents: "100000", cumulativeVolumeShares: 100,
  }]);
  const updated = quoteWithDailyStats(902, 160, 300, "260000", 3, "800", { open: "1000", high: "1000", low: "800", close: "800" });
  owner.upsertFrames([updated]);
  assert.deepEqual(owner.pricePointsFor("600000").at(-1), {
    time: 0, value: 8, volume: 160, buy: true,
    cumulativeTurnoverCents: "260000", cumulativeVolumeShares: 300,
  });
});

test("竞价方向跳过 null 指示价，下降指示价不冒充买入方向", { timeout: 10000 }, () => {
  const owner = new MarketChartProjection();
  const prices = ["1300", "1200", null, "1100"];
  owner.upsertFrames(prices.map((price, index) => {
    const tick = index * 6 + 1;
    return { ...quote(tick, 0), continuousPoints: {}, auctionPoints: { "600000": [{
      key: { phase_rank: 4, entity: { Stock: "600000" }, source: "Sealed", local_event_index: 0 },
      tick, kind: "Indication", phase: "CallAuction", indicative_price: price, matched_volume: 10, imbalance: null,
    }] } };
  }));
  assert.deepEqual(owner.auctionPointsFor("600000").map((point) => point.buy), [true, false, false, false]);
});

test("同六秒竞价槽无变化不翻色刷新，槽内反弹仍与前有效槽比较", { timeout: 10000 }, () => {
  const owner = new MarketChartProjection();
  const auction = (tick: number, price: number): NormalizedTickFrame => ({ ...quote(tick, 0), continuousPoints: {}, auctionPoints: { "600000": [{
    key: { phase_rank: 4, entity: { Stock: "600000" }, source: "Sealed", local_event_index: 0 },
    tick, kind: "Indication", phase: "CallAuction", indicative_price: String(price), matched_volume: 10, imbalance: null,
  }] } });
  owner.upsertFrames([auction(1, 1300), auction(7, 1200)]);
  const before = owner.auctionPointsFor("600000");
  owner.upsertFrames([auction(8, 1200)]);
  assert.equal(owner.auctionPointsFor("600000"), before);
  assert.equal(owner.auctionPointsFor("600000").at(-1)!.buy, false);
  owner.upsertFrames([auction(9, 1250)]);
  assert.equal(owner.auctionPointsFor("600000").at(-1)!.buy, false);
});

test("未变证券的只读分时、竞价和日 K 数组保持引用", { timeout: 10000 }, () => {
  const owner = new MarketChartProjection();
  owner.installBaseline({ ...baseState(), intraday: [quote(901, 100)] });
  const prices = owner.pricePointsFor("600000");
  const auctions = owner.auctionPointsFor("600000");
  const candles = owner.candlesFor("600000");
  owner.upsertFrames([quote(902, 5, "000001")]);
  owner.replaceActiveCandles(structuredClone(baseState().snapshot.active_daily_candles));
  assert.equal(owner.pricePointsFor("600000"), prices);
  assert.equal(owner.auctionPointsFor("600000"), auctions);
  assert.equal(owner.candlesFor("600000"), candles);
  owner.replaceSnapshot(structuredClone(baseState().snapshot));
  assert.equal(owner.candlesFor("600000"), candles);
  owner.upsertFrames([quote(902, 100)]);
  assert.equal(owner.pricePointsFor("600000"), prices);
});
