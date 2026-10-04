import assert from "node:assert/strict";
import test from "node:test";
import { canonicalJson } from "./protocol/canonical.ts";
import { effectsFromFacts } from "./protocol/effects.ts";
import { reduceEngineUpdate } from "./protocol/reduce.ts";
import { parseEngineUpdate } from "./protocol/parse.ts";
import { baseState, snapshot, timeseries, tickBatch } from "./protocol-test-fixtures.ts";

test("Given stable facts and timeseries, when projected, then notices, trades, and automatic points are type-grouped", () => {
  const trade = { Trade: { seq: 1, code: "600000", price: "1000", qty: 100, maker: 0, taker: 1 } };
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

  const effects = effectsFromFacts(current.facts, current.timeseries_payload.continuous_points, current.tick);

  assert.deepEqual(effects.map((effect) => effect.kind), ["notice", "trade", "automatic-order"]);
  assert.equal(effects[0]?.kind === "notice" && effects[0].message, "委托被拒：600000 - 资金不足");
  assert.equal(effects[1]?.kind === "trade" && effects[1].event.tick, current.tick);
  assert.equal(effects[2]?.kind === "automatic-order" && effects[2].points[0]?.code, "600000");
});

test("跨帧成交效果保留各自 tick，重试不重复追加", { timeout: 10000 }, () => {
  const frames = [1, 2].map((tick) => {
    const event = { Trade: { seq: tick, code: "600000", price: "1000", qty: 100, maker: 0, taker: 1 } };
    return { tick, events: [event], facts: [{
      key: { phase_rank: 4, entity: { Stock: "600000" }, source: "Sealed", local_event_index: 0 },
      event, canonical_payload: canonicalJson(event),
    }], timeseries_payload: timeseries(tick), seq_from: tick - 1, seq_to: tick };
  });
  const update = tickBatch(frames, snapshot(2, 2));
  const reduction = reduceEngineUpdate(baseState(), "generation-1", update);
  const trades = reduction.effects.filter((effect) => effect.kind === "trade");
  assert.deepEqual(trades.map((effect) => [effect.event.seq, effect.event.tick]), [[1, 1], [2, 2]]);
  assert.deepEqual(reduceEngineUpdate(reduction.state, "generation-1", update).effects, []);
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

  const effects = effectsFromFacts(current.facts, current.timeseries_payload.continuous_points, current.tick);

  assert.equal(effects[0]?.kind, "notice");
  assert.equal(
    effects[0]?.kind === "notice" && effects[0].message,
    "委托被拒：600000 - 委托已全部成交，无法撤单",
  );
});
