import type { HistogramData, UTCTimestamp } from "lightweight-charts";
import type { KlinePoint, PricePoint } from "./PriceChart.tsx";
import { parseIndicatorResults, type IndicatorInput, type IndicatorResults } from "./indicator-results.ts";
import { volumeHistogramData } from "./volume-histogram.ts";

type ChartType = "分时" | "日K";
interface IndicatorSource {
  readonly chartType: ChartType;
  readonly input: IndicatorInput;
  readonly points: readonly PricePoint[];
}

export function buildPriceChartIndicatorSource(chartType: ChartType, data: readonly PricePoint[], daily: readonly KlinePoint[]): IndicatorSource {
  if (chartType === "分时") return { chartType, input: { prices: data.map((point) => point.value) }, points: data };
  return {
    chartType,
    input: { prices: daily.map((candle) => candle.close), candles: daily.map(({ high, low, close }) => ({ high, low, close })) },
    points: daily.map((candle) => ({ time: candle.time, value: candle.close, volume: candle.volume, buy: candle.close >= candle.open })),
  };
}

function visibleStart(source: IndicatorSource, klineDays: number): number {
  if (!Number.isSafeInteger(klineDays) || klineDays <= 0) throw new RangeError("日 K 窗口必须是正安全整数");
  return source.chartType === "日K" ? Math.max(0, source.points.length - klineDays) : 0;
}

export function priceChartIndicatorData(source: IndicatorSource, response: IndicatorResults, klineDays: number) {
  const results = parseIndicatorResults(response, source.points.length, source.chartType === "日K" ? source.points.length : 0);
  const start = visibleStart(source, klineDays);
  const points = (values: readonly number[]) => values.map((value, index) => ({ time: source.points[index].time as UTCTimestamp, value })).slice(start);
  const kdj = source.chartType === "日K" ? results.candleKdj : results.priceKdj;
  return {
    dif: points(results.macd.dif), dea: points(results.macd.dea),
    histogram: results.macd.histogram.map((value, index) => ({ time: source.points[index].time as UTCTimestamp, value, color: results.macd.dif[index] >= results.macd.dea[index] ? "#d81e06" : "#009944" })).slice(start),
    k: points(kdj.k), d: points(kdj.d), j: points(kdj.j),
  };
}

export function priceChartVolumeData(source: IndicatorSource, klineDays: number): HistogramData<UTCTimestamp>[] {
  return volumeHistogramData(source.points.slice(visibleStart(source, klineDays)));
}
