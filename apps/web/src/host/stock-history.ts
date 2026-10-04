import type { HistoricalStockData } from "../types/engine.ts";
import type { StockCode } from "../types/generated/StockCode.ts";
import { exact, field, record, values } from "./protocol/guards.ts";
import { parseDailyCandle } from "./protocol/wire-values.ts";

export function parseHistoricalStockData(value: unknown, requestedCode: StockCode): HistoricalStockData {
  const source = record(value, "股票历史响应");
  exact(source, ["code", "daily_candles", "active_daily_candle"], "股票历史响应");
  const code = field(source, "code", "股票历史响应");
  if (code !== requestedCode) throw new Error(`股票历史响应代码 ${String(code)} 与请求代码 ${requestedCode} 不匹配`);
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
