import type { KlinePoint } from "./PriceChart.tsx";

/** 报价摘要只使用权威日 K；零成交时的昨收占位不代表真实开高低。 */
export function marketQuoteFacts(candle: KlinePoint | undefined) {
  if (candle !== undefined && candle.rawPrices === undefined) {
    throw new Error("当日 K 线缺少精确分值，不能展示报价摘要");
  }
  const volume = candle?.volume ?? 0;
  const prices = volume > 0 ? candle!.rawPrices! : null;
  const statsUnavailable = volume > 0 && candle?.tradeStats === undefined;
  return {
    prices,
    volume,
    statsUnavailable,
    turnoverCents: statsUnavailable ? null : (candle?.tradeStats?.turnoverCents ?? "0"),
    tradeCount: statsUnavailable ? null : (candle?.tradeStats?.tradeCount ?? 0),
  };
}
