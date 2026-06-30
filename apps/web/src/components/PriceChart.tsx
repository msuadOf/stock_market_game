/**
 * 分时价格图（TradingView Lightweight Charts v5）。
 * 显示选中股票的实时价格走势线 + 量能柱 + MACD/KDJ 可选指标 pane。
 */
import { useEffect, useRef, useState } from "react";
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

export interface PricePoint {
  time: number;
  value: number; // 元（yuan）
  volume?: number;
  buy?: boolean;
}

export interface KlinePoint {
  time: number;
  open: number;
  high: number;
  low: number;
  close: number;
}

interface Props {
  data: PricePoint[];
  lastClose: number; // 昨收（元），用于着色基准
  chartType?: "分时" | "日K";
  klineDays?: number; // 日K 显示天数（5/10/20/30/60）
}

/** MACD 指标计算（12/26/9 参数）。 */
function calcMACD(data: PricePoint[]): { macd: { time: UTCTimestamp; value: number }[]; signal: { time: UTCTimestamp; value: number }[]; hist: { time: UTCTimestamp; value: number; color?: string }[] } {
  const prices = data.map((d) => d.value);
  const ema = (period: number) => {
    const k = 2 / (period + 1);
    const result: number[] = [];
    let prev = prices[0] ?? 0;
    for (let i = 0; i < prices.length; i++) {
      prev = i === 0 ? prices[0] : prices[i] * k + prev * (1 - k);
      result.push(prev);
    }
    return result;
  };
  const ema12 = ema(12);
  const ema26 = ema(26);
  const dif = ema12.map((v, i) => v - ema26[i]);
  const k9 = 2 / 10;
  let prevDea = dif[0] ?? 0;
  const dea: number[] = [];
  for (let i = 0; i < dif.length; i++) {
    prevDea = i === 0 ? dif[0] : dif[i] * k9 + prevDea * (1 - k9);
    dea.push(prevDea);
  }
  const macd = dif.map((v, i) => ({ time: data[i].time as UTCTimestamp, value: v }));
  const signal = dea.map((v, i) => ({ time: data[i].time as UTCTimestamp, value: v }));
  const hist = dif.map((v, i) => ({
    time: data[i].time as UTCTimestamp,
    value: (v - dea[i]) * 2,
    color: v - dea[i] >= 0 ? "#d81e06" : "#009944",
  }));
  return { macd, signal, hist };
}

/** KDJ 指标计算（9/3/3 参数，简化版 — 用价格序列代替最高最低价）。 */
function calcKDJ(data: PricePoint[]): { k: { time: UTCTimestamp; value: number }[]; d: { time: UTCTimestamp; value: number }[]; j: { time: UTCTimestamp; value: number }[] } {
  const prices = data.map((d) => d.value);
  const kArr: number[] = [];
  const dArr: number[] = [];
  let prevK = 50;
  let prevD = 50;
  for (let i = 0; i < prices.length; i++) {
    const lookback = prices.slice(Math.max(0, i - 8), i + 1);
    const hv = Math.max(...lookback);
    const lv = Math.min(...lookback);
    const rsv = hv !== lv ? ((prices[i] - lv) / (hv - lv)) * 100 : 50;
    prevK = (2 / 3) * prevK + (1 / 3) * rsv;
    prevD = (2 / 3) * prevD + (1 / 3) * prevK;
    kArr.push(prevK);
    dArr.push(prevD);
  }
  const k = kArr.map((v, i) => ({ time: data[i].time as UTCTimestamp, value: v }));
  const d = dArr.map((v, i) => ({ time: data[i].time as UTCTimestamp, value: v }));
  const j = kArr.map((v, i) => ({ time: data[i].time as UTCTimestamp, value: 3 * v - 2 * dArr[i] }));
  return { k, d, j };
}

type IndicatorType = "none" | "volume" | "macd" | "kdj";

/**
 * 从分时 PricePoint 序列合成日 K 蜡烛数据。
 * 按「固定窗口」（每 N 个 tick 一根蜡烛）分组 OHLC，适配无限分时数据。
 */
function buildDailyCandles(
  data: PricePoint[],
): { time: UTCTimestamp; open: number; high: number; low: number; close: number }[] {
  if (data.length === 0) return [];
  // 每 ticksPerCandle 个 tick 合一根蜡烛（日K：20 个 tick = 1 根）
  const ticksPerCandle = 20;
  const candles: { time: UTCTimestamp; open: number; high: number; low: number; close: number }[] = [];
  for (let i = 0; i < data.length; i += ticksPerCandle) {
    const slice = data.slice(i, i + ticksPerCandle);
    const open = slice[0].value;
    const close = slice[slice.length - 1].value;
    const high = Math.max(...slice.map((d) => d.value));
    const low = Math.min(...slice.map((d) => d.value));
    candles.push({
      time: slice[0].time as UTCTimestamp,
      open,
      high,
      low,
      close,
    });
  }
  return candles;
}

export function PriceChart({ data, lastClose, chartType = "分时", klineDays = 20 }: Props) {
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
      timeScale: { borderColor: "#ddd", timeVisible: false },
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
        priceFormat: { type: "volume" },
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
    window.addEventListener("resize", handleResize);
    return () => {
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

  // 数据更新 → 主图增量更新（O(1) update 而非 O(n) setData）
  const lastDataLenRef = useRef(0);
  const lastChartTypeRef = useRef(chartType);

  useEffect(() => {
    if (data.length === 0) return;

    if (chartType === "日K") {
      // 显示蜡烛图、隐藏分时线
      priceSeriesRef.current?.applyOptions({ visible: false });
      candleSeriesRef.current?.applyOptions({ visible: true });

      // 从 PricePoint 合成 K 线（按 time 分组 OHLC）
      // 日K 模式：每个交易日一根蜡烛，用当天所有 tick 的 min/max/open/close
      if (candleSeriesRef.current) {
        const candles = buildDailyCandles(data);
        const visible = candles.slice(-Math.max(1, klineDays));
        if (lastChartTypeRef.current !== "日K" || data.length < lastDataLenRef.current) {
          // 切换到日K 或数据重置 → 全量 setData
          candleSeriesRef.current.setData(visible);
          chartRef.current?.timeScale().fitContent();
        } else {
          // 增量更新最后一根蜡烛
          const last = visible[visible.length - 1];
          if (last) candleSeriesRef.current.update(last);
        }
      }
    } else {
      // 分时模式：显示折线、隐藏蜡烛图
      priceSeriesRef.current?.applyOptions({ visible: true });
      candleSeriesRef.current?.applyOptions({ visible: false });

      if (priceSeriesRef.current) {
        const lastVal = data[data.length - 1].value;
        const color = lastVal > lastClose ? "#d81e06" : lastVal < lastClose ? "#009944" : "#b8b8b8";
        priceSeriesRef.current.applyOptions({ color });

        if (lastChartTypeRef.current !== "分时" || lastDataLenRef.current === 0 || data.length < lastDataLenRef.current) {
          priceSeriesRef.current.setData(data.map((d) => ({ time: d.time as UTCTimestamp, value: d.value })));
          chartRef.current?.timeScale().fitContent();
        } else {
          const last = data[data.length - 1];
          priceSeriesRef.current.update({ time: last.time as UTCTimestamp, value: last.value });
        }
      }
    }

    lastDataLenRef.current = data.length;
    lastChartTypeRef.current = chartType;
  }, [data, lastClose, chartType, klineDays]);

  // 副图数据更新
  useEffect(() => {
    if (!indicatorChartRef.current || data.length === 0) return;
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
        const s = chart.addSeries(HistogramSeries, { priceFormat: { type: "volume" }, priceScaleId: "" });
        s.priceScale().applyOptions({ scaleMargins: { top: 0.2, bottom: 0 } });
        volSeriesRef.current = s;
      }
      volSeriesRef.current.setData(
        data.map((d) => ({
          time: d.time as UTCTimestamp,
          value: d.volume ?? 0,
          color: d.buy ? "rgba(216,30,6,0.5)" : "rgba(0,153,68,0.5)",
        })),
      );
    } else if (indicator === "macd") {
      const { macd, signal, hist } = calcMACD(data);
      if (!macdDifRef.current) {
        macdDifRef.current = chart.addSeries(LineSeries, { color: "#d85b73", lineWidth: 1, priceScaleId: "" });
        macdDeaRef.current = chart.addSeries(LineSeries, { color: "#6ca6e8", lineWidth: 1, priceScaleId: "" });
        macdHistRef.current = chart.addSeries(HistogramSeries, { priceScaleId: "" });
        macdHistRef.current.priceScale().applyOptions({ scaleMargins: { top: 0.3, bottom: 0.1 } });
      }
      macdDifRef.current!.setData(macd);
      macdDeaRef.current!.setData(signal);
      macdHistRef.current!.setData(hist);
    } else if (indicator === "kdj") {
      const { k, d, j } = calcKDJ(data);
      if (!kdjKRef.current) {
        kdjKRef.current = chart.addSeries(LineSeries, { color: "#e6a400", lineWidth: 1, priceScaleId: "" });
        kdjDRef.current = chart.addSeries(LineSeries, { color: "#c56ae6", lineWidth: 1, priceScaleId: "" });
        kdjJRef.current = chart.addSeries(LineSeries, { color: "#4ea15f", lineWidth: 1, priceScaleId: "" });
        kdjKRef.current.priceScale().applyOptions({ scaleMargins: { top: 0.1, bottom: 0.1 } });
      }
      kdjKRef.current!.setData(k);
      kdjDRef.current!.setData(d);
      kdjJRef.current!.setData(j);
    }
    chart.timeScale().fitContent();
  }, [data, indicator]);

  return (
    <div style={{ width: "100%" }}>
      <div ref={containerRef} style={{ width: "100%", height: 180 }} />
      <div ref={indicatorContainerRef} style={{ width: "100%", height: 60 }} />
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
