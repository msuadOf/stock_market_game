import assert from "node:assert/strict";
import { test } from "node:test";
import { MARKET_GRID_LOCALE, selectMarketByKeyboard } from "./market-grid-accessibility.ts";

test("G64：焦点单元格 Enter/Space 选择对应股票，不拦截方向导航与空行", { timeout: 10000 }, () => {
  const selected: string[] = [];
  const onSelect = (code: string) => selected.push(code);
  assert.equal(selectMarketByKeyboard("Enter", "300999", onSelect), true);
  assert.equal(selectMarketByKeyboard(" ", "600101", onSelect), true);
  assert.equal(selectMarketByKeyboard("ArrowDown", "600101", onSelect), false);
  assert.equal(selectMarketByKeyboard("Enter", undefined, onSelect), false);
  assert.deepEqual(selected, ["300999", "600101"]);
});

test("G48：sortable 表头内置辅助文本配置中文", { timeout: 10000 }, () => {
  assert.equal(MARKET_GRID_LOCALE.ariaSortableColumn, "按 Enter 排序");
  assert.equal(MARKET_GRID_LOCALE.noRowsToShow, "暂无行情");
});
