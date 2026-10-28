import assert from "node:assert/strict";
import test from "node:test";
import { parseEngineUpdate } from "./protocol/parse.ts";
import { reduceEngineUpdate } from "./protocol/reduce.ts";
import { ProtocolError } from "./protocol/types.ts";
import { baseState, snapshot, tickBatch, timeseries } from "./protocol-test-fixtures.ts";

function rejectedUpdate(account: string, reason: string) {
  return tickBatch([{
    tick: 1,
    facts: [{
      key: { phase_rank: 4, entity: { Account: account }, source: "Sealed", local_event_index: 0 },
      event: { IntentRejected: { seq: 1, account, code: 0, reason } },
    }],
    timeseries_payload: { ...timeseries(1), continuous_points: {} },
    seq_from: 0,
    seq_to: 1,
  }], snapshot(1, 1));
}

test("ExchangeClosed 通过严格解析并向玩家显示一次证券所属交易所休市反馈", { timeout: 10000 }, () => {
  const update = rejectedUpdate("0", "ExchangeClosed");
  const reduction = reduceEngineUpdate(baseState(), "generation-1", update);

  assert.deepEqual(reduction.effects, [{
    kind: "notice",
    message: "委托被拒：000812 - 该证券所属交易所今日休市",
  }]);
  assert.equal(reduction.update.kind, "tick-batch");
  if (reduction.update.kind !== "tick-batch") throw new Error("fixture 必须归一化为 TickBatch");
  assert.deepEqual(reduction.update.frames[0]?.events, [{
    IntentRejected: { seq: 1, account: "0", code: "000812", reason: "ExchangeClosed" },
  }]);
  assert.deepEqual(reduction.state.cursor, { generation: "generation-1", tick: 1, seq: 1 });
  assert.deepEqual(reduceEngineUpdate(reduction.state, "generation-1", update).effects, []);
});

test("NPC 的 ExchangeClosed 仍保留事实且不冒充玩家提示", { timeout: 10000 }, () => {
  const reduction = reduceEngineUpdate(baseState(), "generation-1", rejectedUpdate("7", "ExchangeClosed"));

  assert.deepEqual(reduction.effects, []);
  assert.equal(reduction.update.kind, "tick-batch");
  if (reduction.update.kind !== "tick-batch") throw new Error("fixture 必须归一化为 TickBatch");
  assert.deepEqual(reduction.update.frames[0]?.events, [{
    IntentRejected: { seq: 1, account: "7", code: "000812", reason: "ExchangeClosed" },
  }]);
});

test("休市拒单原因必须匹配 Rust wire variant，未知拼写不得静默替代", { timeout: 10000 }, () => {
  for (const reason of ["exchange_closed", "Closed", "ExchangeClosed "]) {
    assert.throws(() => parseEngineUpdate(rejectedUpdate("0", reason)), ProtocolError);
  }
});
