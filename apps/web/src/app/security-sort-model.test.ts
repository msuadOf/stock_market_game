import assert from "node:assert/strict";
import test from "node:test";
import { buildMarketRows } from "../components/market-grid-rows.ts";
import type { MarketSnap } from "../types/engine.ts";
import { adjacentSecurityCode } from "./security-browser-model.ts";
import { compareSecurityRows, sortSecurityCodes, mobileSecuritySort, cycleChangeSort, validateSecuritySort } from "./security-sort-model.ts";

function market(last: string, close = "1000", bid: string | null = null): MarketSnap {
  return { last_price: last, last_close: close, best_bid: bid, best_ask: null, bids: [], asks: [] };
}
const codes = ["600101", "002156", "300260"];
const markets = { "600101": market("900"), "002156": market("1200"), "300260": market("1100") };

test("同一涨跌幅顺序用于列表和相邻切股，报价变更后按新值重排", () => {
  const sorted = sortSecurityCodes(codes, markets, [{ field: "changePct", direction: "desc" }]);
  assert.deepEqual(sorted, ["002156", "300260", "600101"]);
  assert.equal(adjacentSecurityCode(sorted, "002156", 1), "300260");
  assert.equal(adjacentSecurityCode(sorted, "002156", -1), "600101");
  assert.deepEqual(sortSecurityCodes(codes, { ...markets, "600101": market("1300") }, [{ field: "changePct", direction: "desc" }]), ["600101", "002156", "300260"]);
  assert.deepEqual(codes, ["600101", "002156", "300260"]);
});

test("多列排序依次比较，完全相同时保留输入顺序，取消排序还原原顺序", () => {
  const tied = { ...markets, "300260": market("1200") };
  assert.deepEqual(sortSecurityCodes(codes, tied, [{ field: "lastPrice", direction: "desc" }]), ["002156", "300260", "600101"]);
  assert.deepEqual(sortSecurityCodes(codes, tied, [{ field: "lastPrice", direction: "desc" }, { field: "code", direction: "desc" }]), ["300260", "002156", "600101"]);
  assert.deepEqual(sortSecurityCodes(codes, markets, []), codes);
  assert.deepEqual(sortSecurityCodes([], markets, []), []);
});

test("金额及涨跌幅排序保留超过 Number 精度的分值差，不按显示舍入排序", () => {
  const high = { "600101": market("9007199254740993", "9007199254740992"), "002156": market("9007199254740994", "9007199254740992") };
  const rows = buildMarketRows(high, ["600101", "002156"]);
  assert.equal(compareSecurityRows(rows[0], rows[1], "lastPrice"), -1);
  assert.equal(compareSecurityRows(rows[0], rows[1], "changePct"), -1);
  assert.deepEqual(sortSecurityCodes(["600101", "002156"], high, [{ field: "changePct", direction: "desc" }]), ["002156", "600101"]);
});

test("空盘口与正报价区分，排序不改变缺失报价", () => {
  const rows = buildMarketRows({ "600101": market("900"), "002156": market("1200", "1000", "1199") }, ["600101", "002156"]);
  assert.equal(compareSecurityRows(rows[0], rows[1], "best_bid"), -1);
  assert.equal(rows[0]._source.best_bid, null);
});

test("手机只应用涨跌幅规则，操作按降序、升序、原顺序循环", () => {
  assert.deepEqual(mobileSecuritySort([{ field: "lastPrice", direction: "asc" }]), []);
  assert.deepEqual(mobileSecuritySort([{ field: "lastPrice", direction: "asc" }, { field: "changePct", direction: "desc" }]), [{ field: "changePct", direction: "desc" }]);
  const descending = cycleChangeSort([]);
  const ascending = cycleChangeSort(descending);
  assert.deepEqual(descending, [{ field: "changePct", direction: "desc" }]);
  assert.deepEqual(ascending, [{ field: "changePct", direction: "asc" }]);
  assert.deepEqual(cycleChangeSort(ascending), []);
});

test("非法排序规则及缺失证券明确报错，不静默退回默认排序", () => {
  for (const value of [null, {}, [{ field: "missing", direction: "asc" }], [{ field: "code", direction: "bad" }], [{ field: "code", direction: "asc" }, { field: "code", direction: "desc" }]]) {
    assert.throws(() => validateSecuritySort(value), /排序/);
  }
  assert.throws(() => sortSecurityCodes(["missing"], markets, [{ field: "lastPrice", direction: "asc" }]), /证券|行情/);
});
