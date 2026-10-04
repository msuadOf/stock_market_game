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

export interface PricePoint {
  time: number;
  value: number; // 元（yuan）
  volume?: number;
  buy?: boolean;
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
}

const EMPTY_MA_DAYS: readonly number[] = [];
const EMPTY_PRICE_POINTS: readonly PricePoint[] = [];
const EMPTY_DAILY_CANDLES: readonly KlinePoint[] = [];

export function PriceChart({ data, intraday, dayRange, dailyCandles, lastClose, chartType = "分时", klineDays = 20, movingAverageDays = EMPTY_MA_DAYS, indicatorCalculator = null }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const indicatorContainerRef = useRef<HTMLDivElement>(null);
  const runtimeRef = useRef<PriceChartRuntime | null>(null);

  const [indicator, setIndicator] = useState<PriceChartIndicator>("volume");
  const indicatorPrices = chartType === "分时" ? data : EMPTY_PRICE_POINTS;
  const indicatorCandles = chartType === "日K" ? dailyCandles ?? EMPTY_DAILY_CANDLES : EMPTY_DAILY_CANDLES;
  const indicatorSource = useMemo(() => buildPriceChartIndicatorSource(chartType, indicatorPrices, indicatorCandles), [chartType, indicatorPrices, indicatorCandles]);
  const indicatorResult = useIndicatorResults(
    indicatorCalculator,
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
    runtimeRef.current?.updateMovingAverages(dailyCandles ?? EMPTY_DAILY_CANDLES, movingAverageDays, chartType, klineDays);
  }, [dailyCandles, movingAverageDays, chartType, klineDays]);

  return (
    <div className="price-chart" style={{ width: "100%" }}>
      {chartType === "日K" && (!dailyCandles || dailyCandles.length === 0) && (
        <div className="chart-empty" role="status">暂无权威日 K 数据，请检查引擎快照。</div>
      )}
      {chartType === "分时" && <DesktopIntradayChart projection={intraday} dayRange={dayRange} lastClose={lastClose} indicator={indicator} result={indicatorResult} />}
      <div hidden={chartType === "分时"} className="price-chart-main" ref={containerRef} style={{ width: "100%", height: 180 }} />
      <div hidden={chartType === "分时" || indicator === "none"} className="price-chart-indicator" ref={indicatorContainerRef} style={{ width: "100%", height: 60 }} />
      {(indicator === "macd" || indicator === "kdj") && indicatorResult.kind !== "ready" && (
        <div role={indicatorResult.kind === "error" || indicatorResult.kind === "unavailable" ? "alert" : "status"}>
          {indicatorResult.kind === "pending" ? "Rust 指标计算中…" : indicatorResult.kind === "error" ? `Rust 指标计算失败：${indicatorResult.message}` : indicatorResult.kind === "unavailable" ? "Rust 指标宿主尚未就绪" : ""}
        </div>
      )}
      <div style={{ display: "flex", gap: "4px", marginTop: "4px" }}>
        {([
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
