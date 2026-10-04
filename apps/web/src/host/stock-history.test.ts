import assert from "node:assert/strict";
import { test } from "node:test";
import { parseHistoricalStockData } from "./stock-history.ts";

const valid = {
  code: "600000",
  daily_candles: [{ time: 1, open: "1000", high: "1200", low: "900", close: "1100", volume: 100, trade_stats: { turnover_cents: "100000", trade_count: 2 } }],
  active_daily_candle: { time: 2, open: "1100", high: "1200", low: "1000", close: "1150", volume: 200 },
};

test("历史查询响应严格校验请求代码、日 K 字段与时间顺序", () => {
  assert.equal(parseHistoricalStockData(valid, "600000").daily_candles.length, 1);
  const largeMoneyCandle = { ...valid.daily_candles[0], open: "900719925474099300", high: "900719925474099300", low: "900719925474099300", close: "900719925474099300" };
  assert.equal(parseHistoricalStockData({ ...valid, daily_candles: [largeMoneyCandle] }, "600000").daily_candles[0]?.close, "900719925474099300");
  assert.throws(() => parseHistoricalStockData({ ...valid, code: "000001" }, "600000"), /与请求代码/);
  assert.throws(() => parseHistoricalStockData({ ...valid, extra: true }, "600000"), /字段不符合协议契约/);
  assert.throws(() => parseHistoricalStockData({ code: "600000", active_daily_candle: null }, "600000"), /字段不符合协议契约/);
  assert.throws(() => parseHistoricalStockData({ ...valid, daily_candles: [{ ...valid.daily_candles[0], open: 1000 }] }, "600000"), /规范十进制整数分字符串/);
  assert.throws(() => parseHistoricalStockData({ ...valid, daily_candles: [{ ...valid.daily_candles[0], close: 1100 }] }, "600000"), /规范十进制整数分字符串/);
  assert.throws(() => parseHistoricalStockData({ ...valid, daily_candles: [valid.daily_candles[0], valid.daily_candles[0]] }, "600000"), /严格递增/);
  assert.throws(() => parseHistoricalStockData({ ...valid, active_daily_candle: { ...valid.active_daily_candle, time: 1 } }, "600000"), /必须晚于/);
});
