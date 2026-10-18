import assert from "node:assert/strict";
import { test } from "node:test";
import { parseHistoricalStockData } from "./stock-history.ts";

// wire 的 code 为数值 StockId；id 0 ↔ 代码串 "000812"（ADR 0041）。
const valid = {
  code: 0,
  daily_candles: [{ time: 1893456000, open: "1000", high: "1200", low: "900", close: "1100", volume: 100, trade_stats: { turnover_cents: "100000", trade_count: 2 } }],
  active_daily_candle: { time: 1893542400, open: "1100", high: "1200", low: "1000", close: "1150", volume: 200 },
};

test("历史查询响应严格校验请求代码、日 K 字段与时间顺序", () => {
  assert.equal(parseHistoricalStockData(valid, "000812").daily_candles.length, 1);
  const largeMoneyCandle = { ...valid.daily_candles[0], open: "900719925474099300", high: "900719925474099300", low: "900719925474099300", close: "900719925474099300" };
  assert.equal(parseHistoricalStockData({ ...valid, daily_candles: [largeMoneyCandle] }, "000812").daily_candles[0]?.close, "900719925474099300");
  assert.throws(() => parseHistoricalStockData({ ...valid, code: 1 }, "000812"), /与请求代码/);
  assert.throws(() => parseHistoricalStockData({ ...valid, extra: true }, "000812"), /字段不符合协议契约/);
  assert.throws(() => parseHistoricalStockData({ code: 0, active_daily_candle: null }, "000812"), /字段不符合协议契约/);
  assert.throws(() => parseHistoricalStockData({ ...valid, daily_candles: [{ ...valid.daily_candles[0], open: 1000 }] }, "000812"), /规范十进制整数分字符串/);
  assert.throws(() => parseHistoricalStockData({ ...valid, daily_candles: [{ ...valid.daily_candles[0], close: 1100 }] }, "000812"), /规范十进制整数分字符串/);
  assert.throws(() => parseHistoricalStockData({ ...valid, daily_candles: [valid.daily_candles[0], valid.daily_candles[0]] }, "000812"), /严格递增/);
  assert.throws(() => parseHistoricalStockData({ ...valid, active_daily_candle: { ...valid.active_daily_candle, time: 1893456000 } }, "000812"), /必须晚于/);
});
