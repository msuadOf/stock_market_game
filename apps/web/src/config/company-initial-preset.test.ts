import assert from "node:assert/strict";
import test from "node:test";
import { createCompanyInitialPreset } from "./company-initial-preset.ts";

const stock = { code: "600101", initial_price: "1000", total_shares: "1000000" };

test("虚拟 PE 与 PB 反推初值按同期间费用和税务勾稽，误差仅来自分舍入", { timeout: 10000 }, () => {
  const marketCapitalization = BigInt(stock.initial_price) * BigInt(stock.total_shares);
  const absolute = (value: bigint) => value < 0n ? -value : value;
  const rounded = (numerator: bigint, denominator: bigint) => {
    const quotient = numerator / denominator;
    const remainder = numerator % denominator;
    return quotient + (remainder * 2n > denominator || (remainder * 2n === denominator && quotient % 2n === 1n) ? 1n : 0n);
  };
  for (const seed of [0n, 42n, (1n << 64n) - 1n]) {
    for (const months of [1, 3, 6, 12]) {
      const initial = createCompanyInitialPreset(stock, seed, months);
      const revenue = BigInt(initial.initial_revenue.replace(".", ""));
      const fixedExpense = BigInt(initial.initial_fixed_expense.replace(".", ""));
      const variableExpense = rounded(revenue * BigInt(initial.variable_expense_ratio_bp), 10000n);
      const pretaxProfit = revenue - fixedExpense - variableExpense;
      const netProfit = pretaxProfit - rounded(pretaxProfit * 2500n, 10000n);
      const annualizedProfitError = absolute(netProfit * 12n * BigInt(initial.virtual_pe) - marketCapitalization * BigInt(months));
      assert.ok(annualizedProfitError <= 2n * 12n * BigInt(initial.virtual_pe));
      const equity = BigInt(initial.initial_equity.replace(".", ""));
      assert.ok(absolute(equity * BigInt(initial.virtual_pb_bp) - marketCapitalization * 10000n) * 2n <= BigInt(initial.virtual_pb_bp));
    }
  }
});

test("开局价格与股本形成虚拟基本面基准，相同 seed 可复现且不设置净利润", { timeout: 10000 }, () => {
  const roundingFreeStock = { ...stock, total_shares: "2329089562800" };
  const initial = createCompanyInitialPreset(roundingFreeStock, 42n, 1);
  assert.deepEqual(createCompanyInitialPreset(roundingFreeStock, 42n, 1), initial);
  const repriced = createCompanyInitialPreset({ ...roundingFreeStock, initial_price: "2000" }, 42n, 1);
  assert.equal(BigInt(repriced.initial_revenue.replace(".", "")), BigInt(initial.initial_revenue.replace(".", "")) * 2n);
  assert.ok(BigInt(repriced.initial_equity.replace(".", "")) > BigInt(initial.initial_equity.replace(".", "")));
  assert.equal(Object.hasOwn(initial, "net_income"), false);
  assert.equal(Object.hasOwn(initial, "correct_price"), false);
});

test("期初收入基准与结算长度匹配，后续无需读取价格", { timeout: 10000 }, () => {
  const annual = createCompanyInitialPreset(stock, 42n, 12), quarterly = createCompanyInitialPreset(stock, 42n, 3);
  assert.ok(BigInt(annual.initial_revenue.replace(".", "")) >= BigInt(quarterly.initial_revenue.replace(".", "")) * 4n - 2n);
  assert.equal(annual.virtual_pe, quarterly.virtual_pe);
  assert.equal(annual.initial_equity, quarterly.initial_equity);
  assert.ok(annual.virtual_pe >= 10 && annual.virtual_pe <= 30);
  assert.ok(annual.virtual_pb_bp >= 10000 && annual.virtual_pb_bp <= 30000);
  assert.notDeepEqual(createCompanyInitialPreset(stock, 43n, 1), createCompanyInitialPreset(stock, 42n, 1));
});

test("非法价格、股本、seed及周期显式拒绝，不从错误生成默认锚", { timeout: 10000 }, () => {
  for (const invalid of ["0", "-1", "01", "1000.00"]) assert.throws(() => createCompanyInitialPreset({ ...stock, initial_price: invalid }, 42n, 1));
  assert.throws(() => createCompanyInitialPreset({ ...stock, total_shares: "0" }, 42n, 1));
  assert.throws(() => createCompanyInitialPreset(stock, -1n, 1));
  assert.throws(() => createCompanyInitialPreset(stock, 42n, 2));
  assert.throws(() => createCompanyInitialPreset({ ...stock, initial_price: "1", total_shares: "1" }, 42n, 1), /舍入为零/);
});
