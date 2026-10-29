import { catalogFixtureForSnapshot, defaultCatalogFixture } from "../catalog-test-fixture.ts";
import assert from "node:assert/strict";
import test from "node:test";
import { createProtocolState, ProtocolError } from "./types.ts";
import { parseEngineUpdate, parseProtocolSnapshot } from "./parse.ts";
import { reduceEngineUpdate } from "./reduce.ts";
import { civilUpdate, frame, snapshot } from "../protocol-test-fixtures.ts";
import type { RuntimeMirrorOrigin, WireEngineUpdate } from "./stock-identity.ts";

// wire 形态：持仓键与活动委托 code 为数值 StockId（ADR 0041）；id 0 ↔ 代码串 "000812"。
const account = {
  cash: "100000", reserved_cash: "1000", reserved_sell_qty: {},
  positions: { "0": { qty: 100, t1_locked: 100, invested_cents: "1000", recovered_cents: "0" } },
};
const order = { id: 1, code: 0, side: "Buy" as const, price: "1000", remainingQty: 100, venue: "Continuous" as const, frozen: "Cash" as const };
/** 摄入翻译后（代码串）的同一委托，用于断言 store/状态侧的期望。 */
const codedOrder = { ...order, code: "000812" };

function initial() {
  const parsed = parseProtocolSnapshot({ ...snapshot(0, 0), mirror_origin: origin(0, 1), accounts: { "0": account } }, "test.fixture", defaultCatalogFixture);
  return createProtocolState(parsed, "1", catalogFixtureForSnapshot(parsed));
}

type DeltaWire = {
  tick: number; seq_from: number; seq_to: number; day: number; phase: string;
  mirror_origin: Readonly<Record<string, unknown>>;
  accounts: Readonly<Record<string, unknown>>;
  working_orders: { reset: boolean; upserts: readonly unknown[]; removed: readonly number[] };
};

function origin(sourceTick: number, sourceGeneration = 0, authorityEpoch = 1, installSequence = 2): RuntimeMirrorOrigin {
  return { authority_epoch: authorityEpoch, install_sequence: installSequence, source_tick: sourceTick, source_generation: sourceGeneration, day_end_stage: "tick", non_tick_revision: 0, mirror_phase: "Continuous" };
}

function delta(tick = 1, seqFrom = 0, seqTo = 1): DeltaWire {
  return {
    tick, seq_from: seqFrom, seq_to: seqTo, day: 0, phase: "Continuous", mirror_origin: origin(tick, 1),
    accounts: { "0": { ...account, cash: "99000" } },
    working_orders: { reset: tick === 1, upserts: [order], removed: [] as number[] },
  };
}

function batch(runtimeDelta = delta(), runtimeSnapshot: unknown = null) {
  return { TickBatch: {
    frames: [frame(runtimeDelta.tick, runtimeDelta.seq_from, [0])],
    runtime_snapshot: runtimeSnapshot, runtime_delta: runtimeDelta,
  } };
}

test("runtime delta consumes account reservations, T+1, clock and player orders without replacing history", () => {
  const state = initial();
  let reduction: ReturnType<typeof reduceEngineUpdate> | undefined;
  assert.doesNotThrow(() => { reduction = reduceEngineUpdate(state, "1", batch()); });
  assert.ok(reduction);
  assert.equal(reduction.state.snapshot.accounts[0]!.cash, "99000");
  assert.equal(reduction.state.snapshot.accounts[0]!.positions["000812"]!.t1_locked, 100);
  assert.equal(reduction.state.snapshot.accounts[0]!.reserved_cash, "1000");
  assert.equal(reduction.state.snapshot.daily_candles, state.snapshot.daily_candles);
  assert.deepEqual([reduction.state.snapshot.tick, reduction.state.snapshot.seq], [1, 1]);
  assert.deepEqual(Reflect.get(reduction.state, "playerWorkingOrders"), { "1": codedOrder });
  assert.equal(reduction.update.kind, "tick-batch");
  assert.deepEqual(Reflect.get(reduction.update, "runtimeDelta"), {
    ...delta(), working_orders: { reset: true, upserts: [codedOrder], removed: [] },
    accounts: parseProtocolSnapshot({ ...snapshot(0, 0), accounts: delta().accounts }, "test.fixture", defaultCatalogFixture).accounts,
  });
});

test("subsequent delta replaces remaining quantity and removes filled/canceled orders without touching unchanged accounts", () => {
  let first: ReturnType<typeof reduceEngineUpdate> | undefined;
  assert.doesNotThrow(() => { first = reduceEngineUpdate(initial(), "1", batch()); });
  assert.ok(first);
  const partial = { ...delta(2, 1, 2), accounts: {}, working_orders: { reset: false, upserts: [{ ...order, remainingQty: 50 }], removed: [] } };
  const second = reduceEngineUpdate(first.state, "1", batch(partial));
  assert.equal(second.state.snapshot.accounts, first.state.snapshot.accounts);
  assert.deepEqual(Reflect.get(second.state, "playerWorkingOrders"), { "1": { ...codedOrder, remainingQty: 50 } });
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
  assert.throws(() => reduceEngineUpdate(first!.state, "1", batch({ ...delta(), accounts: { "0": { ...account, cash: "98000" } } })), { code: "PROTOCOL_REPLAY" });
});

test("full-snapshot batches and complete baselines remain supported alongside explicit delta batches", () => {
  const current = parseProtocolSnapshot(snapshot(1, 1), "test.fixture", defaultCatalogFixture);
  const parsed = parseEngineUpdate({ TickBatch: { frames: [frame(1, 0, [0])], runtime_snapshot: current } });
  assert.equal("TickBatch" in parsed && parsed.TickBatch.runtime_snapshot?.tick, 1);
  let reduction: ReturnType<typeof reduceEngineUpdate> | undefined;
  assert.doesNotThrow(() => { reduction = reduceEngineUpdate(initial(), "1", batch()); });
  assert.ok(reduction);
  const newBaseline = createProtocolState(current, "2", catalogFixtureForSnapshot(current));
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
    assert.equal(state.snapshot.accounts[0]!.cash, "100000");
  }
});

test("delta rejects NPC account data, invalid reservations and inconsistent order patches", () => {
  for (const runtimeDelta of [
    { ...delta(), accounts: { "1": account } },
    { ...delta(), accounts: { "00": account } },
    { ...delta(), accounts: { "0": { ...account, reserved_cash: "100001" } } },
    { ...delta(), accounts: { "0": { ...account, positions: { "0": { ...account.positions["0"]!, t1_locked: 101 } } } } },
    { ...delta(), working_orders: { reset: true, upserts: [order, order], removed: [] } },
    { ...delta(), working_orders: { reset: false, upserts: [order], removed: [1] } },
    { ...delta(), working_orders: { reset: true, upserts: [], removed: [1] } },
    { ...delta(), working_orders: { reset: true, upserts: [{ ...order, remainingQty: 0 }], removed: [] } },
    { ...delta(), working_orders: { reset: true, upserts: [{ ...order, owner: "1" }], removed: [] } },
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
    frames: [frame(2, 1, [0])],
    runtime_snapshot: { ...snapshot(2, 2), mirror_origin: origin(2, 1), accounts: { "0": account } },
  } });
  assert.deepEqual(Reflect.get(full.state, "playerWorkingOrders"), {});
  assert.equal(Reflect.get(full.state, "playerOrdersReady"), false);
  assert.throws(() => reduceEngineUpdate(full.state, "1", batch(delta(3, 2, 3))), { code: "PROTOCOL_MALFORMED" });
  const reset = reduceEngineUpdate(full.state, "1", batch({ ...delta(3, 2, 3), working_orders: { reset: true, upserts: [], removed: [] } }));
  assert.deepEqual(Reflect.get(reset.state, "playerWorkingOrders"), {});
});


test("runtime delta keeps an older same-generation mirror origin while tick facts advance", () => {
  const earlier = reduceEngineUpdate(initial(), "1", batch());
  const lagging = { ...delta(2, 1, 2), mirror_origin: origin(1, 1) };
  const next = reduceEngineUpdate(earlier.state, "1", batch(lagging));
  assert.deepEqual(next.state.snapshot.mirror_origin, origin(1, 1));
  assert.equal(next.state.cursor.tick, 2);
  assert.equal(next.update.kind, "tick-batch");
});

test("runtime delta rejects a regressing mirror install cursor and authority epoch", () => {
  const earlier = reduceEngineUpdate(initial(), "1", batch());
  for (const mirror_origin of [
    origin(2, 1, 1, 1),
    origin(2, 1, 0, 3),
  ]) {
    assert.throws(() => reduceEngineUpdate(earlier.state, "1", batch({ ...delta(2, 1, 2), mirror_origin })), { code: "PROTOCOL_MALFORMED" });
  }
  assert.throws(() => reduceEngineUpdate(earlier.state, "1", batch({
    ...delta(2, 1, 2), mirror_origin: origin(2, 2, 0, 0),
  })), { code: "PROTOCOL_MALFORMED" });
  const newBaseline = parseProtocolSnapshot({ ...snapshot(0, 0), mirror_origin: origin(0, 2, 0, 0) }, "test.baseline", defaultCatalogFixture);
  assert.deepEqual(createProtocolState(newBaseline, "2", defaultCatalogFixture).snapshot.mirror_origin, origin(0, 2, 0, 0));
});

test("runtime origin publication generation may change while the host generation cursor advances", () => {
  for (const [authorityEpoch, installSequence] of [[1, 2], [1, 3], [2, 0]] as const) {
    const state = createProtocolState(
      parseProtocolSnapshot({ ...snapshot(0, 0), mirror_origin: origin(0, 2, 1, 2), accounts: { "0": account } }, "test.fixture", defaultCatalogFixture),
      "1",
      defaultCatalogFixture,
    );
    const nextOrigin = origin(1, 0, authorityEpoch, installSequence);
    const next = reduceEngineUpdate(state, "1", batch({
      ...delta(), mirror_origin: nextOrigin,
    }));
    assert.deepEqual(next.state.snapshot.mirror_origin, nextOrigin);
  }
});

test("exact delta, snapshot, and civil retries survive later mirror-origin advancement", () => {
  const firstDelta = batch();
  const deltaState = reduceEngineUpdate(initial(), "1", firstDelta).state;
  const afterDelta = reduceEngineUpdate(deltaState, "1", batch({
    ...delta(2, 1, 2), mirror_origin: origin(2, 0, 1, 3),
    accounts: {}, working_orders: { reset: false, upserts: [], removed: [] },
  })).state;
  const retriedDelta = reduceEngineUpdate(afterDelta, "1", firstDelta);
  assert.equal(retriedDelta.kind, "exact-retry");
  assert.equal(retriedDelta.state, afterDelta);
  assert.deepEqual(retriedDelta.effects, []);

  const firstSnapshot = { TickBatch: {
    frames: [frame(1, 0, [0])], runtime_snapshot: { ...snapshot(1, 1), mirror_origin: origin(1, 1, 1, 2) },
  } };
  const snapshotState = reduceEngineUpdate(initial(), "1", firstSnapshot).state;
  const afterSnapshot = reduceEngineUpdate(snapshotState, "1", batch({
    ...delta(2, 1, 2), mirror_origin: origin(2, 0, 1, 3),
    accounts: {}, working_orders: { reset: true, upserts: [], removed: [] },
  })).state;
  const retriedSnapshot = reduceEngineUpdate(afterSnapshot, "1", firstSnapshot);
  assert.equal(retriedSnapshot.kind, "exact-retry");
  assert.equal(retriedSnapshot.state, afterSnapshot);
  assert.deepEqual(retriedSnapshot.effects, []);

  const firstCivilTick = reduceEngineUpdate(initial(), "1", batch());
  const parsedCivil = parseEngineUpdate(civilUpdate());
  assert.ok("CivilUpdate" in parsedCivil);
  const firstCivil: WireEngineUpdate = { CivilUpdate: {
    ...parsedCivil.CivilUpdate,
    refresh: {
      ...parsedCivil.CivilUpdate.refresh,
      snapshot: { ...parsedCivil.CivilUpdate.refresh.snapshot, mirror_origin: origin(1, 2, 1, 3) },
    },
  } };
  const civilState = reduceEngineUpdate(firstCivilTick.state, "1", firstCivil).state;
  const afterCivil = reduceEngineUpdate(civilState, "1", {
    TickBatch: {
      frames: [frame(2, 2, [0])],
      runtime_snapshot: null,
      runtime_delta: {
        ...delta(2, 2, 3), mirror_origin: origin(2, 0, 1, 4),
        accounts: {}, working_orders: { reset: true, upserts: [], removed: [] },
      },
    },
  }).state;
  const retriedCivil = reduceEngineUpdate(afterCivil, "1", firstCivil);
  assert.equal(retriedCivil.kind, "exact-retry");
  assert.equal(retriedCivil.state, afterCivil);
  assert.deepEqual(retriedCivil.effects, []);
});
