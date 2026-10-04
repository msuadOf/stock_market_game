import assert from "node:assert/strict";
import test from "node:test";
import { ProtocolCoordinator } from "./protocol-coordinator.ts";
import { createBaselineUpdate, createProtocolUpdate } from "./host-update.ts";
import { parseProtocolSnapshot } from "./protocol/parse.ts";
import { frame, snapshot } from "./protocol-test-fixtures.ts";
import { applyProtocolFrame, setSnapshot, store } from "../store/store.ts";

test("production coordinator consumes delta before the existing chart frame callback without a full snapshot replacement", () => {
  const baseline = parseProtocolSnapshot({ ...snapshot(0, 0), accounts: {
    "0": { cash: "100000", reserved_cash: "0", reserved_sell_qty: {}, positions: {} },
  } });
  const failures: unknown[] = [];
  const coordinator = new ProtocolCoordinator({
    onBaseline: (state) => { store.dispatch(setSnapshot(state.snapshot)); },
    onApplied: (reduction) => {
      if (reduction.update.kind !== "tick-batch") return;
      const last = reduction.update.frames.at(-1)!;
      store.dispatch(applyProtocolFrame({ tick: last.tick, seq: last.seqTo, markets: last.markets, activeDailyCandles: last.activeDailyCandles }));
    },
    onFailure: (failure) => failures.push(failure),
  });
  coordinator.accept(createBaselineUpdate("1", baseline));
  const before = store.getState().snapshot.snapshot!;
  coordinator.accept(createProtocolUpdate("1", { TickBatch: {
    frames: [frame(1, 0, ["600000"])], runtime_snapshot: null,
    runtime_delta: {
      seq_from: 0, seq_to: 1, tick: 1, day: 0, phase: "CallAuction",
      accounts: { "0": { cash: "99000", reserved_cash: "1000", reserved_sell_qty: {}, positions: {} } },
      working_orders: { reset: true, upserts: [{ id: 9, code: "600000", side: "Buy", price: "1000", remainingQty: 100, venue: "auction", frozen: "cash" }], removed: [] },
    },
  } }));
  assert.deepEqual(failures, []);
  const after = store.getState().snapshot;
  assert.equal(after.snapshot!.accounts[0]!.cash, "99000");
  assert.equal(after.snapshot!.accounts[0]!.reserved_cash, "1000");
  assert.equal(after.snapshot!.phase, "CallAuction");
  assert.equal(after.snapshot!.daily_candles, before.daily_candles);
  assert.deepEqual(Reflect.get(after, "playerWorkingOrders"), {
    "9": { id: 9, code: "600000", side: "Buy", price: "1000", remainingQty: 100, venue: "auction", frozen: "cash" },
  });
});

test("a reconnect baseline can seed exact-cursor public orders before consuming a non-reset delta", () => {
  const baseline = parseProtocolSnapshot({ ...snapshot(5, 5), accounts: {
    "0": { cash: "100000", reserved_cash: "0", reserved_sell_qty: {}, positions: {} },
  } });
  const failures: unknown[] = [];
  const coordinator = new ProtocolCoordinator({ onBaseline: () => {}, onApplied: () => {}, onFailure: (failure) => failures.push(failure) });
  coordinator.accept(createBaselineUpdate("2", baseline));
  const seed = Reflect.get(coordinator, "installPlayerWorkingOrdersBaseline");
  assert.equal(typeof seed, "function");
  seed.call(coordinator, { generation: "2", tick: 5, seq: 5 }, [
    { id: 9, code: "600000", side: "Buy", price: "1000", remainingQty: 100, venue: "continuous", frozen: "cash" },
  ]);
  coordinator.accept(createProtocolUpdate("2", { TickBatch: {
    frames: [frame(6, 5, ["600000"])], runtime_snapshot: null,
    runtime_delta: {
      seq_from: 5, seq_to: 6, tick: 6, day: 0, phase: "Continuous", accounts: {},
      working_orders: { reset: false, upserts: [], removed: [9] },
    },
  } }));
  assert.deepEqual(failures, []);
  const status = coordinator.status();
  assert.equal(status.kind, "ready");
  if (status.kind !== "ready") throw new Error("the coordinator did not publish the delta");
  assert.deepEqual(status.state.playerWorkingOrders, {});
  assert.deepEqual(Reflect.get(store.getState().snapshot, "playerWorkingOrders"), {});
});

test("late or cross-generation baseline order seeds cannot overwrite the current protocol and store", () => {
  const baseline = parseProtocolSnapshot({ ...snapshot(5, 5), accounts: {
    "0": { cash: "100000", reserved_cash: "0", reserved_sell_qty: {}, positions: {} },
  } });
  const coordinator = new ProtocolCoordinator({ onBaseline: () => {}, onApplied: () => {}, onFailure: () => {} });
  coordinator.accept(createBaselineUpdate("2", baseline));
  const seed = Reflect.get(coordinator, "installPlayerWorkingOrdersBaseline");
  assert.equal(typeof seed, "function");
  const before = coordinator.status();
  const storeBefore = store.getState().snapshot;
  for (const cursor of [{ generation: "1", tick: 5, seq: 5 }, { generation: "2", tick: 6, seq: 5 }, { generation: "2", tick: 5, seq: 4 }]) {
    assert.throws(() => seed.call(coordinator, cursor, []), { code: "PROTOCOL_CURSOR" });
    assert.equal(coordinator.status(), before);
    assert.equal(store.getState().snapshot, storeBefore);
  }
});
