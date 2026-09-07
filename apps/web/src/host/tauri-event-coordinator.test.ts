import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type { EngineEvent, Snapshot } from "../types/engine.ts";
import { createTauriEventCoordinator } from "./tauri-event-coordinator.ts";

const boundary: EngineEvent = {
  DayBoundary: { seq: 10, day: 1, closed_daily_candles: {} },
};
const nextTick: EngineEvent = {
  PriceTick: {
    seq: 11,
    tick: 14_401,
    code: "AAA",
    last_price: 101,
    daily_candle: { time: 1, open: 101, high: 101, low: 101, close: 101, volume: 1 },
    bids: [],
    asks: [],
  },
};

describe("Tauri event coordinator", () => {
  it("delivers each boundary and its same-tick snapshot atomically before newer events", () => {
    const order: string[] = [];
    const coordinator = createTauriEventCoordinator({
      deliverEvents(events) { order.push(...events.map((event) => `event:${Object.values(event)[0].seq}`)); },
      deliverSnapshot(snapshot) { order.push(`snapshot:${snapshot.seq}`); },
    });

    coordinator.accept([boundary], { seq: 10, tick: 14_400 } as Snapshot);
    coordinator.accept([nextTick]);
    assert.deepEqual(order, ["event:10", "snapshot:10", "event:11"]);
  });

  it("rejects a missing or stale snapshot before delivering a day boundary", () => {
    const coordinator = createTauriEventCoordinator({
      deliverEvents() { throw new Error("invalid payload must not be delivered"); },
      deliverSnapshot() { throw new Error("invalid snapshot must not be delivered"); },
    });
    assert.throws(() => coordinator.accept([boundary]), /缺少同批 runtime snapshot/);
    assert.throws(
      () => coordinator.accept([boundary], { seq: 9 } as Snapshot),
      /早于同批事件/,
    );
  });
});
