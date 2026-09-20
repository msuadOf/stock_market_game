import assert from "node:assert/strict";
import test from "node:test";
import { reduceEngineUpdate } from "./protocol/reduce.ts";
import {
  baseState,
  firstFrame,
  frame,
  recordArray,
  snapshot,
  tickBatch,
} from "./protocol-test-fixtures.ts";
import type { JsonRecord, JsonValue } from "./protocol-test-fixtures.ts";

test("Given three frames including an empty one, when reduced, then every frame and timeseries survives", () => {
  const result = reduceEngineUpdate(
    baseState(),
    "generation-1",
    tickBatch([frame(1, 0, ["600000"]), frame(2, 1, []), frame(3, 1, ["600002"])], snapshot(3, 2)),
  );

  assert.equal(result.kind, "applied");
  assert.deepEqual(result.state.intraday.map((item) => [item.tick, item.seqFrom, item.seqTo]), [[1, 0, 1], [2, 1, 1], [3, 1, 2]]);
});

test("Given facts and events reversed together, when replayed, then it is exact and emits no effects", () => {
  const update = tickBatch([frame(1, 0, ["600000", "600001"])], snapshot(1, 2));
  const first = reduceEngineUpdate(baseState(), "generation-1", update);
  assert.equal(first.kind, "applied");
  const original = firstFrame(update.TickBatch);
  const reversed = tickBatch([{
    ...original,
    events: [...recordArray(original.events, "events")].reverse(),
    facts: [...recordArray(original.facts, "facts")].reverse(),
  }], snapshot(1, 2));

  const retried = reduceEngineUpdate(first.state, "generation-1", reversed);

  assert.equal(retried.kind, "exact-retry");
  assert.deepEqual(retried.effects, []);
});

test("Given an event-fact pair moved between valid frames, when replayed, then it rejects changed tick boundaries", () => {
  const original = tickBatch([frame(1, 0, ["600000"]), frame(2, 1, ["600001"])], snapshot(2, 2));
  const first = reduceEngineUpdate(baseState(), "generation-1", original);
  assert.equal(first.kind, "applied");

  assert.throws(
    () => reduceEngineUpdate(first.state, "generation-1", tickBatch([
      frame(1, 0, ["600001"]),
      frame(2, 1, ["600000"]),
    ], snapshot(2, 2))),
    { code: "PROTOCOL_REPLAY" },
  );
});

test("Given a coherent mutation ending at an accepted cursor, when replayed, then it rejects PROTOCOL_REPLAY", () => {
  const accepted = reduceEngineUpdate(baseState(), "generation-1", tickBatch([frame(1, 0, ["600000"])], snapshot(1, 1)));
  assert.equal(accepted.kind, "applied");

  assert.throws(
    () => reduceEngineUpdate(accepted.state, "generation-1", tickBatch([frame(1, 0, ["600001"])], snapshot(1, 1))),
    { code: "PROTOCOL_REPLAY" },
  );
});

test("Given missing, duplicate, or mutated facts, when reduced, then input state remains unchanged", () => {
  const state = baseState();
  const valid = tickBatch([frame(1, 0, ["600000", "600001"])], snapshot(1, 2));
  const current = firstFrame(valid.TickBatch);
  const facts = recordArray(current.facts, "facts");
  const missingFact = tickBatch([{ ...current, facts: [facts[0] ?? {}] }], snapshot(1, 2));
  const duplicateFact = tickBatch([{ ...current, facts: [facts[0] ?? {}, facts[0] ?? {}] }], snapshot(1, 1));
  const original = facts[0];
  if (original === undefined) throw new Error("test fixture invalid");
  const mutatedFact = tickBatch([{ ...current, facts: [{ ...original, canonical_payload: "{}" }, facts[1] ?? {}] }], snapshot(1, 2));

  for (const malformed of [missingFact, duplicateFact, mutatedFact]) {
    assert.throws(() => reduceEngineUpdate(state, "generation-1", malformed), { code: "PROTOCOL_MALFORMED" });
    assert.deepEqual(state.cursor, { generation: "generation-1", tick: 0, seq: 0 });
  }
});

test("Given a snapshot mismatch or cursor gap, when reduced, then input state remains unchanged", () => {
  const state = baseState();
  const snapshotSequenceMismatch = tickBatch([frame(1, 0, ["600000"])], snapshot(1, 2));
  const snapshotTickMismatch = tickBatch([frame(1, 0, ["600000"])], snapshot(2, 1));
  const cursorGap = tickBatch([frame(2, 1, ["600000"])], snapshot(2, 2));

  for (const malformed of [snapshotSequenceMismatch, snapshotTickMismatch, cursorGap]) {
    assert.throws(() => reduceEngineUpdate(state, "generation-1", malformed));
    assert.deepEqual(state.cursor, { generation: "generation-1", tick: 0, seq: 0 });
  }
});

test("Given a tick or sequence gap inside one batch, when reduced, then the whole batch is rejected", () => {
  const state = baseState();
  const tickGap = tickBatch([frame(1, 0, ["600000"]), frame(3, 1, ["600001"])], snapshot(3, 2));
  const sequenceGap = tickBatch([frame(1, 0, ["600000"]), frame(2, 2, ["600001"])], snapshot(2, 3));

  for (const malformed of [tickGap, sequenceGap]) {
    assert.throws(() => reduceEngineUpdate(state, "generation-1", malformed), { code: "PROTOCOL_MALFORMED" });
    assert.deepEqual(state.cursor, { generation: "generation-1", tick: 0, seq: 0 });
  }
});

test("Given reversed fact arrays, when reduced, then effects and canonical state are invariant", () => {
  const original = tickBatch([frame(1, 0, ["600000", "600001"])], snapshot(1, 2));
  const current = firstFrame(original.TickBatch);
  const reversed = tickBatch([{
    ...current,
    events: [...recordArray(current.events, "events")].reverse(),
    facts: [...recordArray(current.facts, "facts")].reverse(),
  }], snapshot(1, 2));

  const left = reduceEngineUpdate(baseState(), "generation-1", original);
  const right = reduceEngineUpdate(baseState(), "generation-1", reversed);

  assert.equal(left.kind, "applied");
  assert.equal(right.kind, "applied");
  assert.deepEqual(left.effects, right.effects);
  assert.deepEqual(left.state.intraday, right.state.intraday);
  assert.deepEqual(left.effects.map((effect) => effect.kind), ["notice", "notice", "automatic-order"]);
});

test("Given JSON and WASM Map forms, when reduced, then they produce equal state", () => {
  const json = tickBatch([frame(1, 0, ["600000"])], snapshot(1, 1));
  const wasm: JsonRecord = {
    TickBatch: new Map<string, JsonValue>([
      ["frames", [frame(1, 0, ["600000"])]],
      ["runtime_snapshot", snapshot(1, 1)],
    ]),
  };

  const jsonResult = reduceEngineUpdate(baseState(), "generation-1", json);
  const wasmResult = reduceEngineUpdate(baseState(), "generation-1", wasm);

  assert.equal(jsonResult.kind, "applied");
  assert.equal(wasmResult.kind, "applied");
  assert.deepEqual(jsonResult.state, wasmResult.state);
});

test("Given null final snapshot or a valid empty frame, when reduced, then the cursor advances without replacing authority", () => {
  const state = baseState();
  const withoutSnapshot = reduceEngineUpdate(state, "generation-1", tickBatch([frame(1, 0, ["600000"])], null));
  assert.equal(withoutSnapshot.kind, "applied");
  assert.deepEqual(withoutSnapshot.state.snapshot, state.snapshot);

  const empty = reduceEngineUpdate(baseState(), "generation-1", tickBatch([frame(1, 0, [])], snapshot(1, 0)));
  assert.equal(empty.kind, "applied");
  assert.deepEqual(empty.state.intraday.map((item) => [item.tick, item.seqFrom, item.seqTo, item.events.length]), [[1, 0, 0, 0]]);
});
