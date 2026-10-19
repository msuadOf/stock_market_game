/**
 * 分时价格图（TradingView Lightweight Charts v5）。
 * 显示选中股票的实时价格走势线 + 量能柱 + MACD/KDJ 可选指标 pane。
 */
import { useEffect, useMemo, useRef, useState } from "react";
import type { UTCTimestamp } from "lightweight-charts";
import { PriceChartRuntime, type PriceChartIndicator } from "./price-chart-runtime.ts";
import type { IndicatorCalculator } from "./indicator-results.ts";
import { useIndicatorResults } from "./useIndicatorResults.ts";
import { buildPriceChartIndicatorSource } from "./price-chart-indicators.ts";
import { DesktopIntradayChart } from "./DesktopIntradayChart.tsx";
import type { MobileIntradayProjection } from "../mobile/market-model.ts";
import { moneyToChartNumber } from "../utils/money.ts";
import type { Cents } from "../types/engine.ts";
import type { IndicatorCapabilities } from "../host/engine-host.ts";
import type { IntradayAverageCurveInput, IntradayAverageResult } from "../host/engine-host.ts";
import { intradayAverageYuan } from "../mobile/market-model.ts";
import type { IndicatorDataSource } from "../store/store.ts";
import { resolveIndicatorRoute, type IndicatorName } from "./indicator-source-policy.ts";
import { useMovingAverageSettings } from "./useMovingAverageSettings.ts";
import { MovingAverageSettings } from "./MovingAverageSettings.tsx";
import { exactMovingAverage, MOVING_AVERAGE_COLORS } from "./moving-average.ts";

export interface PricePoint {
  time: number;
  value: number; // 元（yuan）
  volume?: number;
  buy?: boolean;
  cumulativeTurnoverCents?: string;
  cumulativeVolumeShares?: number;
  cumulativeTradeCount?: number;
}

export interface KlinePoint {
  time: UTCTimestamp;
  open: number;
  high: number;
  low: number;
  close: number;
  rawPrices?: Readonly<{ open: Cents; high: Cents; low: Cents; close: Cents }>;
  /** 当日真实成交股数；手工构造的展示数据可省略。 */
  volume?: number;
  /** 仅真实逐笔可对账的交易日存在；预置合成历史不伪造该统计。 */
  tradeStats?: { turnoverCents: string; tradeCount: number };
}

interface Props {
  data: readonly PricePoint[];
  intraday: MobileIntradayProjection;
  dayRange?: Readonly<{ high: number; low: number; volume?: number }>;
  /** Rust Snapshot 同步的已完成交易日 OHLC。 */
  dailyCandles?: readonly KlinePoint[];
  lastClose: Cents; // 精确昨收（分）
  chartType?: "分时" | "日K";
  movingAverageDays?: readonly number[];
  klineDays?: number; // 日K 显示天数（20/60/120/240/360）
  indicatorCalculator?: IndicatorCalculator | null;
  indicatorDataSource?: IndicatorDataSource;
  indicatorCapabilities?: IndicatorCapabilities;
  calculateIntradayAverageCurve?: (input: IntradayAverageCurveInput) => Promise<readonly (IntradayAverageResult | null)[]>;
  seriesKey?: string;
}

const EMPTY_PRICE_POINTS: readonly PricePoint[] = [];
const EMPTY_DAILY_CANDLES: readonly KlinePoint[] = [];

export function PriceChart({ data, intraday, dayRange, dailyCandles, lastClose, chartType = "分时", klineDays = 20, indicatorCalculator = null, indicatorDataSource = "frontend", indicatorCapabilities = { intradayAverage: false, macd: indicatorCalculator !== null, priceKdj: indicatorCalculator !== null, candleKdj: indicatorCalculator !== null }, calculateIntradayAverageCurve, seriesKey = "intraday" }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const indicatorContainerRef = useRef<HTMLDivElement>(null);
  const runtimeRef = useRef<PriceChartRuntime | null>(null);
  const movingAverages = useMovingAverageSettings();
  const movingAveragePlot = useMemo(() => {
    if (chartType !== "日K" || indicatorDataSource !== "frontend" || !movingAverages.available) return { lines: [], error: null };
    try {
      const candles = dailyCandles === undefined ? EMPTY_DAILY_CANDLES : dailyCandles;
      const start = Math.max(0, candles.length - Math.max(1, klineDays));
      const lines = movingAverages.settings.filter((item) => item.visible).map((item, index) => ({ period: item.period, color: MOVING_AVERAGE_COLORS[index % MOVING_AVERAGE_COLORS.length], points: exactMovingAverage(candles, item.period).flatMap((value, position) => value === null || position < start ? [] : [{ time: candles[position].time, value }]) }));
      return { lines, error: null };
    } catch (failure) { return { lines: [], error: `MA计算失败：${failure instanceof Error ? failure.message : String(failure)}；请反馈错误详情。` }; }
  }, [chartType, indicatorDataSource, dailyCandles, klineDays, movingAverages.settings, movingAverages.available]);

  const [indicator, setIndicator] = useState<PriceChartIndicator>("volume");
  const [rustAveragePoints, setRustAveragePoints] = useState<readonly { time: number; value: number }[]>([]);
  const [rustAverageError, setRustAverageError] = useState<string | null>(null);
  const [rustAveragePending, setRustAveragePending] = useState(false);
  const indicatorPrices = chartType === "分时" ? data : EMPTY_PRICE_POINTS;
  const indicatorCandles = chartType === "日K" ? dailyCandles ?? EMPTY_DAILY_CANDLES : EMPTY_DAILY_CANDLES;
  const indicatorSource = useMemo(() => buildPriceChartIndicatorSource(chartType, indicatorPrices, indicatorCandles), [chartType, indicatorPrices, indicatorCandles]);
  const indicatorName: IndicatorName | null = indicator === "intradayAverage"
    ? "intradayAverage"
    : indicator === "macd"
    ? "macd"
    : indicator === "kdj"
      ? chartType === "日K" ? "candleKdj" : "priceKdj"
      : null;
  const route = indicatorName === null ? null : resolveIndicatorRoute(indicatorDataSource, indicatorName, indicatorCapabilities);
  const sampledPoints = useMemo(() => data.filter((point) => point.cumulativeTurnoverCents !== undefined && point.cumulativeVolumeShares !== undefined && point.cumulativeTradeCount !== undefined), [data]);
  const samples = useMemo(() => sampledPoints.map((point) => ({ turnoverCents: point.cumulativeTurnoverCents!, volumeShares: point.cumulativeVolumeShares!, tradeCount: point.cumulativeTradeCount! })), [sampledPoints]);
  useEffect(() => {
    let current = true;
    setRustAveragePoints([]);
    setRustAverageError(null);
    setRustAveragePending(false);
    if (indicator !== "intradayAverage" || chartType !== "分时" || route?.kind !== "rust") return () => { current = false; };
    if (calculateIntradayAverageCurve === undefined) {
      setRustAverageError("Rust 分时均价曲线计算入口未接线");
      return () => { current = false; };
    }
    setRustAveragePending(true);
    void calculateIntradayAverageCurve({ seriesKey, samples }).then((results) => {
      if (!current) return;
      if (results.length !== sampledPoints.length) throw new Error("Rust 分时均价曲线结果长度与请求不一致");
      setRustAveragePoints(results.flatMap((result, index) => {
        const sample = samples[index];
        if (result === null) {
          if (sample.turnoverCents !== "0" || sample.volumeShares !== 0 || sample.tradeCount !== 0) throw new Error("Rust 分时均价曲线对非零成交事实返回无结果");
          return [];
        }
        if (result.turnoverCents !== sample.turnoverCents || result.volumeShares !== sample.volumeShares) throw new Error("Rust 分时均价曲线返回统计与请求不一致");
        const value = intradayAverageYuan(result.turnoverCents, result.volumeShares);
        return value === null ? [] : [{ time: sampledPoints[index].time, value }];
      }));
      setRustAveragePending(false);
    }).catch((error: unknown) => {
      if (current) {
        setRustAverageError(`Rust 分时均价计算失败：${error instanceof Error ? error.message : String(error)}`);
        setRustAveragePending(false);
      }
    });
    return () => { current = false; };
  }, [calculateIntradayAverageCurve, chartType, indicator, route?.kind, sampledPoints, samples, seriesKey]);
  const averagePoints = indicator !== "intradayAverage" || chartType !== "分时" || route?.kind === "unsupported"
    ? []
    : route?.kind === "frontend"
      ? data.flatMap((point) => {
        if (point.cumulativeTurnoverCents === undefined || point.cumulativeVolumeShares === undefined) return [];
        const value = intradayAverageYuan(point.cumulativeTurnoverCents, point.cumulativeVolumeShares);
        return value === null ? [] : [{ time: point.time, value }];
      })
      : rustAveragePoints;
  const selectedCalculator = route?.kind === "rust" ? indicatorCalculator : null;
  const indicatorResult = useIndicatorResults(
    selectedCalculator,
    indicatorSource.input,
    indicator === "macd" || indicator === "kdj",
  );

  // 每次挂载 effect 单独拥有本次创建的 runtime。
  useEffect(() => {
    const runtime = PriceChartRuntime.create({ main: containerRef.current, indicator: indicatorContainerRef.current });
    runtimeRef.current = runtime;
    return () => {
      runtime?.dispose();
      if (runtimeRef.current === runtime) runtimeRef.current = null;
    };
  }, []);

  useEffect(() => {
    runtimeRef.current?.updatePrice(data, dailyCandles, moneyToChartNumber(lastClose) / 100, chartType, klineDays);
  }, [data, dailyCandles, lastClose, chartType, klineDays]);

  useEffect(() => {
    runtimeRef.current?.updateIndicator(indicator, indicatorSource, indicatorResult, klineDays);
  }, [indicatorSource, klineDays, indicator, indicatorResult]);

  useEffect(() => {
    runtimeRef.current?.updateAverageOverlay(averagePoints);
  }, [averagePoints]);
  useEffect(() => { runtimeRef.current?.updateMovingAverageOverlays(movingAveragePlot.lines); }, [movingAveragePlot.lines]);

  return (
    <div className="price-chart" style={{ width: "100%" }}>
      {chartType === "日K" && <><MovingAverageSettings {...movingAverages} />{indicatorDataSource === "rust" && movingAverages.settings.some((item) => item.visible) && <p role="status">Rust指标源不支持MA，未回退到前端。</p>}{movingAveragePlot.error !== null && <p role="alert">{movingAveragePlot.error}</p>}</>}
      {chartType === "日K" && (!dailyCandles || dailyCandles.length === 0) && (
        <div className="chart-empty" role="status">暂无权威日 K 数据，请检查引擎快照。</div>
      )}
      {chartType === "分时" && <DesktopIntradayChart projection={intraday} dayRange={dayRange} lastClose={lastClose} indicator={indicator} result={indicatorResult} averagePoints={averagePoints} />}
      <div hidden={chartType === "分时"} className="price-chart-main" ref={containerRef} style={{ width: "100%", height: 180 }} />
      <div hidden={chartType === "分时" || indicator === "none"} className="price-chart-indicator" ref={indicatorContainerRef} style={{ width: "100%", height: 60 }} />
      {(indicator === "macd" || indicator === "kdj") && indicatorResult.kind !== "ready" && (
        <div role={indicatorResult.kind === "error" || indicatorResult.kind === "unavailable" ? "alert" : "status"}>
          {route?.kind === "unsupported" ? `所选${indicatorDataSource === "rust" ? "Rust" : "前端"}指标源不支持${indicator === "macd" ? " MACD" : " KDJ"}，未回退到另一数据源。` : indicatorResult.kind === "pending" ? "Rust 指标计算中…" : indicatorResult.kind === "error" ? `Rust 指标计算失败：${indicatorResult.message}` : indicatorResult.kind === "unavailable" ? "Rust 指标宿主尚未就绪" : ""}
        </div>
      )}
      {indicator === "intradayAverage" && route?.kind === "unsupported" && <div role="alert">所选{indicatorDataSource === "rust" ? "Rust" : "前端"}指标源不支持分时均价，未回退到另一数据源。</div>}
      {indicator === "intradayAverage" && route?.kind === "rust" && rustAverageError !== null && <div role="alert">{rustAverageError}</div>}
      {indicator === "intradayAverage" && route?.kind === "rust" && rustAverageError === null && <div role="status">{rustAveragePending ? "Rust 分时均价曲线计算中…" : rustAveragePoints.length === 0 ? "暂无可证分时均价点" : "Rust 分时均价曲线"}</div>}
      <div style={{ display: "flex", gap: "4px", marginTop: "4px" }}>
        {([
          ...(chartType === "分时" ? [["intradayAverage", "均价"] as const] : []),
          ["volume", "量能"],
          ["macd", "MACD"],
          ["kdj", "KDJ"],
          ["none", "无"],
        ] as const).map(([type, label]) => (
          <button
            key={type}
            className="chart-indicator-button"
            aria-pressed={indicator === type}
            onClick={() => setIndicator(type)}

          >
            {label}
          </button>
        ))}
      </div>
    </div>
  );
}
