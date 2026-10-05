import assert from "node:assert/strict";
import { test } from "node:test";
import { filterSecurityCodes, adjacentSecurityCode, toggleWatchlistCode, securityListKeyboardTarget } from "./security-browser-model.ts";

const codes = ["600101", "002156", "X0101"];
const names = { "600101": "稳健实业", "002156": "芯片科技", X0101: "新股票" };

test("全部、自选、持仓与名称/代码查询取交集，保留输入顺序", () => {
  const base = { codes, names, favorites: ["002156", "missing"], heldCodes: new Set(["600101"]), query: "" };
  assert.deepEqual(filterSecurityCodes({ ...base, view: "all" }), codes);
  assert.deepEqual(filterSecurityCodes({ ...base, view: "watchlist" }), ["002156"]);
  assert.deepEqual(filterSecurityCodes({ ...base, view: "holdings" }), ["600101"]);
  assert.deepEqual(filterSecurityCodes({ ...base, view: "all", query: " 芯片 " }), ["002156"]);
  assert.deepEqual(filterSecurityCodes({ ...base, view: "all", query: "x01" }), ["X0101"]);
  assert.deepEqual(filterSecurityCodes({ ...base, view: "watchlist", query: "600" }), []);
  assert.deepEqual(filterSecurityCodes({ ...base, view: "watchlist", favorites: [] }), []);
});

test("列表方向键只选择可见股票，Home/End 定位首尾，其他按键不接管", () => {
  assert.equal(securityListKeyboardTarget(codes, "600101", "ArrowDown"), "002156");
  assert.equal(securityListKeyboardTarget(codes, "600101", "ArrowUp"), "X0101");
  assert.equal(securityListKeyboardTarget(codes, "002156", "Home"), "600101");
  assert.equal(securityListKeyboardTarget(codes, "002156", "End"), "X0101");
  assert.equal(securityListKeyboardTarget(codes, "002156", "Tab"), null);
  assert.equal(securityListKeyboardTarget([], "002156", "Home"), null);
});

test("自选独立切换，不修改输入；切股只在可见名单内循环，移出后不虚构相邻股票", () => {
  const favorites = ["600101"];
  assert.deepEqual(toggleWatchlistCode(favorites, "002156"), ["600101", "002156"]);
  assert.deepEqual(toggleWatchlistCode(favorites, "600101"), []);
  assert.deepEqual(favorites, ["600101"]);
  assert.equal(adjacentSecurityCode(codes, "600101", -1), "X0101");
  assert.equal(adjacentSecurityCode(codes, "X0101", 1), "600101");
  assert.equal(adjacentSecurityCode([], "600101", 1), null);
  assert.equal(adjacentSecurityCode(["002156"], "600101", 1), null);
  assert.equal(adjacentSecurityCode(["600101"], "600101", 1), null);
});
