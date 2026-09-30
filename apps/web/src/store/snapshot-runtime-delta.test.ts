import assert from "node:assert/strict";
import test from "node:test";
import { applyProtocolRuntimeDelta, installProtocolSnapshotBaseline, snapshotReducer } from "./store.ts";
import { parseProtocolSnapshot } from "../host/protocol/parse.ts";
import { snapshot } from "../host/protocol-test-fixtures.ts";
import type { RuntimeDelta } from "../types/generated/RuntimeDelta.ts";

function baseline() {
  return snapshotReducer(undefined, installProtocolSnapshotBaseline({ generation: "1", snapshot: parseProtocolSnapshot({
    ...snapshot(0, 0), accounts: { "0": { cash: 1_000, reserved_cash: 0, positions: {}, reserved_sell_qty: {} } },
  }) }));
}

function delta(): RuntimeDelta {
  return {
    tick: 1, seq_from: 0, seq_to: 0, day: 0, phase: "Continuous",
    accounts: {}, working_orders: { reset: true, upserts: [], removed: [] },
  };
}

test("snapshot delta keeps unchanged account, market and completed candle references", () => {
  const before = baseline();
  const original = before.snapshot!;
  const after = snapshotReducer(before, applyProtocolRuntimeDelta({
    generation: "1", delta: delta(),
    markets: structuredClone(original.markets), activeDailyCandles: structuredClone(original.active_daily_candles),
  }));
  assert.equal(after.snapshot!.accounts, original.accounts);
  assert.equal(after.snapshot!.markets, original.markets);
  assert.equal(after.snapshot!.active_daily_candles, original.active_daily_candles);
  assert.equal(after.snapshot!.daily_candles, original.daily_candles);
  assert.equal(after.snapshot!.tick, 1);
});

test("snapshot store rejects stale generation and noncontiguous seq without partial account updates", () => {
  const before = baseline();
  const original = before.snapshot!;
  for (const [generation, from] of [["2", 0], ["1", 1]] as const) {
    assert.throws(() => snapshotReducer(before, applyProtocolRuntimeDelta({
      generation, delta: { ...delta(), seq_from: from }, markets: original.markets, activeDailyCandles: original.active_daily_candles,
    })), { code: "PROTOCOL_CURSOR" });
    assert.equal(before.snapshot, original);
    assert.equal(before.snapshot!.tick, 0);
  }
});

test("fresh baseline clears working-order state and rejects the former generation", () => {
  const before = baseline();
  const refreshed = snapshotReducer(before, installProtocolSnapshotBaseline({ generation: "2", snapshot: before.snapshot! }));
  assert.equal(refreshed.playerOrdersReady, false);
  assert.deepEqual(refreshed.playerWorkingOrders, {});
  assert.throws(() => snapshotReducer(refreshed, applyProtocolRuntimeDelta({ generation: "1", delta: delta(), markets: before.snapshot!.markets, activeDailyCandles: before.snapshot!.active_daily_candles })), { code: "PROTOCOL_CURSOR" });
});
