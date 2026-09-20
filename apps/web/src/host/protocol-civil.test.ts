import assert from "node:assert/strict";
import test from "node:test";
import { canonicalJson } from "./protocol/canonical.ts";
import { reduceEngineUpdate } from "./protocol/reduce.ts";
import {
  baseState,
  civilUpdate,
  frame,
  isJsonRecord,
  recordArray,
  snapshot,
  tickBatch,
} from "./protocol-test-fixtures.ts";
import type { JsonRecord } from "./protocol-test-fixtures.ts";

function stateAfterTick() {
  const applied = reduceEngineUpdate(baseState(), "generation-1", tickBatch([frame(1, 0, ["600000"])], snapshot(1, 1)));
  assert.equal(applied.kind, "applied");
  return applied.state;
}

test("Given a CivilUpdate barrier, when reduced before a next tick, then it refreshes authority and preserves seq continuity", () => {
  const barrier = reduceEngineUpdate(stateAfterTick(), "generation-1", civilUpdate());
  assert.equal(barrier.kind, "applied");
  assert.equal(barrier.state.cursor.tick, 1);
  assert.equal(barrier.state.cursor.seq, 2);
  assert.equal(barrier.state.securities[0]?.code, "600000");
  assert.equal(barrier.effects.filter((effect) => effect.kind === "civil-barrier").length, 2);
  assert.deepEqual(
    barrier.effects.filter((effect) => effect.kind === "civil-barrier").map((effect) => effect.barrier),
    ["AfterClose", "BeforeOpen"],
  );

  const next = reduceEngineUpdate(barrier.state, "generation-1", tickBatch([frame(2, 2, ["600000"])], snapshot(2, 3)));
  assert.equal(next.kind, "applied");
  assert.equal(next.state.cursor.seq, 3);
});

test("Given CivilUpdate refresh history, when reduced, then it does not replay historical effects", () => {
  const barrier = reduceEngineUpdate(stateAfterTick(), "generation-1", civilUpdate());

  assert.equal(barrier.kind, "applied");
  assert.deepEqual(barrier.effects.map((effect) => effect.kind), ["civil-barrier", "civil-barrier"]);
});

test("Given two CivilDateAdvanced facts in one barrier, when reduced, then it rejects the ambiguous barrier atomically", () => {
  const raw = civilUpdate();
  const civil = raw["CivilUpdate"];
  if (!isJsonRecord(civil)) throw new Error("test fixture invalid");
  const refresh = civil["refresh"];
  if (!isJsonRecord(refresh)) throw new Error("test fixture invalid");
  const event: JsonRecord = {
    CivilDateAdvanced: {
      seq: 3,
      settled_date: "2030-01-02",
      next_date: "2030-01-03",
      next_status: "Trading",
    },
  };
  const duplicate: JsonRecord = {
    CivilUpdate: {
      ...civil,
      events: [...recordArray(civil["events"], "civil events"), event],
      facts: [...recordArray(civil["facts"], "civil facts"), {
        key: { phase_rank: 6, entity: "Session", source: "Session", local_event_index: 1 },
        event,
        canonical_payload: canonicalJson(event),
      }],
      seq_to: 3,
      refresh: { ...refresh, snapshot: snapshot(1, 3) },
    },
  };
  const state = stateAfterTick();

  assert.throws(() => reduceEngineUpdate(state, "generation-1", duplicate), { code: "PROTOCOL_MALFORMED" });
  assert.deepEqual(state.cursor, { generation: "generation-1", tick: 1, seq: 1 });
});

test("Given an AfterClose refresh whose history does not start at the settled-day first tick, when reduced, then it rejects atomically", () => {
  const raw = civilUpdate();
  const civil = raw["CivilUpdate"];
  if (!isJsonRecord(civil)) throw new Error("test fixture invalid");
  const refresh = civil["refresh"];
  if (!isJsonRecord(refresh)) throw new Error("test fixture invalid");
  const malformed: JsonRecord = {
    CivilUpdate: {
      ...civil,
      tick: 2,
      seq_from: 2,
      seq_to: 3,
      events: [{ CivilDateAdvanced: { seq: 3, settled_date: "2030-01-02", next_date: "2030-01-03", next_status: "Trading" } }],
      facts: [{
        key: { phase_rank: 6, entity: "Session", source: "Session", local_event_index: 0 },
        event: { CivilDateAdvanced: { seq: 3, settled_date: "2030-01-02", next_date: "2030-01-03", next_status: "Trading" } },
        canonical_payload: canonicalJson({ CivilDateAdvanced: { seq: 3, settled_date: "2030-01-02", next_date: "2030-01-03", next_status: "Trading" } }),
      }],
      refresh: {
        ...refresh,
        ticks_per_day: 2,
        snapshot: snapshot(2, 3),
        intraday: [frame(2, 0, ["600000"]), frame(3, 1, ["600000"])],
      },
    },
  };
  const prior = reduceEngineUpdate(baseState(), "generation-1", tickBatch([
    frame(1, 0, ["600000"]),
    frame(2, 1, ["600000"]),
  ], snapshot(2, 2)));
  assert.equal(prior.kind, "applied");

  assert.throws(() => reduceEngineUpdate(prior.state, "generation-1", malformed), { code: "PROTOCOL_MALFORMED" });
  assert.deepEqual(prior.state.cursor, { generation: "generation-1", tick: 2, seq: 2 });
});
