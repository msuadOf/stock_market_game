import assert from "node:assert/strict";
import test from "node:test";
import { createCompanyInitialPreset } from "./company-initial-preset.ts";

const stock = { code: "600101", initial_price: "1000", total_shares: "1000000" };

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
