import assert from "node:assert/strict";
import test from "node:test";
import { ProtocolCoordinator } from "./protocol-coordinator.ts";
import { parseEngineUpdate } from "./protocol/parse.ts";
import { canonicalJson } from "./protocol/canonical.ts";
import { frame, snapshot, tickBatch } from "./protocol-test-fixtures.ts";
import { buildErrorFeedback } from "../app/error-details.ts";
import type { HostFailure } from "./host-update.ts";

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

test("ProtocolCoordinator cursor 错误保留已知 actual/expected generation、tick 和 seq", { timeout: 10000 }, () => {
  let context: unknown;
  const coordinator = new ProtocolCoordinator({ onBaseline: () => {}, onApplied: () => {}, onFailure: (failure) => { context = failure.context; } });
  coordinator.accept(baseline());
  coordinator.accept({ type: "protocol", generation: "generation-2", update: tickBatch([frame(3, 7, ["600000"])], snapshot(3, 8)), civilDate: null, revision: null });
  assert.deepEqual(context, {
    actual: { generation: "generation-2", kind: "TickBatch", tickFrom: 3, tickTo: 3, seqFrom: 7, seqTo: 8 },
    expected: { generation: "generation-1", tickFrom: 1, seqFrom: 0 },
    cursor: { generation: "generation-1", tick: 0, seq: 0 },
  });
});

test("ProtocolCoordinator 真实复制反馈保留公开游标且未知或双 variant 不猜 expected tick", { timeout: 10000 }, () => {
  let failure: HostFailure | undefined;
  const coordinator = new ProtocolCoordinator({ onBaseline: () => {}, onApplied: () => {}, onFailure: (value) => { failure = value; } });
  coordinator.accept({ ...baseline(), generation: "1" });
  coordinator.accept({ type: "protocol", generation: "2", update: tickBatch([frame(3, 7, ["600000"])], snapshot(3, 8)), civilDate: null, revision: null });
  assert.ok(failure);
  const text = buildErrorFeedback({ ...failure, context: { ...(failure.context as object), password: "PRIVATE_PASSWORD", session_id: "PRIVATE_SESSION" } });
  for (const field of ["tickFrom", "tickTo", "seqFrom", "seqTo", "cursor", "generation", "TickBatch"]) assert.ok(text.includes(field), field);
  assert.match(text, /"tickFrom": 3/);
  assert.match(text, /"seqFrom": 7/);
  assert.doesNotMatch(text, /PRIVATE_PASSWORD|PRIVATE_SESSION/);
  for (const update of [{ unknown: true }, { TickBatch: {}, CivilUpdate: {} }]) {
    coordinator.accept({ ...baseline(), generation: "1" });
    coordinator.accept({ type: "protocol", generation: "2", update, civilDate: null, revision: null });
    const context = failure.context as { actual: Record<string, unknown>; expected: Record<string, unknown> };
    assert.equal(Object.hasOwn(context.expected, "tickFrom"), false);
    assert.equal(Object.hasOwn(context.actual, "kind"), false);
  }
});

test("ProtocolCoordinator onApplied 抛错时反馈引用本次输入 cursor，不捏造下一包 expected", { timeout: 10000 }, () => {
  let failure: HostFailure | undefined;
  const coordinator = new ProtocolCoordinator({ onBaseline: () => {}, onApplied: () => { throw new Error("consumer 处理失败"); }, onFailure: (value) => { failure = value; } });
  coordinator.accept({ ...baseline(), generation: "1" });
  assert.equal(coordinator.accept({ type: "protocol", generation: "1", update: tickBatch([frame(1, 0, ["600000"])], snapshot(1, 1)), civilDate: null, revision: null }), false);
  assert.ok(failure);
  const context = failure.context as { expected: unknown; cursor: unknown };
  assert.deepEqual(context.expected, { generation: "1", tickFrom: 1, seqFrom: 0 });
  assert.deepEqual(context.cursor, { generation: "1", tick: 0, seq: 0 });
  const feedback = buildErrorFeedback(failure);
  assert.match(feedback, /"expected": \{\s*"generation": "1",\s*"seqFrom": 0,\s*"tickFrom": 1/);
  assert.doesNotMatch(feedback, /"tickFrom": 2/);
});

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
  assert.equal(coordinator.accept({ type: "protocol", generation: "generation-1", update: { Wrong: {} }, civilDate: null, revision: null }), false);
  assert.equal(failures.length, 1, "fatal 后禁止消费下一提交或覆盖原错误");
  coordinator.accept(baseline());
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
      refresh: { ticks_per_day: 1, snapshot: snapshot(1, 2), securities: [{ code: "600000", exchange: "Shanghai", initial_price: "1000", category: "MainBoard", limit_pct: 0.1, tick: "1", total_shares: "1000000", float_shares: 1_000_000 }], intraday: [frame(1, 0, ["600000"])], public_publication_ids: [] },
    },
  }, civilDate: "2030-01-03", revision: "1" });
  assert.equal(publications, 2);
});
