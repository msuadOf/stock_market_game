import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type { EngineEvent } from "../types/engine.ts";
import {
  compactFastForwardEvents,
  normalizeEventMaps,
  normalizeWasmStepEvents,
  uiBackpressurePolicy,
} from "./event-buffer.ts";

const tick = (seq: number, code: string, close: number): EngineEvent => ({
  PriceTick: {
    seq,
    tick: seq,
    code,
    last_price: close,
    daily_candle: {
      time: 0,
      open: 100,
      high: close,
      low: 100,
      close,
      volume: seq,
      trade_stats: { turnover_cents: "9007200000000000", trade_count: 7 },
    },
    bids: [],
    asks: [],
  },
});

describe("fast-forward event buffer", () => {
  it("keeps engine stepping while the UI is waiting for its next paint", () => {
    assert.deepEqual(uiBackpressurePolicy(true), {
      stepEngine: true,
      flushUi: false,
    });
  });

  it("keeps every closed daily candle and one latest sample per active trading minute", () => {
    const boundary: EngineEvent = {
      DayBoundary: {
        seq: 4,
        day: 1,
        closed_daily_candles: {
          AAA: { time: 0, open: 100, high: 120, low: 90, close: 110, volume: 20 },
        },
      },
    };
    const events = [
      tick(1, "AAA", 101),
      tick(60, "AAA", 102),
      boundary,
      tick(61, "AAA", 105),
      tick(119, "AAA", 106),
      tick(120, "AAA", 107),
      tick(121, "AAA", 108),
      tick(122, "BBB", 205),
    ];

    assert.deepEqual(compactFastForwardEvents(events, 100, 60, 14_400, 0), [
      boundary,
      tick(120, "AAA", 107),
      tick(121, "AAA", 108),
      tick(122, "BBB", 205),
    ]);
  });

  it("keeps one auction indication per six-second volume slot and the final uncross event", () => {
    const auction = (seq: number, tickValue: number, price: number): EngineEvent => ({
      AuctionTick: { seq, tick: tickValue, phase: "CallAuction", code: "AAA", indicative_price: price, matched_volume: seq, imbalance: 0 },
    });
    const completed: EngineEvent = {
      AuctionCompleted: { seq: 4, tick: 900, phase: "CallAuction", code: "AAA", clearing_price: 103, matched_volume: 20 },
    };
    assert.deepEqual(compactFastForwardEvents([
      auction(1, 1, 101), auction(2, 6, 102), auction(3, 7, 103), completed,
    ]), [auction(2, 6, 102), auction(3, 7, 103), completed]);
  });

  it("retains errors and only the newest trades needed by the visible tape", () => {
    const rejected: EngineEvent = {
      IntentRejected: { seq: 5, account: 1, code: "AAA", reason: "InsufficientCash" },
    };
    const trades = Array.from({ length: 4 }, (_, i): EngineEvent => ({
      Trade: { seq: i + 1, code: "AAA", qty: 1, price: 100 + i, maker: 1, taker: 2 },
    }));

    assert.deepEqual(compactFastForwardEvents([...trades, rejected], 2), [trades[2], trades[3], rejected]);
  });

  it("normalizes WASM day-boundary maps before Redux receives them", () => {
    const wasmEvent = {
      DayBoundary: {
        seq: 1,
        day: 1,
        closed_daily_candles: new Map([
          ["AAA", { time: 0, open: 100, high: 110, low: 90, close: 105, volume: 20 }],
        ]),
      },
    } as unknown as EngineEvent;

    assert.deepEqual(normalizeEventMaps([wasmEvent]), [{
      DayBoundary: {
        seq: 1,
        day: 1,
        closed_daily_candles: {
          AAA: { time: 0, open: 100, high: 110, low: 90, close: 105, volume: 20 },
        },
      },
    }]);
  });

  it("normalizes WASM optional auction prices from undefined to null", () => {
    const tick = {
      AuctionTick: { seq: 1, tick: 1, code: "AAA", indicative_price: undefined, matched_volume: 0, imbalance: 0 },
    } as unknown as EngineEvent;
    const completed = {
      AuctionCompleted: { seq: 2, tick: 900, code: "AAA", clearing_price: undefined, matched_volume: 0 },
    } as unknown as EngineEvent;

    assert.deepEqual(normalizeEventMaps([tick, completed]), [
      { AuctionTick: { seq: 1, tick: 1, code: "AAA", indicative_price: null, matched_volume: 0, imbalance: 0 } },
      { AuctionCompleted: { seq: 2, tick: 900, code: "AAA", clearing_price: null, matched_volume: 0 } },
    ]);
  });

  it("normalizes the single-thread WASM inbound event batch before delivery", () => {
    const raw = [{
      DayBoundary: {
        seq: 3,
        day: 2,
        closed_daily_candles: new Map([
          ["600101", { time: 1, open: 1000, high: 1010, low: 990, close: 1005, volume: 100 }],
        ]),
      },
    }, {
      AuctionCompleted: {
        seq: 4,
        tick: 900,
        code: "600101",
        clearing_price: undefined,
        matched_volume: 0,
      },
    }];

    assert.deepEqual(normalizeWasmStepEvents(raw), [{
      DayBoundary: {
        seq: 3,
        day: 2,
        closed_daily_candles: {
          "600101": { time: 1, open: 1000, high: 1010, low: 990, close: 1005, volume: 100 },
        },
      },
    }, {
      AuctionCompleted: {
        seq: 4,
        tick: 900,
        code: "600101",
        clearing_price: null,
        matched_volume: 0,
      },
    }]);
  });

  it("rejects a malformed WASM step payload", () => {
    assert.throws(() => normalizeWasmStepEvents({}), /事件数组/);
  });
});
