import assert from "node:assert/strict";
import test from "node:test";
import { eventStableKey } from "./protocol/canonical.ts";
import type { Event } from "../types/generated/Event.ts";
import { effectsFromFacts } from "./protocol/effects.ts";
import { reduceEngineUpdate } from "./protocol/reduce.ts";
import { parseEngineUpdate } from "./protocol/parse.ts";
import { codedFactIdentity, translateEngineUpdate } from "./protocol/stock-identity.ts";
import { baseState, snapshot, timeseries, tickBatch } from "./protocol-test-fixtures.ts";

test("NPC普通拒单不冒充玩家反馈，原始事件、玩家拒单和结算错误继续保留", { timeout: 10000 }, () => {
  for (const account of [1, 7, 60_003]) {
    // wire 事件 code 为数值 StockId（ADR 0041）；摄入翻译后回代码串 "000812"。
    const events: readonly Event[] = [
      { IntentRejected: { seq: 1, account, code: 0, reason: "AuctionOrderEntryClosed" } },
      { IntentRejected: { seq: 2, account: 0, code: 0, reason: "InsufficientCash" } },
      { SettlementError: { seq: 3, account, code: 0, reason: "测试结算故障" } },
      { Trade: { seq: 4, code: 0, price: "1000", qty: 100, maker: account, taker: 0 } },
    ];
    const facts = events.map((event, index) => ({
      key: eventStableKey(event, index), event,
    }));
    const coded = translateEngineUpdate({ TickBatch: { frames: [{ tick: 1, facts: [...facts], timeseries_payload: { markets: {}, active_daily_candles: {}, closed_daily_candles: {}, auction_points: {}, continuous_points: {} }, seq_from: 0, seq_to: 4 }], runtime_snapshot: null } });
    if (!("TickBatch" in coded)) throw new Error("测试期望 TickBatch");
    const codedEvents = coded.TickBatch.frames[0]?.events ?? [];
    const codedFacts = coded.TickBatch.frames[0]?.facts ?? [];
    const update = tickBatch([{ tick: 1, facts, timeseries_payload: timeseries(1), seq_from: 0, seq_to: 4 }], snapshot(1, 4));
    const reduction = reduceEngineUpdate(baseState(), "generation-1", update);
    const notices = reduction.effects.filter(effect => effect.kind === "notice").map(effect => effect.message).sort();
    assert.deepEqual(notices, ["委托被拒：000812 - 资金不足", "结算错误：000812 - 测试结算故障"].sort());
    assert.equal(reduction.update.kind, "tick-batch");
    if (reduction.update.kind !== "tick-batch") throw new Error("测试期望 TickBatch");
    assert.deepEqual(reduction.update.frames[0]?.events, codedEvents);
    assert.deepEqual(reduction.state.intraday[0]?.facts, codedFacts.toSorted((left, right) => codedFactIdentity(left).localeCompare(codedFactIdentity(right))));
    assert.deepEqual(reduction.state.cursor, { generation: "generation-1", tick: 1, seq: 4 });
    assert.equal(reduction.effects.filter(effect => effect.kind === "trade").length, 1);
    assert.equal(reduction.effects.filter(effect => effect.kind === "automatic-order").length, 1);
    assert.deepEqual(reduceEngineUpdate(reduction.state, "generation-1", update).effects, []);
  }
});

test("Given stable facts and timeseries, when projected, then notices, trades, and automatic points are type-grouped", () => {
  const trade = { Trade: { seq: 1, code: 0, price: "1000", qty: 100, maker: 0, taker: 1 } };
  const rejected = { IntentRejected: { seq: 2, account: 0, code: 0, reason: "InsufficientCash" } };
  const update = {
    TickBatch: {
      frames: [{
        tick: 1,
        facts: [
          { key: { phase_rank: 4, entity: { Account: 0 }, source: "Sealed", local_event_index: 0 }, event: rejected },
          { key: { phase_rank: 4, entity: { Stock: 0 }, source: "Sealed", local_event_index: 0 }, event: trade },
        ],
        timeseries_payload: timeseries(1),
        seq_from: 0,
        seq_to: 2,
      }],
      runtime_snapshot: snapshot(1, 2),
    },
  };
  // 摄入翻译后事件/时序键均为代码串（ADR 0041）。
  const parsed = translateEngineUpdate(parseEngineUpdate(update));
  if (!("TickBatch" in parsed)) throw new Error("test fixture invalid");
  const current = parsed.TickBatch.frames[0];
  if (current === undefined) throw new Error("test fixture invalid");

  const effects = effectsFromFacts(current.facts, current.timeseries_payload.continuous_points, current.tick);

  assert.deepEqual(effects.map((effect) => effect.kind), ["notice", "trade", "automatic-order"]);
  assert.equal(effects[0]?.kind === "notice" && effects[0].message, "委托被拒：000812 - 资金不足");
  assert.equal(effects[1]?.kind === "trade" && effects[1].event.tick, current.tick);
  assert.equal(effects[2]?.kind === "automatic-order" && effects[2].points[0]?.code, "000812");
});

test("跨帧成交效果保留各自 tick，重试不重复追加", { timeout: 10000 }, () => {
  const frames = [1, 2].map((tick) => {
    const event = { Trade: { seq: tick, code: 0, price: "1000", qty: 100, maker: 0, taker: 1 } };
    return { tick, facts: [{
      key: { phase_rank: 4, entity: { Stock: 0 }, source: "Sealed", local_event_index: 0 },
      event,
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
      code: 0,
      reason: "OrderAlreadyFilled",
    },
  };
  const update = {
    TickBatch: {
      frames: [{
        tick: 1,
        facts: [{
          key: { phase_rank: 4, entity: { Account: 0 }, source: "Sealed", local_event_index: 0 },
          event: rejected,
        }],
        timeseries_payload: timeseries(1),
        seq_from: 0,
        seq_to: 1,
      }],
      runtime_snapshot: snapshot(1, 1),
    },
  };
  const parsed = translateEngineUpdate(parseEngineUpdate(update));
  if (!("TickBatch" in parsed)) throw new Error("test fixture invalid");
  const current = parsed.TickBatch.frames[0];
  if (current === undefined) throw new Error("test fixture invalid");

  const effects = effectsFromFacts(current.facts, current.timeseries_payload.continuous_points, current.tick);

  assert.equal(effects[0]?.kind, "notice");
  assert.equal(
    effects[0]?.kind === "notice" && effects[0].message,
    "委托被拒：000812 - 委托已全部成交，无法撤单",
  );
});
