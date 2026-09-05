import type { EngineEvent } from "../types/engine";
import type { PricePoint } from "../components/PriceChart";

/** 一个游戏世界分钟包含的 tick 数：每 tick = 游戏世界 1 秒。 */
export const TICKS_PER_TRADING_MINUTE = 60;

/**
 * 将单只股票的逐秒事件累积成一分钟分时点。
 *
 * Worker 会分批推送事件，因此聚合状态必须跨批次保存；到第 60 个 PriceTick
 * 才生成一个价格点和对应的一分钟成交量柱。
 */
export class MinutePointCollector {
  private ticksInMinute = 0;
  private volume = 0;
  private lastTradePrice: number | null = null;
  private previousMinutePrice: number | null = null;
  private readonly code: string;
  private readonly ticksPerMinute: number;

  constructor(code: string, ticksPerMinute = TICKS_PER_TRADING_MINUTE) {
    if (!Number.isSafeInteger(ticksPerMinute) || ticksPerMinute <= 0) {
      throw new RangeError("ticksPerMinute 必须是正整数");
    }
    this.code = code;
    this.ticksPerMinute = ticksPerMinute;
  }

  collect(events: EngineEvent[]): PricePoint[] {
    const result: PricePoint[] = [];

    for (const event of events) {
      if ("Trade" in event && event.Trade.code === this.code) {
        this.volume += event.Trade.qty;
        this.lastTradePrice = event.Trade.price / 100;
        continue;
      }
      if (!("PriceTick" in event) || event.PriceTick.code !== this.code) continue;

      this.ticksInMinute += 1;
      if (this.ticksInMinute < this.ticksPerMinute) continue;

      const value = event.PriceTick.last_price / 100;
      const directionPrice = this.lastTradePrice ?? value;
      result.push({
        time: event.PriceTick.seq,
        value,
        volume: this.volume,
        buy: this.previousMinutePrice === null ? this.volume > 0 : directionPrice >= this.previousMinutePrice,
      });
      this.ticksInMinute = 0;
      this.volume = 0;
      this.lastTradePrice = null;
      this.previousMinutePrice = value;
    }

    return result;
  }
}

export function tradingDayProgress(elapsedMinutes: number, totalMinutes: number): number {
  if (!Number.isFinite(totalMinutes) || totalMinutes <= 0) {
    throw new RangeError("totalMinutes 必须是正数");
  }
  return Math.min(1, Math.max(0, elapsedMinutes / totalMinutes));
}

/** A 股连续竞价时间；120 分钟后跳过午间休市。 */
export function formatTradingMinute(minute: number): string {
  const safeMinute = Math.min(239, Math.max(0, Math.floor(minute)));
  const total = safeMinute < 120 ? 9 * 60 + 30 + safeMinute : 13 * 60 + safeMinute - 120;
  const hours = Math.floor(total / 60);
  const minutes = total % 60;
  return `${String(hours).padStart(2, "0")}:${String(minutes).padStart(2, "0")}`;
}
