import assert from "node:assert/strict";
import test from "node:test";
import { parseInitialAllocation } from "./initial-allocation.ts";

// wire 的 code 为数值 StockId；id 3 ↔ 代码串 "600101"（ADR 0041）。
export const wireInitialAllocationFixture = {
  stocks: [{ code: 3, float_shares: 7, unallocated_shares: 0, categories: [
    { kind: "Retail", shares: 3, account_count: 5, zero_holders: 2 },
    { kind: "Inst", shares: 4, account_count: 1, zero_holders: 0 },
    { kind: "Hot", shares: 0, account_count: 0, zero_holders: 0 },
  ] }],
};

export const initialAllocationFixture = {
  stocks: [{ ...wireInitialAllocationFixture.stocks[0]!, code: "600101" }],
};

test("实际初始分配按整数股数对账并保留零持股人数", () => {
  assert.deepEqual(parseInitialAllocation(wireInitialAllocationFixture, ["600101"]), initialAllocationFixture);
});
test("实际初始分配拒绝错股、重复类别、负数、未知字段和不守恒", () => {
  for (const mutate of [
    (value: typeof wireInitialAllocationFixture) => { value.stocks[0]!.code = 4; },
    (value: typeof wireInitialAllocationFixture) => { value.stocks[0]!.categories[1]!.kind = "Retail"; },
    (value: typeof wireInitialAllocationFixture) => { value.stocks[0]!.categories[0]!.shares = -1; },
    (value: typeof wireInitialAllocationFixture) => { value.stocks[0]!.categories[0]!.zero_holders = 6; },
    (value: typeof wireInitialAllocationFixture) => { value.stocks[0]!.float_shares = 8; },
    (value: typeof wireInitialAllocationFixture) => { Object.assign(value, { estimate: true }); },
  ]) {
    const value = structuredClone(wireInitialAllocationFixture); mutate(value);
    assert.throws(() => parseInitialAllocation(value, ["600101"]));
  }
});
