import assert from "node:assert/strict";
import test from "node:test";
import { formatTradeTime } from "./market-model.ts";

test("逐笔按自身权威 tick 显示秒级成交时间，不借用当前时钟", { timeout: 10000 }, () => {
  assert.equal(formatTradeTime(600), "09:25:00");
  assert.equal(formatTradeTime(901), "09:30:01");
  assert.equal(formatTradeTime(8100), "11:30:00");
  assert.equal(formatTradeTime(8101), "13:00:01");
  assert.equal(formatTradeTime(15300), "15:00:00");
  assert.equal(formatTradeTime(16201), "09:30:01");
  assert.equal(formatTradeTime(undefined), "成交时间缺失");
  assert.throws(() => formatTradeTime(-1), RangeError);
});
