import assert from "node:assert/strict";
import test from "node:test";
import { valueHeldPosition } from "./position-valuation.ts";

test("审计G49：正负半分按偶数成本舍入，零/负净成本仍能真实估值", () => {
  for (const [net, cost] of [[1, 0], [3, 2], [5, 2], [-1, 0], [-3, -2], [-5, -2], [7, 4], [-7, -4], [2001, 1000], [2003, 1002], [-2001, -1000], [-2003, -1002], [0, 0]]) {
    const position = { qty: 2, t1_locked: 0, invested_cents: Math.max(0, net), recovered_cents: Math.max(0, -net) };
    assert.deepEqual(valueHeldPosition(position, 1001), { avgCost: cost, marketValue: 2002, pnl: (1001 - cost) * 2 });
  }
  for (const [net, cost] of [[1, 0], [3, 1], [-1, 0], [-3, -1]]) {
    const position = { qty: 4, t1_locked: 0, invested_cents: Math.max(0, net), recovered_cents: Math.max(0, -net) };
    assert.deepEqual(valueHeldPosition(position, 1001), { avgCost: cost, marketValue: 4004, pnl: (1001 - cost) * 4 });
  }
});

test("审计G49：输入或结果不能无声越过当前Web安全整数边界", () => {
  const position = { qty: 2, t1_locked: 0, invested_cents: 2000, recovered_cents: 0 };
  for (const quantity of [0, -1]) assert.throws(() => valueHeldPosition({ ...position, qty: quantity }, 1000), /正股数/);
  for (const quantity of [0.5, Number.MAX_SAFE_INTEGER + 1]) assert.throws(() => valueHeldPosition({ ...position, qty: quantity }, 1000), /安全整数/);
  for (const price of [0, -1]) assert.throws(() => valueHeldPosition(position, price), /正数/);
  assert.throws(() => valueHeldPosition({ ...position, invested_cents: Number.NaN }, 1000), /安全整数/);
  assert.throws(() => valueHeldPosition(position, Number.MAX_SAFE_INTEGER), /安全范围/);
  assert.throws(() => valueHeldPosition(position, Number.MAX_SAFE_INTEGER + 1), /安全整数/);
});
