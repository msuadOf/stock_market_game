import assert from "node:assert/strict";
import test from "node:test";
import { createProtocolState, ProtocolError } from "./types.ts";
import { parseEngineUpdate, parseProtocolSnapshot } from "./parse.ts";
import { reduceEngineUpdate } from "./reduce.ts";
import { frame, snapshot } from "../protocol-test-fixtures.ts";

const account = {
  cash: 100_000, reserved_cash: 1_000, reserved_sell_qty: {},
  positions: { "600000": { qty: 100, t1_locked: 100, invested_cents: 1_000, recovered_cents: 0 } },
};
const order = { id: 1, code: "600000", side: "Buy", price: 1_000, remainingQty: 100, venue: "continuous", frozen: "cash" };

function initial() {
  return createProtocolState(parseProtocolSnapshot({ ...snapshot(0, 0), accounts: { "0": account } }), "1");
}

type DeltaWire = {
  tick: number; seq_from: number; seq_to: number; day: number; phase: string;
  accounts: Readonly<Record<string, unknown>>;
  working_orders: { reset: boolean; upserts: readonly unknown[]; removed: readonly number[] };
};

function delta(tick = 1, seqFrom = 0, seqTo = 1): DeltaWire {
  return {
    tick, seq_from: seqFrom, seq_to: seqTo, day: 0, phase: "Continuous",
    accounts: { "0": { ...account, cash: 99_000 } },
    working_orders: { reset: tick === 1, upserts: [order], removed: [] as number[] },
  };
}

function batch(runtimeDelta = delta(), runtimeSnapshot: unknown = null) {
  return { TickBatch: {
    frames: [frame(runtimeDelta.tick, runtimeDelta.seq_from, ["600000"])],
    runtime_snapshot: runtimeSnapshot, runtime_delta: runtimeDelta,
  } };
}

test("runtime delta consumes account reservations, T+1, clock and player orders without replacing history", () => {
  const state = initial();
  let reduction: ReturnType<typeof reduceEngineUpdate> | undefined;
  assert.doesNotThrow(() => { reduction = reduceEngineUpdate(state, "1", batch()); });
  assert.ok(reduction);
  assert.equal(reduction.state.snapshot.accounts[0]!.cash, 99_000);
  assert.equal(reduction.state.snapshot.accounts[0]!.positions["600000"]!.t1_locked, 100);
  assert.equal(reduction.state.snapshot.accounts[0]!.reserved_cash, 1_000);
  assert.equal(reduction.state.snapshot.daily_candles, state.snapshot.daily_candles);
  assert.deepEqual([reduction.state.snapshot.tick, reduction.state.snapshot.seq], [1, 1]);
  assert.deepEqual(Reflect.get(reduction.state, "playerWorkingOrders"), { "1": order });
  assert.equal(reduction.update.kind, "tick-batch");
  assert.deepEqual(Reflect.get(reduction.update, "runtimeDelta"), {
    ...delta(), accounts: parseProtocolSnapshot({ ...snapshot(0, 0), accounts: delta().accounts }).accounts,
  });
});

test("subsequent delta replaces remaining quantity and removes filled/canceled orders without touching unchanged accounts", () => {
  let first: ReturnType<typeof reduceEngineUpdate> | undefined;
  assert.doesNotThrow(() => { first = reduceEngineUpdate(initial(), "1", batch()); });
  assert.ok(first);
  const partial = { ...delta(2, 1, 2), accounts: {}, working_orders: { reset: false, upserts: [{ ...order, remainingQty: 50 }], removed: [] } };
  const second = reduceEngineUpdate(first.state, "1", batch(partial));
  assert.equal(second.state.snapshot.accounts, first.state.snapshot.accounts);
  assert.deepEqual(Reflect.get(second.state, "playerWorkingOrders"), { "1": { ...order, remainingQty: 50 } });
  const removed = { ...delta(3, 2, 3), accounts: {}, working_orders: { reset: false, upserts: [], removed: [1] } };
  const third = reduceEngineUpdate(second.state, "1", batch(removed));
  assert.deepEqual(Reflect.get(third.state, "playerWorkingOrders"), {});
});

test("delta exact retry is effect-free but mutation at the accepted cursor is rejected", () => {
  let first: ReturnType<typeof reduceEngineUpdate> | undefined;
  assert.doesNotThrow(() => { first = reduceEngineUpdate(initial(), "1", batch()); });
  assert.ok(first);
  const retry = reduceEngineUpdate(first.state, "1", batch());
  assert.equal(retry.kind, "exact-retry");
  assert.equal(retry.state, first.state);
  assert.deepEqual(retry.effects, []);
  assert.throws(() => reduceEngineUpdate(first!.state, "1", batch({ ...delta(), accounts: { "0": { ...account, cash: 98_000 } } })), { code: "PROTOCOL_REPLAY" });
});

test("full-snapshot batches and complete baselines remain supported alongside explicit delta batches", () => {
  const current = parseProtocolSnapshot(snapshot(1, 1));
  const parsed = parseEngineUpdate({ TickBatch: { frames: [frame(1, 0, ["600000"])], runtime_snapshot: current } });
  assert.equal("TickBatch" in parsed && parsed.TickBatch.runtime_snapshot?.tick, 1);
  let reduction: ReturnType<typeof reduceEngineUpdate> | undefined;
  assert.doesNotThrow(() => { reduction = reduceEngineUpdate(initial(), "1", batch()); });
  assert.ok(reduction);
  const newBaseline = createProtocolState(current, "2");
  assert.deepEqual(Reflect.get(newBaseline, "playerWorkingOrders"), {});
  assert.throws(() => reduceEngineUpdate(newBaseline, "1", batch()), { code: "PROTOCOL_CURSOR" });
});

test("delta rejects mismatched cursors and simultaneous full snapshot authority atomically", () => {
  for (const update of [
    { TickBatch: { ...batch().TickBatch, runtime_delta: { ...delta(), seq_from: 1 } } },
    { TickBatch: { ...batch().TickBatch, runtime_delta: { ...delta(), seq_to: 2 } } },
    { TickBatch: { ...batch().TickBatch, runtime_delta: { ...delta(), tick: 2 } } },
    batch(delta(), snapshot(1, 1)),
  ]) {
    const state = initial();
    assert.throws(() => reduceEngineUpdate(state, "1", update), (error: unknown) => {
      assert.ok(error instanceof ProtocolError);
      assert.match(error.where, /runtime_delta/);
      return true;
    });
    assert.deepEqual(state.cursor, { generation: "1", tick: 0, seq: 0 });
    assert.equal(state.snapshot.accounts[0]!.cash, 100_000);
  }
});

test("delta rejects NPC account data, invalid reservations and inconsistent order patches", () => {
  for (const runtimeDelta of [
    { ...delta(), accounts: { "1": account } },
    { ...delta(), accounts: { "00": account } },
    { ...delta(), accounts: { "0": { ...account, reserved_cash: 100_001 } } },
    { ...delta(), accounts: { "0": { ...account, positions: { "600000": { ...account.positions["600000"], t1_locked: 101 } } } } },
    { ...delta(), working_orders: { reset: true, upserts: [order, order], removed: [] } },
    { ...delta(), working_orders: { reset: false, upserts: [order], removed: [1] } },
    { ...delta(), working_orders: { reset: true, upserts: [], removed: [1] } },
    { ...delta(), working_orders: { reset: true, upserts: [{ ...order, remainingQty: 0 }], removed: [] } },
    { ...delta(), working_orders: { reset: true, upserts: [{ ...order, owner: 1 }], removed: [] } },
  ]) {
    assert.throws(() => reduceEngineUpdate(initial(), "1", batch(runtimeDelta)), (error: unknown) => {
      assert.ok(error instanceof ProtocolError);
      assert.match(error.where, /runtime_delta/);
      return true;
    });
  }
});

test("delta cannot remove unknown orders or skip the explicit order reset after a baseline", () => {
  const state = initial();
  for (const workingOrders of [
    { reset: false, upserts: [], removed: [99] },
    { reset: false, upserts: [order], removed: [] },
  ]) {
    assert.throws(() => reduceEngineUpdate(state, "1", batch({ ...delta(), working_orders: workingOrders })), (error: unknown) => {
      assert.ok(error instanceof ProtocolError);
      assert.match(error.where, /working_orders/);
      return true;
    });
  }
});

test("a supported full snapshot invalidates order authority and requires a new explicit reset", () => {
  const first = reduceEngineUpdate(initial(), "1", batch());
  const full = reduceEngineUpdate(first.state, "1", { TickBatch: {
    frames: [frame(2, 1, ["600000"])],
    runtime_snapshot: { ...snapshot(2, 2), accounts: { "0": account } },
  } });
  assert.deepEqual(Reflect.get(full.state, "playerWorkingOrders"), {});
  assert.equal(Reflect.get(full.state, "playerOrdersReady"), false);
  assert.throws(() => reduceEngineUpdate(full.state, "1", batch(delta(3, 2, 3))), { code: "PROTOCOL_MALFORMED" });
  const reset = reduceEngineUpdate(full.state, "1", batch({ ...delta(3, 2, 3), working_orders: { reset: true, upserts: [], removed: [] } }));
  assert.deepEqual(Reflect.get(reset.state, "playerWorkingOrders"), {});
});
