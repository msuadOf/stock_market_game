import assert from "node:assert/strict";
import test from "node:test";
import { canonicalJson } from "./protocol/canonical.ts";
import { effectsFromFacts } from "./protocol/effects.ts";
import { parseEngineUpdate } from "./protocol/parse.ts";
import { snapshot, timeseries } from "./protocol-test-fixtures.ts";

test("Given stable facts and timeseries, when projected, then notices, trades, and automatic points are type-grouped", () => {
  const trade = { Trade: { seq: 1, code: "600000", price: 1_000, qty: 100, maker: 0, taker: 1 } };
  const rejected = { IntentRejected: { seq: 2, account: 0, code: "600000", reason: "InsufficientCash" } };
  const update = {
    TickBatch: {
      frames: [{
        tick: 1,
        events: [rejected, trade],
        facts: [
          { key: { phase_rank: 4, entity: { Account: 0 }, source: "Sealed", local_event_index: 0 }, event: rejected, canonical_payload: canonicalJson(rejected) },
          { key: { phase_rank: 4, entity: { Stock: "600000" }, source: "Sealed", local_event_index: 0 }, event: trade, canonical_payload: canonicalJson(trade) },
        ],
        timeseries_payload: timeseries(1),
        seq_from: 0,
        seq_to: 2,
      }],
      runtime_snapshot: snapshot(1, 2),
    },
  };
  const parsed = parseEngineUpdate(update);
  if (!("TickBatch" in parsed)) throw new Error("test fixture invalid");
  const current = parsed.TickBatch.frames[0];
  if (current === undefined) throw new Error("test fixture invalid");

  const effects = effectsFromFacts(current.facts, current.timeseries_payload.continuous_points);

  assert.deepEqual(effects.map((effect) => effect.kind), ["notice", "trade", "automatic-order"]);
  assert.equal(effects[0]?.kind === "notice" && effects[0].message, "委托被拒：600000 - 资金不足");
  assert.equal(effects[2]?.kind === "automatic-order" && effects[2].points[0]?.code, "600000");
});

test("Given an already filled cancellation, when projected, then the notice explains the failure", () => {
  const rejected = {
    IntentRejected: {
      seq: 1,
      account: 0,
      code: "600000",
      reason: "OrderAlreadyFilled",
    },
  };
  const update = {
    TickBatch: {
      frames: [{
        tick: 1,
        events: [rejected],
        facts: [{
          key: { phase_rank: 4, entity: { Account: 0 }, source: "Sealed", local_event_index: 0 },
          event: rejected,
          canonical_payload: canonicalJson(rejected),
        }],
        timeseries_payload: timeseries(1),
        seq_from: 0,
        seq_to: 1,
      }],
      runtime_snapshot: snapshot(1, 1),
    },
  };
  const parsed = parseEngineUpdate(update);
  if (!("TickBatch" in parsed)) throw new Error("test fixture invalid");
  const current = parsed.TickBatch.frames[0];
  if (current === undefined) throw new Error("test fixture invalid");

  const effects = effectsFromFacts(current.facts, current.timeseries_payload.continuous_points);

  assert.equal(effects[0]?.kind, "notice");
  assert.equal(
    effects[0]?.kind === "notice" && effects[0].message,
    "委托被拒：600000 - 委托已全部成交，无法撤单",
  );
});
