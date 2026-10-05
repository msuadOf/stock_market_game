import type { KlinePoint } from "../components/PriceChart.tsx";
import { candleDate } from "../utils/candle-date.ts";
import { parseTurnoverCents } from "../utils/turnover.ts";

export type CalendarCandlePeriod = "日K" | "周K" | "月K" | "季K" | "年K";

export function aggregateCandles(candles: readonly KlinePoint[], period: CalendarCandlePeriod): KlinePoint[] {
  if (!["日K", "周K", "月K", "季K", "年K"].includes(period)) throw new RangeError("未知 K 线周期");
  const groups: KlinePoint[][] = [];
  let previousTime: number | undefined;
  let previousKey: string | undefined;
  for (const [index, candle] of candles.entries()) {
    const date = candleDate(candle.time);
    if (previousTime !== undefined && candle.time <= previousTime) throw new RangeError("日 K 日期必须严格递增");
    if (candle.volume !== undefined && (!Number.isSafeInteger(candle.volume) || candle.volume < 0)) throw new RangeError("日 K 成交量必须是非负安全整数股数");
    previousTime = candle.time;
    const key = period === "日K" ? String(index)
      : period === "年K" ? String(date.getUTCFullYear())
      : period === "季K" ? `${date.getUTCFullYear()}-${Math.floor(date.getUTCMonth() / 3)}`
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
    let tradeStats: KlinePoint["tradeStats"];
    if (group.every((candle) => candle.tradeStats !== undefined)) {
      const turnover = group.reduce((sum, candle) => sum + BigInt(parseTurnoverCents(candle.tradeStats!.turnoverCents, "K线成交额")), 0n);
      const tradeCount = group.reduce((sum, candle) => {
        const count = candle.tradeStats!.tradeCount;
        if (!Number.isSafeInteger(count) || count < 0) throw new RangeError("K线成交笔数必须是非负安全整数");
        return sum + count;
      }, 0);
      if (!Number.isSafeInteger(tradeCount)) throw new RangeError("聚合K线成交笔数超出安全整数范围");
      tradeStats = { turnoverCents: parseTurnoverCents(turnover.toString(), "聚合K线成交额"), tradeCount };
    }
    return {
      time: first.time, open: first.open, high: high.high, low: low.low, close: last.close, volume,
      ...(hasRawPrices ? { rawPrices: Object.freeze({ open: first.rawPrices!.open, high: high.rawPrices!.high, low: low.rawPrices!.low, close: last.rawPrices!.close }) } : {}),
      ...(tradeStats === undefined ? {} : { tradeStats }),
    };
  });
}
