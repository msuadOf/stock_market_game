import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type { EngineEvent, Snapshot } from "../types/engine.ts";
import type { HostUpdate } from "./host-update.ts";
import {
  createTauriEventCoordinator,
  createTimelineEventGate,
  resumeCommittedTimeline,
  resumeRejectedTimeline,
} from "./tauri-event-coordinator.ts";

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
const acceptedSellOrder: EngineEvent = {
  OrderAccepted: {
    seq: 12,
    account: 0,
    code: "AAA",
    id: 1,
    side: "Sell",
    price: 100,
    remaining_qty: 100,
  },
};

describe("Tauri event coordinator", () => {
  it("rejects a delayed event from the pre-restore timeline", () => {
    const delivered: string[] = [];
    const gate = createTimelineEventGate("old", (payload: string) => delivered.push(payload));

    assert.equal(gate.accept("old", "before restore"), true);
    gate.replaceTimeline("new");
    assert.equal(gate.accept("old", "late old event"), false);
    assert.equal(gate.accept("new", "after restore"), true);
    assert.deepEqual(delivered, ["before restore", "after restore"]);
  });

  it("keeps a committed restore successful while reporting a failed resume as paused", async () => {
    const failures: string[] = [];
    const running = await resumeCommittedTimeline(
      async () => { throw new Error("resume unavailable"); },
      (failure) => failures.push(`${failure.code}: ${failure.message}`),
    );

    assert.equal(running, false);
    assert.match(failures[0]!, /TAURI_RESUME_AFTER_RESTORE.*读档已经成功.*保持暂停/);
  });

  it("reports paused state when both restore and recovery resume fail", async () => {
    const failures: string[] = [];
    await assert.rejects(
      resumeRejectedTimeline(
        async () => { throw new Error("resume unavailable"); },
        new Error("invalid save"),
        (failure) => failures.push(`${failure.code}: ${failure.message}`),
      ),
      /TAURI_RESTORE_AND_RESUME_FAILED.*invalid save.*resume unavailable/,
    );
    assert.match(failures[0]!, /TAURI_RESTORE_AND_RESUME_FAILED.*保持暂停/);
  });

  it("delivers each boundary and its same-tick snapshot atomically before newer events", () => {
    const order: string[] = [];
    const coordinator = createTauriEventCoordinator({
      deliverUpdate(update) {
        if (update.type !== "delta") return;
        order.push(...update.events.map((event) => `event:${Object.values(event)[0].seq}`));
        if (update.runtimeSnapshot) order.push(`snapshot:${update.runtimeSnapshot.seq}`);
      },
    });

    coordinator.accept([boundary], { seq: 10, tick: 14_400 } as Snapshot, { fromSeq: 10, toSeq: 10 });
    coordinator.accept([nextTick], undefined, { fromSeq: 11, toSeq: 11 });
    assert.deepEqual(order, ["event:10", "snapshot:10", "event:11"]);
  });

  it("rejects a missing or stale snapshot before delivering a day boundary", () => {
    const coordinator = createTauriEventCoordinator({
      deliverUpdate() { throw new Error("invalid payload must not be delivered"); },
    });
    assert.throws(() => coordinator.accept([boundary], undefined, { fromSeq: 10, toSeq: 10 }), /缺少权威运行快照/);
    assert.throws(
      () => coordinator.accept([boundary], { seq: 9 } as Snapshot, { fromSeq: 10, toSeq: 10 }),
      /必须等于覆盖区间末尾/,
    );
  });

  it("rejects a missing snapshot when a resting order changes reserved cash or shares", () => {
    const coordinator = createTauriEventCoordinator({
      deliverUpdate() { throw new Error("invalid payload must not be delivered"); },
    });

    assert.throws(
      () => coordinator.accept([acceptedSellOrder], undefined, { fromSeq: 12, toSeq: 12 }),
      /缺少权威运行快照/,
    );
  });

  it("preserves the actor's raw seq coverage after fastest compaction", () => {
    const delivered: HostUpdate[] = [];
    const coordinator = createTauriEventCoordinator({ deliverUpdate: (update) => delivered.push(update) });
    coordinator.accept([nextTick], undefined, { fromSeq: 1, toSeq: 11 });
    assert.deepEqual(delivered[0], {
      type: "delta",
      fromSeq: 1,
      toSeq: 11,
      events: [nextTick],
      runtimeSnapshot: undefined,
    });
  });
});
