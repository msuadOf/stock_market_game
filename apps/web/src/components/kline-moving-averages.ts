import type { KlinePoint } from "./PriceChart.tsx";
import { exactMovingAverage } from "./moving-average.ts";

export const KLINE_MOVING_AVERAGES = [
  { days: 5, color: "#ff8a00" }, { days: 10, color: "#1890ff" },
  { days: 20, color: "#c82bb5" }, { days: 30, color: "#00bfd8" },
  { days: 60, color: "#ff6500" },
] as const;

/** 展示用收盘价简单移动平均；不足完整周期不补值。先计算历史再裁剪显示窗口。 */
export function klineMovingAverage(candles: readonly KlinePoint[], days: number, visibleDays: number) {
  if (!Number.isSafeInteger(days) || days <= 0 || !Number.isSafeInteger(visibleDays) || visibleDays <= 0) throw new RangeError("均线周期与显示窗口必须为正整数");
  const result: { time: KlinePoint["time"]; value: number }[] = [];
  const start = Math.max(0, candles.length - visibleDays);
  exactMovingAverage(candles, days).forEach((value, index) => {
    if (value !== null && index >= start) result.push({ time: candles[index].time, value });
  });
  return result;
}
