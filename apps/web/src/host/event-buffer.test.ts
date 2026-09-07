import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type { EngineEvent } from "../types/engine.ts";
import {
  compactFastForwardEvents,
  normalizeEventMaps,
  uiBackpressurePolicy,
} from "./event-buffer.ts";

const tick = (seq: number, code: string, close: number): EngineEvent => ({
  PriceTick: {
    seq,
    tick: seq,
    code,
    last_price: close,
    daily_candle: { time: 0, open: 100, high: close, low: 100, close, volume: seq },
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

    assert.deepEqual(compactFastForwardEvents(events), [
      boundary,
      tick(120, "AAA", 107),
      tick(121, "AAA", 108),
      tick(122, "BBB", 205),
    ]);
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
});
