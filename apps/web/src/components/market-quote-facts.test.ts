import assert from "node:assert/strict";
import { test } from "node:test";
import type { KlinePoint } from "./PriceChart.tsx";
import { marketQuoteFacts } from "./market-quote-facts.ts";

const candle: KlinePoint = {
  time: 0 as KlinePoint["time"], open: 10, high: 11, low: 9, close: 10,
  rawPrices: { open: "1000", high: "1100", low: "900", close: "1000" },
  volume: 250, tradeStats: { turnoverCents: "9007199254740993", tradeCount: 5 },
};

test("行情摘要保留精确分和全天统计，不从图表近似值还原", { timeout: 10000 }, () => {
  const facts = marketQuoteFacts({ ...candle, high: 99, rawPrices: { ...candle.rawPrices!, high: "9007199254740993" } });
  assert.equal(facts.prices?.high, "9007199254740993");
  assert.equal(facts.turnoverCents, "9007199254740993");
  assert.equal(facts.volume, 250);
  assert.equal(facts.tradeCount, 5);
});

test("缺少成交额笔数时显式不可用，不能用零或最新价乘数量冒充", { timeout: 10000 }, () => {
  assert.deepEqual(marketQuoteFacts({ ...candle, tradeStats: undefined }), {
    prices: candle.rawPrices, volume: 250, statsUnavailable: true, turnoverCents: null, tradeCount: null,
  });
  assert.throws(() => marketQuoteFacts({ ...candle, rawPrices: undefined }), /缺少精确分值/);
});

test("未形成当日日K与零成交占位均无开高低，统计诚实显示零", { timeout: 10000 }, () => {
  const empty = { prices: null, volume: 0, statsUnavailable: false, turnoverCents: "0", tradeCount: 0 };
  assert.deepEqual(marketQuoteFacts(undefined), empty);
  assert.deepEqual(marketQuoteFacts({ ...candle, volume: 0, tradeStats: undefined }), empty);
});
