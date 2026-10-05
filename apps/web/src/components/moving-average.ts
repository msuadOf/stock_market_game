import type { KlinePoint } from "./PriceChart.tsx";
import { moneyToBigInt } from "../utils/money.ts";
export function exactMovingAverage(candles: readonly KlinePoint[], period: number): readonly (number | null)[] {
  if (!Number.isSafeInteger(period) || period <= 0) throw new Error("MA周期必须为正安全整数");
  const prices = candles.map((candle) => {
    if (candle.rawPrices === undefined) throw new Error("MA计算缺少原始分报价，不能使用近似图表值补齐");
    const close = moneyToBigInt(candle.rawPrices.close);
    if (close <= 0n) throw new Error("MA收盘价必须为正分金额");
    return close;
  });
  let sum = 0n;
  const denominator = BigInt(period) * 100n;
  return prices.map((price, index) => {
    sum += price;
    if (index >= period) sum -= prices[index - period];
    if (index + 1 < period) return null;
    return Number(sum / denominator) + Number(sum % denominator) / Number(denominator);
  });
}
export const MOVING_AVERAGE_COLORS = ["#926b00", "#335eae", "#9048a6", "#9c463c", "#28734b"] as const;
