import assert from "node:assert/strict";
import { test } from "node:test";
import * as indicatorResults from "./indicator-results.ts";
import { IndicatorRequestGate, parseIndicatorResults, type IndicatorInput } from "./indicator-results.ts";
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

test("指标请求绑定引用身份并仅提交当前 generation 的转换结果", () => {
  const gate = new IndicatorRequestGate();
  const calculator = async () => result(2, 1);
  const input: IndicatorInput = { prices: [10, 11], candles: [{ high: 11, low: 10, close: 11 }] };
  const { IndicatorResultRequest } = indicatorResults;
  assert.equal(typeof IndicatorResultRequest, "function");
  const first = IndicatorResultRequest.capture(gate, calculator, input);
  assert.equal(first.matches(calculator, input), true);
  assert.equal(first.matches(calculator, { ...input }), false);
  assert.equal(first.matches(async () => result(2, 1), input), false);
  assert.equal(first.pendingRecord().state.kind, "pending");
  gate.invalidate();
  const second = IndicatorResultRequest.capture(gate, calculator, input);
  assert.equal(first.resolveRecord(result(2, 1)), null);
  assert.equal(first.rejectRecord(new Error("旧失败")), null);
  assert.deepEqual(second.resolveRecord(result(2, 1))?.state, { kind: "ready", value: result(2, 1) });
  assert.deepEqual(second.resolveRecord(result(1, 1))?.state, { kind: "error", message: "RangeError: MACD DIF长度必须为 2" });
  assert.deepEqual(second.rejectRecord(new Error("计算失败"))?.state, { kind: "error", message: "Error: 计算失败" });
  assert.deepEqual(second.rejectRecord("计算失败")?.state, { kind: "error", message: "计算失败" });
  gate.invalidate();
  assert.equal(second.resolveRecord(result(2, 1)), null);
});
