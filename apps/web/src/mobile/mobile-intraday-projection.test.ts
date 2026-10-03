import assert from "node:assert/strict";
import { test } from "node:test";
import * as model from "./market-model.ts";
import type { TradeEvent } from "../types/engine.ts";

function inputs() {
  return { market: { last_close: 1000 }, minutePoints: [], auctionPoints: [], trades: [] as TradeEvent[], elapsedMinutes: 0, totalMinutes: 240, gameDay: 3, gameTick: 0 };
}

test("空分时以昨收显示均价，保持空 signature 和价格中轴", () => {
  assert.equal(typeof model.MobileIntradayProjection, "function", "分时投影必须提供共同数据与几何行为 owner");
  const p = model.MobileIntradayProjection.fromInputs(inputs());
  assert.equal(p.displayedAverage, 10);
  assert.equal(p.priceY(10), 50);
  assert.equal(p.auctionLine(), "");
  assert.equal(p.continuousLine(), "");
  assert.equal(p.averageLine(), "");
  assert.equal(p.signature, "3:empty");
  assert.equal(p.progress, 0);
  assert.deepEqual(p.volumeMarks(), []);
});

test("null 竞价价不进入曲线但保留累计量，连续量独立缩放，均价为算术均值", () => {
  const source = { ...inputs(), auctionPoints: [{ time: 0, value: null, volume: 100000, buy: false }, { time: 99, value: 12, volume: 50000, buy: true }], minutePoints: [{ time: 119, value: 10, volume: 100, buy: false }, { time: 120, value: 8, volume: 200, buy: true }], elapsedMinutes: 121 };
  const before = structuredClone(source);
  const p = model.MobileIntradayProjection.fromInputs(source);
  assert.equal(p.visibleAuctionPricePoints.length, 1);
  assert.equal(p.displayedAverage, 9);
  assert.equal(p.averageLine(), `${model.intradayChartX({ phase: "continuous", minute: 119 })},${p.priceY(10)} ${model.intradayChartX({ phase: "continuous", minute: 120 })},${p.priceY(9)}`);
  assert.deepEqual(p.volumeMarks().map(m => m.height), [100, 50, 50, 100]);
  assert.equal(p.volumeMarks()[0].phase, "auction");
  assert.equal(p.visibleAuctionPricePoints[0].value, 12);
  assert.equal(p.priceY(10), 50);
  assert.ok(p.scale.top > 12 && p.scale.bottom < 8);
  assert.equal(p.signature, "3:continuous:120:8:200");
  assert.equal(p.progress, (2 / 10 + 121) / 250);
  assert.deepEqual(source, before);
});

test("分时保留单点竞价、最后七笔倒序和现有统一明细时刻", () => {
  const trades = Array.from({ length: 9 }, (_, seq) => ({ seq, code: "600001", price: 1000, qty: 1, maker: 1, taker: 2 }));
  const source = { ...inputs(), auctionPoints: [{ time: 99, value: 11, volume: 0, buy: true }], trades, elapsedMinutes: 121, gameTick: 8100 };
  const p = model.MobileIntradayProjection.fromInputs(source);
  assert.equal(p.displayedAverage, 11);
  assert.equal(p.auctionLine(), `16,${p.priceY(11)}`);
  assert.equal(p.signature, "3:auction:99:11:0");
  assert.deepEqual(p.recentTrades.map(t => t.seq), [8, 7, 6, 5, 4, 3, 2]);
  assert.equal(p.tradeTime, "13:00");
  assert.equal(p.clockTime, "13:00");
  assert.equal(p.volumeMarks()[0].height, 1);
  assert.equal(trades[0].seq, 0);
});

test("分时按原 slice 限制连续与竞价可见点，异常价与量继续显式拒绝", () => {
  const source = { ...inputs(), totalMinutes: 1, minutePoints: [{ time: 0, value: 9 }, { time: 1, value: 10 }], auctionPoints: Array.from({ length: 101 }, (_, i) => ({ time: Math.max(0, i - 1), value: null, volume: i, buy: false })) };
  const p = model.MobileIntradayProjection.fromInputs(source);
  assert.equal(p.visiblePoints.length, 1);
  assert.equal(p.visiblePoints[0].time, 1);
  assert.equal(p.visibleAuctionPoints.length, 100);
  assert.throws(() => model.MobileIntradayProjection.fromInputs({ ...inputs(), minutePoints: [{ time: 0, value: -1 }] }), /分时图价格/);
  assert.throws(() => model.MobileIntradayProjection.fromInputs({ ...inputs(), auctionPoints: [{ time: 0, value: null, volume: -1, buy: false }] }), /分时量/);
});
