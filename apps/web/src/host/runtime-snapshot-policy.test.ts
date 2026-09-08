import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type { EngineEvent, Snapshot } from "../types/engine.ts";
import {
  deliverEventsThenSnapshot,
  requiresRuntimeSnapshot,
} from "./runtime-snapshot-policy.ts";

describe("runtime snapshot policy", () => {
  it("refreshes authoritative account state after every trade", () => {
    const trade = {
      Trade: { seq: 1, code: "600101", price: 1000, qty: 100, maker: 1, taker: 0 },
    } as EngineEvent;
    assert.equal(requiresRuntimeSnapshot([trade]), true);
  });

  it("refreshes at day boundary but not for price-only ticks", () => {
    const boundary = { DayBoundary: { seq: 2, day: 1, closed_daily_candles: {} } } as EngineEvent;
    const priceTick = {
      PriceTick: {
        seq: 3,
        tick: 1,
        code: "600101",
        last_price: 1000,
        daily_candle: { time: 0, open: 1000, high: 1000, low: 1000, close: 1000, volume: 0 },
        bids: [],
        asks: [],
      },
    } as EngineEvent;
    assert.equal(requiresRuntimeSnapshot([boundary]), true);
    assert.equal(requiresRuntimeSnapshot([priceTick]), false);
  });

  it("refreshes after auction completion so transferred remainders and PreOpen phase are visible", () => {
    const completed = {
      AuctionCompleted: {
        seq: 4,
        tick: 600,
        code: "600101",
        opening_price: null,
        matched_volume: 0,
      },
    } as EngineEvent;
    assert.equal(requiresRuntimeSnapshot([completed]), true);
  });

  it("refreshes after accepting or canceling an order so available cash and shares stay authoritative", () => {
    const accepted = {
      OrderAccepted: {
        seq: 5,
        account: 0,
        code: "600101",
        id: 1,
        side: "Sell",
        price: 1000,
        remaining_qty: 100,
      },
    } as EngineEvent;
    const canceled = {
      OrderCanceled: {
        seq: 6,
        account: 0,
        code: "600101",
        id: 1,
        remaining_qty: 100,
      },
    } as EngineEvent;

    assert.equal(requiresRuntimeSnapshot([accepted]), true);
    assert.equal(requiresRuntimeSnapshot([canceled]), true);
  });

  it("delivers state-changing events before their authoritative runtime snapshot", () => {
    const order: string[] = [];
    const event = {
      OrderAccepted: {
        seq: 7,
        account: 0,
        code: "600101",
        id: 2,
        side: "Buy",
        price: 1000,
        remaining_qty: 100,
      },
    } as EngineEvent;
    const snapshot = {} as Snapshot;

    deliverEventsThenSnapshot(
      [event],
      snapshot,
      () => order.push("events"),
      () => order.push("snapshot"),
    );

    assert.deepEqual(order, ["events", "snapshot"]);
  });
});
