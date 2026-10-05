import assert from "node:assert/strict";
import test from "node:test";
import { formatTradeTime, formatGameClock } from "./market-model.ts";
import { DEFAULT_SETUP } from "../config/defaults.ts";

test("短交易日时钟与逐笔按相同阶段换算，收盘成交保留15点而当前时钟进入下一日", { timeout: 10000 }, () => {
  const setup = { ...DEFAULT_SETUP, ticks_per_day: 30, auction_ticks: 9, closing_auction_ticks: 3 };
  assert.equal(formatGameClock(3, setup), "09:20:00");
  assert.equal(formatGameClock(6, setup), "09:25:00");
  assert.equal(formatGameClock(9, setup), "09:30:00");
  assert.equal(formatGameClock(27, setup), "14:57:00");
  assert.equal(formatTradeTime(30, setup), "15:00:00");
  assert.equal(formatGameClock(30, setup), "09:15:00");
  assert.equal(formatTradeTime(39, setup), "09:30:00");
});

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
