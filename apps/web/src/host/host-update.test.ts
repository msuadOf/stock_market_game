import assert from "node:assert/strict";
import test from "node:test";
import { createBaselineUpdate, createProtocolUpdate, UI_UPDATE_INTERVAL_MS } from "./host-update.ts";

const snapshot = {
  seq: 0,
  tick: 0,
  day: 0,
  phase: "Continuous" as const,
  markets: {},
  accounts: {},
  daily_candles: {},
  active_daily_candles: {},
};

test("Given a host baseline and protocol update, when constructed, then both carry an authoritative generation", () => {
  assert.deepEqual(createBaselineUpdate("1", snapshot), {
    type: "baseline", generation: "1", snapshot, civilDate: null, revision: null, securities: [], publicPublicationIds: [],
  });
  assert.deepEqual(createProtocolUpdate("1", { TickBatch: {} }), {
    type: "protocol", generation: "1", update: { TickBatch: {} }, civilDate: null, revision: null,
  });
});

test("Given an invalid generation, when constructing a production host envelope, then it rejects it", () => {
  assert.throws(() => createBaselineUpdate("01", snapshot), /generation/);
  assert.throws(() => createProtocolUpdate("-1", {}), /generation/);
});

test("UI delivery target uses a 16ms cadence above 60Hz", () => {
  assert.equal(UI_UPDATE_INTERVAL_MS, 16);
  assert.ok(1_000 / UI_UPDATE_INTERVAL_MS > 60);
});
