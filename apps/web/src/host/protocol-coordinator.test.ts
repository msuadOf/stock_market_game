import assert from "node:assert/strict";
import test from "node:test";
import { ProtocolCoordinator } from "./protocol-coordinator.ts";
import { parseEngineUpdate } from "./protocol/parse.ts";
import { canonicalJson } from "./protocol/canonical.ts";
import { frame, snapshot, tickBatch } from "./protocol-test-fixtures.ts";

function baseline() {
  const parsed = parseEngineUpdate(tickBatch([frame(0, 0, [])], snapshot(0, 0)));
  if (!("TickBatch" in parsed) || parsed.TickBatch.runtime_snapshot === null) {
    throw new Error("test fixture invalid");
  }
  return {
    type: "baseline" as const,
    generation: "generation-1",
    snapshot: parsed.TickBatch.runtime_snapshot,
    civilDate: null,
    revision: null,
    securities: [],
    publicPublicationIds: [],
  };
}

test("Given a three-frame protocol batch, when the coordinator accepts it, then it publishes one reduction with every frame", () => {
  const reductions: number[] = [];
  const coordinator = new ProtocolCoordinator({
    onBaseline: () => {},
    onApplied: (reduction) => reductions.push(reduction.state.intraday.length),
    onFailure: () => {},
  });

  coordinator.accept(baseline());
  coordinator.accept({
    type: "protocol",
    generation: "generation-1",
    update: tickBatch([
      frame(1, 0, ["600000"]),
      frame(2, 1, []),
      frame(3, 1, ["600002"]),
    ], snapshot(3, 2)),
    civilDate: null,
    revision: null,
  });

  assert.deepEqual(reductions, [3]);
});

test("Given an exact retry, when the coordinator accepts it, then it publishes nothing", () => {
  let publications = 0;
  const update = tickBatch([frame(1, 0, ["600000"])], snapshot(1, 1));
  const coordinator = new ProtocolCoordinator({
    onBaseline: () => {},
    onApplied: () => { publications += 1; },
    onFailure: () => {},
  });

  coordinator.accept(baseline());
  coordinator.accept({ type: "protocol", generation: "generation-1", update, civilDate: null, revision: null });
  coordinator.accept({ type: "protocol", generation: "generation-1", update, civilDate: null, revision: null });

  assert.equal(publications, 1);
});

test("Given malformed or stale-generation protocol updates, when accepted, then the coordinator preserves state and reports a typed failure", () => {
  const failures: Array<{ code: string; where: string }> = [];
  const coordinator = new ProtocolCoordinator({
    onBaseline: () => {},
    onApplied: () => {},
    onFailure: (failure) => failures.push({ code: failure.code, where: failure.where }),
  });
  coordinator.accept(baseline());
  const initial = coordinator.status();

  coordinator.accept({ type: "protocol", generation: "generation-0", update: {}, civilDate: null, revision: null });
  coordinator.accept({ type: "protocol", generation: "generation-1", update: { Wrong: {} }, civilDate: null, revision: null });

  assert.equal(initial.kind, "ready");
  assert.equal(coordinator.status().kind, "failure");
  assert.deepEqual(failures, [
    { code: "PROTOCOL_CURSOR", where: "protocol.reduce.generation" },
    { code: "PROTOCOL_MALFORMED", where: "EngineUpdate" },
  ]);
});

test("Given a fresh baseline after resync, when a new generation update arrives, then prior replay history is discarded", () => {
  let publications = 0;
  const coordinator = new ProtocolCoordinator({
    onBaseline: () => {},
    onApplied: () => { publications += 1; },
    onFailure: () => {},
  });
  const update = tickBatch([frame(1, 0, ["600000"])], snapshot(1, 1));

  coordinator.accept(baseline());
  coordinator.accept({ type: "protocol", generation: "generation-1", update, civilDate: null, revision: null });
  const next = baseline();
  coordinator.accept({ ...next, generation: "generation-2" });
  coordinator.accept({ type: "protocol", generation: "generation-2", update, civilDate: null, revision: null });

  assert.equal(publications, 2);
});

test("Given an already-published tick batch, when a CivilUpdate shares its tick barrier, then the coordinator accepts it", () => {
  let publications = 0;
  const coordinator = new ProtocolCoordinator({
    onBaseline: () => {},
    onApplied: () => { publications += 1; },
    onFailure: () => {},
  });
  coordinator.accept(baseline());
  coordinator.accept({ type: "protocol", generation: "generation-1", update: tickBatch([frame(1, 0, ["600000"])], snapshot(1, 1)), civilDate: null, revision: null });
  coordinator.accept({ type: "protocol", generation: "generation-1", update: {
    CivilUpdate: {
      boundary: { settled_date: "2030-01-02", settled_phase: "IntradayTrading", next_date: "2030-01-03", next_status: "Trading" },
      kinds: ["AfterClose", "BeforeOpen"],
      tick: 1,
      civil_date: "2030-01-03",
      events: [{ CivilDateAdvanced: { seq: 2, settled_date: "2030-01-02", next_date: "2030-01-03", next_status: "Trading" } }],
      facts: (() => {
        const event = { CivilDateAdvanced: { seq: 2, settled_date: "2030-01-02", next_date: "2030-01-03", next_status: "Trading" } };
        return [{ key: { phase_rank: 6, entity: "Session", source: "Session", local_event_index: 0 }, event, canonical_payload: canonicalJson(event) }];
      })(),
      seq_from: 1,
      seq_to: 2,
      refresh: { ticks_per_day: 1, snapshot: snapshot(1, 2), securities: [{ code: "600000", exchange: "Shanghai", initial_price: 1_000, category: "MainBoard", limit_pct: 0.1, tick: 1, total_shares: "1000000", float_shares: 1_000_000 }], intraday: [frame(1, 0, ["600000"])], public_publication_ids: [] },
    },
  }, civilDate: "2030-01-03", revision: "1" });
  assert.equal(publications, 2);
});
