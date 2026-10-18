import type { KlinePoint } from "../components/PriceChart.tsx";
import { candleDate } from "../utils/candle-date.ts";

export type CandleAggregationBasis = "calendar" | "trading-days";

export function aggregateCandles(candles: readonly KlinePoint[], period: "日K" | "周K" | "月K", basis: CandleAggregationBasis = "calendar"): KlinePoint[] {
  if (basis !== "calendar" && basis !== "trading-days") throw new RangeError("未知 K 线聚合口径");
  if (period !== "日K" && period !== "周K" && period !== "月K") throw new RangeError("未知 K 线周期");
  const groups: KlinePoint[][] = [];
  let previousTime: number | undefined;
  let previousKey: string | undefined;
  for (const [index, candle] of candles.entries()) {
    const date = candleDate(candle.time);
    if (previousTime !== undefined && candle.time <= previousTime) throw new RangeError("日 K 日期必须严格递增");
    if (candle.volume !== undefined && (!Number.isSafeInteger(candle.volume) || candle.volume < 0)) throw new RangeError("日 K 成交量必须是非负安全整数股数");
    previousTime = candle.time;
    const key = period === "日K" ? String(index) : basis === "trading-days"
      ? String(Math.floor(index / (period === "周K" ? 5 : 20)))
      : period === "月K" ? `${date.getUTCFullYear()}-${date.getUTCMonth()}`
      : String(candle.time / 86400 - (date.getUTCDay() + 6) % 7);
    if (key !== previousKey) groups.push([]);
    groups.at(-1)!.push(candle);
    previousKey = key;
  }
  return groups.map((group) => {
    if (period === "日K") return { ...group[0] };
    const first = group[0];
    const last = group.at(-1)!;
    const hasRawPrices = group.every((candle) => candle.rawPrices !== undefined);
    const high = group.reduce((highest, candle) => (hasRawPrices ? BigInt(candle.rawPrices!.high) > BigInt(highest.rawPrices!.high) : candle.high > highest.high) ? candle : highest, first);
    const low = group.reduce((lowest, candle) => (hasRawPrices ? BigInt(candle.rawPrices!.low) < BigInt(lowest.rawPrices!.low) : candle.low < lowest.low) ? candle : lowest, first);
    const volume = group.reduce((sum, candle) => sum + (candle.volume === undefined ? 0 : candle.volume), 0);
    if (!Number.isSafeInteger(volume)) throw new RangeError("聚合 K 线成交量超出安全整数范围");
    return {
      time: first.time, open: first.open, high: high.high, low: low.low, close: last.close, volume,
      ...(hasRawPrices ? { rawPrices: Object.freeze({ open: first.rawPrices!.open, high: high.rawPrices!.high, low: low.rawPrices!.low, close: last.rawPrices!.close }) } : {}),
    };
  });
}
