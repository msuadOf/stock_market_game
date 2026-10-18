import type { HistoricalStockData } from "../types/engine.ts";
import type { StockCode } from "../types/generated/StockCode.ts";
import { exact, field, record, safeU32, values } from "./protocol/guards.ts";
import { stockCodeOf } from "./protocol/stock-identity.ts";
import { parseDailyCandle } from "./protocol/wire-values.ts";

export function parseHistoricalStockData(value: unknown, requestedCode: StockCode): HistoricalStockData {
  const source = record(value, "股票历史响应");
  exact(source, ["code", "daily_candles", "active_daily_candle"], "股票历史响应");
  // wire 的 code 为数值 StockId（ADR 0041）；摄入时译回代码串再与请求代码比对。
  const wireCode = safeU32(field(source, "code", "股票历史响应"), "股票历史响应.code");
  const code = stockCodeOf(wireCode, "股票历史响应.code");
  if (code !== requestedCode) throw new Error(`股票历史响应代码 ${code} 与请求代码 ${requestedCode} 不匹配`);
  const candles = values(field(source, "daily_candles", "股票历史响应"), "股票历史响应.daily_candles")
    .map((candle, index) => parseDailyCandle(candle, `股票历史响应.daily_candles[${index}]`));
  for (let index = 1; index < candles.length; index += 1) {
    if (candles[index - 1]!.time >= candles[index]!.time) throw new Error(`股票历史响应.daily_candles[${index}].time 必须严格递增`);
  }
  const rawActive = field(source, "active_daily_candle", "股票历史响应");
  const active = rawActive === null ? null : parseDailyCandle(rawActive, "股票历史响应.active_daily_candle");
  if (active !== null && candles.length > 0 && candles.at(-1)!.time >= active.time) {
    throw new Error("股票历史响应活跃日 K 线时间必须晚于已完成日 K 线");
  }
  return { code: requestedCode, daily_candles: candles, active_daily_candle: active };
}
