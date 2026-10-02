import type { IndicatorInput, IndicatorResults, OhlcIndicatorInput } from "../components/indicator-results.ts";
import { parseIndicatorResults } from "../components/indicator-results.ts";

export function normalizeIndicatorInput(value: IndicatorInput): { readonly prices: number[]; readonly candles: OhlcIndicatorInput[] } {
  if (!Array.isArray(value.prices) || !value.prices.every(Number.isFinite)) throw new TypeError("指标 prices 必须是有限数值数组");
  const candles = value.candles ?? [];
  if (!Array.isArray(candles)) throw new TypeError("指标 candles 必须是数组");
  const normalizedCandles = candles.map((candle, index) => {
    if (candle === null || typeof candle !== "object" || !Number.isFinite(candle.high) || !Number.isFinite(candle.low) || !Number.isFinite(candle.close)) {
      throw new TypeError(`指标 candles[${index}] 必须包含有限 high/low/close`);
    }
    if (candle.high < candle.low) throw new RangeError(`指标 candles[${index}].high 不得小于 low`);
    if (candle.close < candle.low || candle.close > candle.high) throw new RangeError(`指标 candles[${index}].close 必须位于 low 与 high 之间`);
    return { high: candle.high, low: candle.low, close: candle.close };
  });
  return { prices: [...value.prices], candles: normalizedCandles };
}

export function normalizeIndicatorResults(value: unknown, input: IndicatorInput): IndicatorResults {
  const normalizedInput = normalizeIndicatorInput(input);
  return parseIndicatorResults(value, normalizedInput.prices.length, normalizedInput.candles.length);
}
