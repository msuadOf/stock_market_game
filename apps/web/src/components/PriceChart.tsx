/**
 * 分时价格图（TradingView Lightweight Charts v5）。
 * 显示选中股票的实时价格走势线 + 量能柱 + MACD/KDJ 可选指标 pane。
 */
import { useEffect, useMemo, useRef, useState } from "react";
import {
  createChart,
  LineSeries,
  CandlestickSeries,
  HistogramSeries,
  ColorType,
  CrosshairMode,
  type UTCTimestamp,
  type IChartApi,
  type ISeriesApi,
} from "lightweight-charts";
import { formatLotAmount } from "../utils/format";
import { observeChartContainers } from "./chart-resize.ts";
import type { IndicatorCalculator } from "./indicator-results.ts";
import { useIndicatorResults } from "./useIndicatorResults.ts";
import { buildPriceChartIndicatorSource, priceChartIndicatorData, priceChartVolumeData } from "./price-chart-indicators.ts";

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
  /** 当日真实成交股数；手工构造的展示数据可省略。 */
  volume?: number;
  /** 仅真实逐笔可对账的交易日存在；预置合成历史不伪造该统计。 */
  tradeStats?: { turnoverCents: string; tradeCount: number };
}

interface Props {
  data: PricePoint[];
  /** Rust Snapshot 同步的已完成交易日 OHLC。 */
  dailyCandles?: KlinePoint[];
  lastClose: number; // 昨收（元），用于着色基准
  chartType?: "分时" | "日K";
  klineDays?: number; // 日K 显示天数（20/60/120/240/360）
  indicatorCalculator?: IndicatorCalculator | null;
}

type IndicatorType = "none" | "volume" | "macd" | "kdj";
const EMPTY_PRICE_POINTS: readonly PricePoint[] = [];
const EMPTY_DAILY_CANDLES: readonly KlinePoint[] = [];

export function PriceChart({ data, dailyCandles, lastClose, chartType = "分时", klineDays = 20, indicatorCalculator = null }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const priceSeriesRef = useRef<ISeriesApi<"Line"> | null>(null);
  const candleSeriesRef = useRef<ISeriesApi<"Candlestick"> | null>(null);
  const volSeriesRef = useRef<ISeriesApi<"Histogram"> | null>(null);
  const indicatorContainerRef = useRef<HTMLDivElement>(null);
  const indicatorChartRef = useRef<IChartApi | null>(null);
  const macdHistRef = useRef<ISeriesApi<"Histogram"> | null>(null);
  const macdDifRef = useRef<ISeriesApi<"Line"> | null>(null);
  const macdDeaRef = useRef<ISeriesApi<"Line"> | null>(null);
  const kdjKRef = useRef<ISeriesApi<"Line"> | null>(null);
  const kdjDRef = useRef<ISeriesApi<"Line"> | null>(null);
  const kdjJRef = useRef<ISeriesApi<"Line"> | null>(null);

  const [indicator, setIndicator] = useState<IndicatorType>("volume");
  const indicatorPrices = chartType === "分时" ? data : EMPTY_PRICE_POINTS;
  const indicatorCandles = chartType === "日K" ? dailyCandles ?? EMPTY_DAILY_CANDLES : EMPTY_DAILY_CANDLES;
  const indicatorSource = useMemo(() => buildPriceChartIndicatorSource(chartType, indicatorPrices, indicatorCandles), [chartType, indicatorPrices, indicatorCandles]);
  const indicatorResult = useIndicatorResults(
    indicatorCalculator,
    indicatorSource.input,
    indicator === "macd" || indicator === "kdj",
  );

  // 创建主图 + 量能副图（仅挂载时）
  useEffect(() => {
    if (!containerRef.current) return;
    const chart = createChart(containerRef.current, {
      width: containerRef.current.clientWidth,
      height: 180,
      layout: {
        background: { type: ColorType.Solid, color: "transparent" },
        textColor: "#222",
        fontFamily: '"Microsoft YaHei", sans-serif',
        fontSize: 11,
      },
      grid: { vertLines: { color: "rgba(0,0,0,0.04)" }, horzLines: { color: "rgba(0,0,0,0.04)" } },
      rightPriceScale: { borderColor: "#ddd" },
      // 游戏分时图只表达相对行情，不展示真实日历；否则内部序号会被渲染为 1970 年日期。
      timeScale: { visible: false },
      crosshair: { mode: CrosshairMode.Normal },
    });
    const priceSeries = chart.addSeries(LineSeries, {
      color: "#d81e06",
      lineWidth: 2,
      priceFormat: { type: "price", precision: 2, minMove: 0.01 },
    });
    chartRef.current = chart;
    priceSeriesRef.current = priceSeries;

    // 日K 蜡烛图（默认隐藏，切日K时显示）
    const candleSeries = chart.addSeries(CandlestickSeries, {
      upColor: "#d81e06",
      downColor: "#009944",
      borderUpColor: "#d81e06",
      borderDownColor: "#009944",
      wickUpColor: "#d81e06",
      wickDownColor: "#009944",
      priceFormat: { type: "price", precision: 2, minMove: 0.01 },
    });
    candleSeries.applyOptions({ visible: false });
    candleSeriesRef.current = candleSeries;

    // 量能副图（默认显示）
    if (indicatorContainerRef.current) {
      const volChart = createChart(indicatorContainerRef.current, {
        width: indicatorContainerRef.current.clientWidth,
        height: 60,
        layout: {
          background: { type: ColorType.Solid, color: "transparent" },
          textColor: "#888",
          fontFamily: '"Microsoft YaHei", sans-serif',
          fontSize: 10,
        },
        grid: { vertLines: { visible: false }, horzLines: { visible: false } },
        rightPriceScale: { visible: false },
        timeScale: { visible: false },
      });
      indicatorChartRef.current = volChart;
      const volSeries = volChart.addSeries(HistogramSeries, {
        priceFormat: { type: "custom", minMove: 0.01, formatter: (value: number) => `${formatLotAmount(value)}手` },
        priceScaleId: "",
      });
      volSeries.priceScale().applyOptions({ scaleMargins: { top: 0.2, bottom: 0 } });
      volSeriesRef.current = volSeries;
    }

    const handleResize = () => {
      if (containerRef.current) chart.applyOptions({ width: containerRef.current.clientWidth });
      if (indicatorContainerRef.current && indicatorChartRef.current)
        indicatorChartRef.current.applyOptions({ width: indicatorContainerRef.current.clientWidth });
    };
    const disconnectResizeObserver = observeChartContainers(
      [containerRef.current, indicatorContainerRef.current].filter((element): element is HTMLDivElement => element !== null),
      handleResize,
    );
    window.addEventListener("resize", handleResize);
    return () => {
      disconnectResizeObserver();
      window.removeEventListener("resize", handleResize);
      chart.remove();
      indicatorChartRef.current?.remove();
      chartRef.current = null;
      candleSeriesRef.current = null;
      indicatorChartRef.current = null;
      volSeriesRef.current = null;
      macdHistRef.current = null;
      macdDifRef.current = null;
      macdDeaRef.current = null;
      kdjKRef.current = null;
      kdjDRef.current = null;
      kdjJRef.current = null;
    };
  }, []);

  // 周期切换和空数据也要同步主图，不能保留上一股票或上一存档的曲线。
  useEffect(() => {
    if (chartType === "日K") {
      // 显示蜡烛图、隐藏分时线
      priceSeriesRef.current?.setData([]);
      priceSeriesRef.current?.applyOptions({ visible: false });
      candleSeriesRef.current?.applyOptions({ visible: true });

      // 日 K 只使用引擎权威 OHLC，不从当日分时缓存合成历史。
      if (candleSeriesRef.current) {
        const candles = dailyCandles ?? [];
        const visible = candles.slice(-Math.max(1, klineDays));
        // 每次完整写入可正确处理：盘中蜡烛更新、跨日新增、以及 20/60/120/240/360 窗口切换。
        // 最多 360 根，远低于图表库的性能阈值，可靠性比只更新最后一根更重要。
        candleSeriesRef.current.setData(visible);
        chartRef.current?.timeScale().fitContent();
      }
    } else {
      // 分时模式：显示折线、隐藏蜡烛图
      candleSeriesRef.current?.setData([]);
      priceSeriesRef.current?.applyOptions({ visible: true });
      candleSeriesRef.current?.applyOptions({ visible: false });

      if (priceSeriesRef.current) {
        if (data.length === 0) {
          priceSeriesRef.current.setData([]);
          return;
        }
        const lastVal = data[data.length - 1].value;
        const color = lastVal > lastClose ? "#d81e06" : lastVal < lastClose ? "#009944" : "#b8b8b8";
        priceSeriesRef.current.applyOptions({ color });

        priceSeriesRef.current.setData(data.map((d) => ({ time: d.time as UTCTimestamp, value: d.value })));
        chartRef.current?.timeScale().fitContent();
      }
    }
  }, [data, dailyCandles, lastClose, chartType, klineDays]);

  // 副图数据更新
  useEffect(() => {
    if (!indicatorChartRef.current) return;
    const chart = indicatorChartRef.current;

    // 清除旧 series（切换指标时）
    if (indicator !== "volume" && volSeriesRef.current) {
      chart.removeSeries(volSeriesRef.current);
      volSeriesRef.current = null;
    }
    if (indicator !== "macd") {
      [macdHistRef, macdDifRef, macdDeaRef].forEach((ref) => {
        if (ref.current) { chart.removeSeries(ref.current); ref.current = null; }
      });
    }
    if (indicator !== "kdj") {
      [kdjKRef, kdjDRef, kdjJRef].forEach((ref) => {
        if (ref.current) { chart.removeSeries(ref.current); ref.current = null; }
      });
    }

    if (indicator === "volume") {
      if (!volSeriesRef.current) {
        const s = chart.addSeries(HistogramSeries, { priceFormat: { type: "custom", minMove: 0.01, formatter: (value: number) => `${formatLotAmount(value)}手` }, priceScaleId: "" });
        s.priceScale().applyOptions({ scaleMargins: { top: 0.2, bottom: 0 } });
        volSeriesRef.current = s;
      }
      volSeriesRef.current.setData(priceChartVolumeData(indicatorSource, klineDays));
    } else if (indicator === "macd") {
      if (indicatorResult.kind !== "ready") {
        [macdHistRef, macdDifRef, macdDeaRef].forEach((ref) => ref.current?.setData([]));
        return;
      }
      const plotted = priceChartIndicatorData(indicatorSource, indicatorResult.value, klineDays);
      if (!macdDifRef.current) {
        macdDifRef.current = chart.addSeries(LineSeries, { color: "#d85b73", lineWidth: 1, priceScaleId: "" });
        macdDeaRef.current = chart.addSeries(LineSeries, { color: "#6ca6e8", lineWidth: 1, priceScaleId: "" });
        macdHistRef.current = chart.addSeries(HistogramSeries, { priceScaleId: "" });
        macdHistRef.current.priceScale().applyOptions({ scaleMargins: { top: 0.3, bottom: 0.1 } });
      }
      macdDifRef.current!.setData(plotted.dif);
      macdDeaRef.current!.setData(plotted.dea);
      macdHistRef.current!.setData(plotted.histogram);
    } else if (indicator === "kdj") {
      if (indicatorResult.kind !== "ready") {
        [kdjKRef, kdjDRef, kdjJRef].forEach((ref) => ref.current?.setData([]));
        return;
      }
      const plotted = priceChartIndicatorData(indicatorSource, indicatorResult.value, klineDays);
      if (!kdjKRef.current) {
        kdjKRef.current = chart.addSeries(LineSeries, { color: "#e6a400", lineWidth: 1, priceScaleId: "" });
        kdjDRef.current = chart.addSeries(LineSeries, { color: "#c56ae6", lineWidth: 1, priceScaleId: "" });
        kdjJRef.current = chart.addSeries(LineSeries, { color: "#4ea15f", lineWidth: 1, priceScaleId: "" });
        kdjKRef.current.priceScale().applyOptions({ scaleMargins: { top: 0.1, bottom: 0.1 } });
      }
      kdjKRef.current!.setData(plotted.k);
      kdjDRef.current!.setData(plotted.d);
      kdjJRef.current!.setData(plotted.j);
    }
    chart.timeScale().fitContent();
  }, [indicatorSource, klineDays, indicator, indicatorResult]);

  return (
    <div style={{ width: "100%" }}>
      {chartType === "日K" && (!dailyCandles || dailyCandles.length === 0) && (
        <div className="chart-empty" role="status">暂无权威日 K 数据，请检查引擎快照。</div>
      )}
      <div ref={containerRef} style={{ width: "100%", height: 180 }} />
      <div ref={indicatorContainerRef} style={{ width: "100%", height: 60 }} />
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
            onClick={() => setIndicator(type)}
            style={{
              padding: "2px 8px",
              fontSize: "11px",
              border: "1px solid #ddd",
              borderRadius: "3px",
              background: indicator === type ? "#d81e06" : "transparent",
              color: indicator === type ? "#fff" : "#555",
              cursor: "pointer",
              fontFamily: "inherit",
            }}
          >
            {label}
          </button>
        ))}
      </div>
    </div>
  );
}
