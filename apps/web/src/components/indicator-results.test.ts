import assert from "node:assert/strict";
import { test } from "node:test";
import { IndicatorRequestGate, parseIndicatorResults } from "./indicator-results.ts";
import { normalizeIndicatorInput } from "../host/indicator-transport.ts";

function result(priceCount: number, candleCount: number) {
  const series = (length: number) => Array.from({ length }, (_, index) => index + 1);
  return {
    macd: { dif: series(priceCount), dea: series(priceCount), histogram: series(priceCount) },
    priceKdj: { k: series(priceCount), d: series(priceCount), j: series(priceCount) },
    candleKdj: { k: series(candleCount), d: series(candleCount), j: series(candleCount) },
  };
}

test("指标响应按各输入序列长度校验并保留数值", () => {
  assert.deepEqual(parseIndicatorResults(result(2, 1), 2, 1), result(2, 1));
  assert.deepEqual(parseIndicatorResults(result(0, 0), 0, 0), result(0, 0));
  assert.throws(() => parseIndicatorResults(result(1, 1), 2, 1), /MACD DIF长度/);
  const malformed = result(1, 1);
  malformed.candleKdj.j[0] = Number.NaN;
  assert.throws(() => parseIndicatorResults(malformed, 1, 1), /有限数值/);
});

test("新指标请求和宿主替换后使旧响应失效", () => {
  const gate = new IndicatorRequestGate();
  const first = gate.capture();
  gate.invalidate();
  const second = gate.capture();
  gate.invalidate();

  assert.equal(gate.isCurrent(first), false);
  assert.equal(gate.isCurrent(second), false);
  assert.equal(gate.isCurrent(gate.capture()), true);
});

test("指标输入拒绝非有限价格及 high 小于 low 的 OHLC", () => {
  assert.throws(() => normalizeIndicatorInput({ prices: [Number.POSITIVE_INFINITY] }), /prices.*有限/);
  assert.throws(() => normalizeIndicatorInput({ prices: [], candles: [{ high: 9, low: 10, close: 9 }] }), /high.*low/);
});
