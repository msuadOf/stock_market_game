import assert from "node:assert/strict";
import test from "node:test";
import { valueHeldPosition } from "./position-valuation.ts";

test("审计G49：正负半分按偶数成本舍入，零/负净成本仍能真实估值", () => {
  for (const [net, cost] of [[1, 0], [3, 2], [5, 2], [-1, 0], [-3, -2], [-5, -2], [7, 4], [-7, -4], [2001, 1000], [2003, 1002], [-2001, -1000], [-2003, -1002], [0, 0]]) {
    const position = { qty: 2, t1_locked: 0, invested_cents: String(Math.max(0, net)), recovered_cents: String(Math.max(0, -net)) };
    assert.deepEqual(valueHeldPosition(position, "1001"), { avgCost: String(cost), marketValue: "2002", pnl: String((1001 - cost) * 2) });
  }
  for (const [net, cost] of [[1, 0], [3, 1], [-1, 0], [-3, -1]]) {
    const position = { qty: 4, t1_locked: 0, invested_cents: String(Math.max(0, net)), recovered_cents: String(Math.max(0, -net)) };
    assert.deepEqual(valueHeldPosition(position, "1001"), { avgCost: String(cost), marketValue: "4004", pnl: String((1001 - cost) * 4) });
  }
});

test("审计G49：股数保持安全整数，金额精确保留到 i64 边界并显式拒绝越界", () => {
  const position = { qty: 2, t1_locked: 0, invested_cents: "2000", recovered_cents: "0" };
  for (const quantity of [0, -1]) assert.throws(() => valueHeldPosition({ ...position, qty: quantity }, "1000"), /正股数/);
  for (const quantity of [0.5, Number.MAX_SAFE_INTEGER + 1]) assert.throws(() => valueHeldPosition({ ...position, qty: quantity }, "1000"), /安全整数/);
  for (const price of ["0", "-1"]) assert.throws(() => valueHeldPosition(position, price), /正数/);
  assert.throws(() => valueHeldPosition({ ...position, invested_cents: "NaN" }, "1000"), /十进制/);
  assert.deepEqual(valueHeldPosition(position, "9007199254740993"), { avgCost: "1000", marketValue: "18014398509481986", pnl: "18014398509479986" });
  assert.throws(() => valueHeldPosition(position, "9223372036854775807"), /i64/);
  assert.throws(() => valueHeldPosition(position, "9223372036854775808"), /i64/);
});
