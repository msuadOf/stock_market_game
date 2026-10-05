import assert from "node:assert/strict";
import test from "node:test";
import { canonicalJson, eventStableKey, factIdentity } from "./protocol/canonical.ts";
import type { Event } from "../types/generated/Event.ts";
import { effectsFromFacts } from "./protocol/effects.ts";
import { reduceEngineUpdate } from "./protocol/reduce.ts";
import { parseEngineUpdate } from "./protocol/parse.ts";
import { baseState, snapshot, timeseries, tickBatch } from "./protocol-test-fixtures.ts";
import type { ProtocolState } from "./protocol/types.ts";

const accountSnapshot = { cash: "100000", positions: {}, reserved_cash: "0", reserved_sell_qty: {} };

function ownState(account: string, generation = "generation-1"): ProtocolState {
  const state = baseState();
  return { ...state, cursor: { ...state.cursor, generation }, snapshot: { ...state.snapshot, accounts: { [account]: accountSnapshot } } };
}

function eventFacts(events: readonly Event[]) {
  return events.map((event, index) => ({ key: eventStableKey(event, index), event, canonical_payload: canonicalJson(event) }));
}

test("NPC普通拒单不冒充玩家反馈，原始事件、玩家拒单和结算错误继续保留", { timeout: 10000 }, () => {
  for (const account of ["1", "7", "60003"]) {
    const events: readonly Event[] = [
      { IntentRejected: { seq: 1, account, code: "600000", reason: "AuctionOrderEntryClosed" } },
      { IntentRejected: { seq: 2, account: "0", code: "600000", reason: "InsufficientCash" } },
      { SettlementError: { seq: 3, account, code: "600000", reason: "测试结算故障" } },
      { Trade: { seq: 4, code: "600000", price: "1000", qty: 100, maker: account, taker: "0" } },
    ];
    const facts = events.map((event, index) => ({
      key: eventStableKey(event, index), event, canonical_payload: canonicalJson(event),
    }));
    const update = tickBatch([{ tick: 1, events, facts, timeseries_payload: timeseries(1), seq_from: 0, seq_to: 4 }], snapshot(1, 4));
    const reduction = reduceEngineUpdate(ownState("0"), "generation-1", update);
    const notices = reduction.effects.filter(effect => effect.kind === "notice").map(effect => effect.message).sort();
    assert.deepEqual(notices, ["委托被拒：600000 - 资金不足", "结算错误：600000 - 测试结算故障"].sort());
    assert.equal(reduction.update.kind, "tick-batch");
    if (reduction.update.kind !== "tick-batch") throw new Error("测试期望 TickBatch");
    assert.deepEqual(reduction.update.frames[0]?.events, events);
    assert.deepEqual(reduction.state.intraday[0]?.facts, facts.toSorted((left, right) => factIdentity(left).localeCompare(factIdentity(right))));
    assert.deepEqual(reduction.state.cursor, { generation: "generation-1", tick: 1, seq: 4 });
    assert.equal(reduction.effects.filter(effect => effect.kind === "trade").length, 1);
    assert.equal(reduction.effects.filter(effect => effect.kind === "automatic-order").length, 1);
    assert.deepEqual(reduceEngineUpdate(reduction.state, "generation-1", update).effects, []);
  }
});

test("Given stable facts and timeseries, when projected, then notices, trades, and automatic points are type-grouped", () => {
  const trade = { Trade: { seq: 1, code: "600000", price: "1000", qty: 100, maker: "0", taker: "1" } };
  const rejected = { IntentRejected: { seq: 2, account: "0", code: "600000", reason: "InsufficientCash" } };
  const update = {
    TickBatch: {
      frames: [{
        tick: 1,
        events: [rejected, trade],
        facts: [
          { key: { phase_rank: 4, entity: { Account: "0" }, source: "Sealed", local_event_index: 0 }, event: rejected, canonical_payload: canonicalJson(rejected) },
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

  const effects = effectsFromFacts(current.facts, current.timeseries_payload.continuous_points, current.tick, "0");

  assert.deepEqual(effects.map((effect) => effect.kind), ["notice", "trade", "automatic-order"]);
  assert.equal(effects[0]?.kind === "notice" && effects[0].message, "委托被拒：600000 - 资金不足");
  assert.equal(effects[1]?.kind === "trade" && effects[1].event.tick, current.tick);
  assert.equal(effects[2]?.kind === "automatic-order" && effects[2].points[0]?.code, "600000");
});

test("跨帧成交效果保留各自 tick，重试不重复追加", { timeout: 10000 }, () => {
  const frames = [1, 2].map((tick) => {
    const event = { Trade: { seq: tick, code: "600000", price: "1000", qty: 100, maker: "0", taker: "1" } };
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
      account: "0",
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
          key: { phase_rank: 4, entity: { Account: "0" }, source: "Sealed", local_event_index: 0 },
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

  const effects = effectsFromFacts(current.facts, current.timeseries_payload.continuous_points, current.tick, "0");

  assert.equal(effects[0]?.kind, "notice");
  assert.equal(
    effects[0]?.kind === "notice" && effects[0].message,
    "委托被拒：600000 - 委托已全部成交，无法撤单",
  );
});

test("非0及MAX本人拒单只按同代scoped基线账户反馈，其他0账户不冒充本人", { timeout: 10000 }, () => {
  for (const account of ["7", "18446744073709551615"]) {
    const events: readonly Event[] = [
      { IntentRejected: { seq: 1, account: "0", code: "600000", reason: "InsufficientShares" } },
      { IntentRejected: { seq: 2, account, code: "600000", reason: "InsufficientCash" } },
      { PrivateEventOmitted: { seq: 3 } },
    ];
    const update = tickBatch([{ tick: 1, events, facts: eventFacts(events), timeseries_payload: timeseries(1), seq_from: 0, seq_to: 3 }], null);
    const state = ownState(account);
    const reduction = reduceEngineUpdate(state, "generation-1", update);
    assert.deepEqual(reduction.effects.filter(effect => effect.kind === "notice").map(effect => effect.message), ["委托被拒：600000 - 资金不足"]);
    assert.deepEqual(reduction.update.kind === "tick-batch" && reduction.update.frames[0]?.events, events);
    assert.deepEqual(reduceEngineUpdate(reduction.state, "generation-1", update).effects, []);
    assert.throws(() => reduceEngineUpdate(ownState("0", "generation-2"), "generation-1", update), /代际/);
    const replaced = reduceEngineUpdate(ownState("0", "generation-2"), "generation-2", update);
    assert.deepEqual(replaced.effects.filter(effect => effect.kind === "notice").map(effect => effect.message), ["委托被拒：600000 - 持仓不足"]);
  }
});

test("缺席或多账户未指定本人时不猜0，系统结算错误保持显式通知", { timeout: 10000 }, () => {
  const events: readonly Event[] = [
    { IntentRejected: { seq: 1, account: "0", code: "600000", reason: "InsufficientCash" } },
    { SettlementError: { seq: 2, account: "7", code: "600000", reason: "测试结算故障" } },
  ];
  const update = tickBatch([{ tick: 1, events, facts: eventFacts(events), timeseries_payload: timeseries(1), seq_from: 0, seq_to: 2 }], null);
  const multiple = ownState("0");
  for (const state of [baseState(), { ...multiple, snapshot: { ...multiple.snapshot, accounts: { "0": accountSnapshot, "7": accountSnapshot } } }]) {
    assert.deepEqual(reduceEngineUpdate(state, "generation-1", update).effects.filter(effect => effect.kind === "notice").map(effect => effect.message), ["结算错误：600000 - 测试结算故障"]);
  }
  assert.deepEqual(effectsFromFacts(eventFacts(events), {}, 1).filter(effect => effect.kind === "notice").map(effect => effect.message), ["结算错误：600000 - 测试结算故障"]);
});

test("直接effects本人身份严格校验规范u64，不接受宽字符串或数字身份", { timeout: 10000 }, () => {
  const events: readonly Event[] = [{ IntentRejected: { seq: 1, account: "0", code: "600000", reason: "InsufficientCash" } }];
  for (const identity of ["", "00", "-1", "+1", " 0", "18446744073709551616", 0]) {
    assert.throws(() => effectsFromFacts(eventFacts(events), {}, 1, identity as string), /u64|字符串|规范/);
  }
});
