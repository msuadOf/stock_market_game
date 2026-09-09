import assert from "node:assert/strict";
import test from "node:test";
import type { EngineEvent, Snapshot } from "../types/engine.ts";
import {
  UI_UPDATE_INTERVAL_MS,
  createBaselineUpdate,
  createDeltaUpdate,
} from "./host-update.ts";

const SNAPSHOT = {
  seq: 3,
  tick: 3,
  day: 0,
  phase: "Continuous",
  markets: {},
  accounts: {},
  daily_candles: {},
  active_daily_candles: {},
} as Snapshot;

const priceTick = (seq: number) => ({ PriceTick: { seq } }) as EngineEvent;
const trade = (seq: number) => ({ Trade: { seq } }) as EngineEvent;

test("all host transports share one baseline and delta application protocol", () => {
  assert.deepEqual(createBaselineUpdate(SNAPSHOT), { type: "baseline", snapshot: SNAPSHOT });
  assert.deepEqual(createDeltaUpdate([priceTick(1), priceTick(3)], undefined, { fromSeq: 1, toSeq: 3 }), {
    type: "delta",
    fromSeq: 1,
    toSeq: 3,
    events: [priceTick(1), priceTick(3)],
    runtimeSnapshot: undefined,
  });
});

test("local transports derive a complete seq coverage from their event batch", () => {
  const update = createDeltaUpdate([priceTick(4), priceTick(5)]);
  assert.equal(update.fromSeq, 4);
  assert.equal(update.toSeq, 5);
});

test("state-changing delta is rejected atomically without an exact runtime snapshot", () => {
  assert.throws(() => createDeltaUpdate([trade(3)]), /缺少权威运行快照/);
  assert.throws(
    () => createDeltaUpdate([trade(3)], { ...SNAPSHOT, seq: 4 }),
    /必须等于覆盖区间末尾/,
  );
});

test("UI delivery target uses a 16ms cadence above 60Hz", () => {
  assert.equal(UI_UPDATE_INTERVAL_MS, 16);
  assert.ok(1_000 / UI_UPDATE_INTERVAL_MS > 60);
});
