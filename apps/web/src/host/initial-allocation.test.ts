import assert from "node:assert/strict";
import test from "node:test";
import { parseInitialAllocation } from "./initial-allocation.ts";

export const initialAllocationFixture = {
  stocks: [{ code: "600101", float_shares: 7, unallocated_shares: 0, categories: [
    { kind: "Retail", shares: 3, account_count: 5, zero_holders: 2 },
    { kind: "Inst", shares: 4, account_count: 1, zero_holders: 0 },
    { kind: "Hot", shares: 0, account_count: 0, zero_holders: 0 },
  ] }],
};

test("实际初始分配按整数股数对账并保留零持股人数", () => {
  assert.deepEqual(parseInitialAllocation(initialAllocationFixture, ["600101"]), initialAllocationFixture);
});
test("实际初始分配拒绝错股、重复类别、负数、未知字段和不守恒", () => {
  for (const mutate of [
    (value: typeof initialAllocationFixture) => { value.stocks[0]!.code = "600102"; },
    (value: typeof initialAllocationFixture) => { value.stocks[0]!.categories[1]!.kind = "Retail"; },
    (value: typeof initialAllocationFixture) => { value.stocks[0]!.categories[0]!.shares = -1; },
    (value: typeof initialAllocationFixture) => { value.stocks[0]!.categories[0]!.zero_holders = 6; },
    (value: typeof initialAllocationFixture) => { value.stocks[0]!.float_shares = 8; },
    (value: typeof initialAllocationFixture) => { Object.assign(value, { estimate: true }); },
  ]) {
    const value = structuredClone(initialAllocationFixture); mutate(value);
    assert.throws(() => parseInitialAllocation(value, ["600101"]));
  }
});
