import type { DailyCandle } from "../types/generated/DailyCandle.ts";

/** 校验已结算开市日的独立日K；虚拟前史不属于此契约。 */
export function assertSettledDailyCandle(candle: DailyCandle, path: string): void {
  const [open, high, low, close] = [BigInt(candle.open), BigInt(candle.high), BigInt(candle.low), BigInt(candle.close)];
  if (low <= 0n || low > high || open < low || open > high || close < low || close > high) {
    throw new Error(`${path} 日K OHLC 不符合正价格范围`);
  }
  const stats = candle.trade_stats;
  if (stats === undefined || stats === null) throw new Error(`${path} 日K缺少真实日K成交统计`);
  const volume = BigInt(candle.volume);
  const count = BigInt(stats.trade_count);
  const turnover = BigInt(stats.turnover_cents);
  if (count > volume || ((count === 0n) !== (volume === 0n)) || turnover < low * volume || turnover > high * volume) {
    throw new Error(`${path} 日K成交统计与价量范围不一致`);
  }
}
