import assert from "node:assert/strict";
import test from "node:test";
import { resolveIndicatorRoute, type IndicatorCapabilities } from "./indicator-source-policy.ts";

const capabilities: IndicatorCapabilities = {
  intradayAverage: true,
  macd: true,
  priceKdj: true,
  candleKdj: true,
};

test("默认前端源使用支持的分时VWAP，未实现的前端指标明确不支持", () => {
  assert.deepEqual(resolveIndicatorRoute("frontend", "intradayAverage", capabilities), { kind: "frontend" });
  assert.deepEqual(resolveIndicatorRoute("frontend", "macd", capabilities), {
    kind: "unsupported", requested: "frontend", indicator: "macd",
  });
});

test("显式Rust源仅消费相应能力，不支持时绝不回退前端", () => {
  assert.deepEqual(resolveIndicatorRoute("rust", "intradayAverage", capabilities), { kind: "rust" });
  assert.deepEqual(resolveIndicatorRoute("rust", "candleKdj", { ...capabilities, candleKdj: false }), {
    kind: "unsupported", requested: "rust", indicator: "candleKdj",
  });
});
