import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type { Snapshot } from "../types/engine.ts";
import { candlesFromSnapshot, reduceCandleEvents } from "./kline-sync.ts";

describe("Rust K-line snapshot sync", () => {
  it("converts engine cents to chart yuan and keeps the active candle separate", () => {
    const snapshot = {
      seq: 0,
      tick: 1,
      day: 0,
      phase: "Continuous",
      markets: {},
      accounts: {},
      daily_candles: {
        "600101": [{ time: -86400, open: 1000, high: 1100, low: 900, close: 1050, volume: 12 }],
      },
      active_daily_candles: {
        "600101": { time: 0, open: 1050, high: 1070, low: 1040, close: 1060, volume: 8 },
      },
    } satisfies Snapshot;

    assert.deepEqual(candlesFromSnapshot(snapshot), {
      completed: {
        "600101": [{ time: -86400, open: 10, high: 11, low: 9, close: 10.5, volume: 12 }],
      },
      active: {
        "600101": { time: 0, open: 10.5, high: 10.7, low: 10.4, close: 10.6, volume: 8 },
      },
    });
  });
});

describe("authoritative K-line event sync", () => {
  it("uses Rust candles across a boundary without rebuilding an old event on the new day", () => {
    const completed: Record<string, import("../components/PriceChart.ts").KlinePoint[]> = {
      "600101": [{ time: -86400 as import("../components/PriceChart.ts").KlinePoint["time"], open: 10, high: 11, low: 9, close: 10.5, volume: 12 }],
    };
    const active = {};
    const closed = { time: 0, open: 10.5, high: 12, low: 10, close: 11, volume: 30 };
    const next = { time: 86400, open: 12, high: 12.5, low: 11.8, close: 12.2, volume: 4 };

    const result = reduceCandleEvents(completed, active, [
      { PriceTick: { seq: 1, tick: 1, code: "600101", last_price: 1100, daily_candle: { time: 0, open: 1050, high: 1200, low: 1000, close: 1100, volume: 30 }, bids: [], asks: [] } },
      { DayBoundary: { seq: 2, day: 1, closed_daily_candles: { "600101": { time: 0, open: 1050, high: 1200, low: 1000, close: 1100, volume: 30 } } } },
      { PriceTick: { seq: 3, tick: 14_401, code: "600101", last_price: 1220, daily_candle: { time: 86400, open: 1200, high: 1250, low: 1180, close: 1220, volume: 4 }, bids: [], asks: [] } },
    ]);

    assert.deepEqual(result.completed["600101"], [completed["600101"][0], closed]);
    assert.deepEqual(result.active["600101"], next);
    assert.deepEqual([...result.changedCodes], ["600101"]);
  });

  it("keeps only the last authoritative active candle per stock in a high-speed batch", () => {
    const first = { time: 0, open: 10, high: 10, low: 10, close: 10, volume: 0 };
    const last = { time: 0, open: 10, high: 12, low: 9, close: 11, volume: 20 };
    const result = reduceCandleEvents({}, {}, [
      { PriceTick: { seq: 1, tick: 1, code: "600101", last_price: 1000, daily_candle: { time: 0, open: 1000, high: 1000, low: 1000, close: 1000, volume: 0 }, bids: [], asks: [] } },
      { PriceTick: { seq: 2, tick: 2, code: "600101", last_price: 1100, daily_candle: { time: 0, open: 1000, high: 1200, low: 900, close: 1100, volume: 20 }, bids: [], asks: [] } },
    ]);
    assert.notDeepEqual(result.active["600101"], first);
    assert.deepEqual(result.active["600101"], last);
  });
});
