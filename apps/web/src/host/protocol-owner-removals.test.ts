import assert from "node:assert/strict";
import test from "node:test";
import { reduceEngineUpdate } from "./protocol/reduce.ts";
import { createProtocolState } from "./protocol/types.ts";
import { parseProtocolSnapshot } from "./protocol/parse.ts";
import { frame, snapshot, tickBatch, type JsonValue } from "./protocol-test-fixtures.ts";

test("本人MAX owner删除只消费对应订单，旧number数组与其他owner明确拒绝", { timeout: 10000 }, () => {
  const owner = "18446744073709551615";
  const account = { cash: "100000", reserved_cash: "0", positions: {}, reserved_sell_qty: {} };
  const initial = createProtocolState(parseProtocolSnapshot({ ...snapshot(0, 0), accounts: { [owner]: account } }), "1");
  const order = { owner, id: 1, code: "600000", side: "Buy", price: "1000", remainingQty: 100, venue: "continuous", frozen: "cash" };
  const delta = { seq_from: 0, seq_to: 0, tick: 1, day: 0, phase: "Continuous", accounts: {}, working_orders: { reset: true, upserts: [order, { ...order, id: 2 }], removed: [] } };
  const installed = reduceEngineUpdate(initial, "1", tickBatch([frame(1, 0, [])], null, delta)).state;
  const remove = (removed: JsonValue[]) => tickBatch([frame(2, 0, [])], null, { ...delta, tick: 2, working_orders: { reset: false, upserts: [], removed } });
  const result = reduceEngineUpdate(installed, "1", remove([{ id: 1, owner }]));
  assert.equal(result.state.playerWorkingOrders[1], undefined);
  assert.equal(result.state.playerWorkingOrders[2]!.owner, owner);
  for (const removed of [[1], [{ id: 1, owner: "0" }], [{ id: 1, owner: 0 }], [{ id: 1, owner: "00" }], [{ id: 1, owner, secret: "x" }]]) {
    assert.throws(() => reduceEngineUpdate(installed, "1", remove(removed)), /对象|owner|账户|规范|字段/);
    assert.equal(installed.playerWorkingOrders[1]!.owner, owner);
  }
});
