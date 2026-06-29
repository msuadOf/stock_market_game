/**
 * 分时价格图（TradingView Lightweight Charts v5）。
 * 显示选中股票的实时价格走势线，红涨绿跌。
 */
import { useEffect, useRef } from "react";
import { createChart, LineSeries, ColorType, type UTCTimestamp } from "lightweight-charts";

export interface PricePoint {
  time: number;
  value: number; // 元（yuan）
}

interface Props {
  data: PricePoint[];
  lastClose: number; // 昨收（元），用于着色基准
}

export function PriceChart({ data, lastClose }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const chartRef = useRef<ReturnType<typeof createChart> | null>(null);
  const seriesRef = useRef<ReturnType<ReturnType<typeof createChart>["addSeries"]> | null>(null);

  // 创建图表（仅挂载时）
  useEffect(() => {
    if (!containerRef.current) return;
    const chart = createChart(containerRef.current, {
      width: containerRef.current.clientWidth,
      height: 220,
      layout: {
        background: { type: ColorType.Solid, color: "#ffffff" },
        textColor: "#222",
        fontFamily: '"Microsoft YaHei", sans-serif',
      },
      grid: {
        vertLines: { color: "#f0f0f0" },
        horzLines: { color: "#f0f0f0" },
      },
      rightPriceScale: { borderColor: "#ddd" },
      timeScale: { borderColor: "#ddd", timeVisible: false },
    });
    const series = chart.addSeries(LineSeries, {
      color: "#d81e06",
      lineWidth: 2,
      priceFormat: { type: "price", precision: 2, minMove: 0.01 },
    });
    chartRef.current = chart;
    seriesRef.current = series;

    const handleResize = () => {
      if (containerRef.current) chart.applyOptions({ width: containerRef.current.clientWidth });
    };
    window.addEventListener("resize", handleResize);
    return () => {
      window.removeEventListener("resize", handleResize);
      chart.remove();
      chartRef.current = null;
      seriesRef.current = null;
    };
  }, []);

  // 数据更新 → 重绘
  useEffect(() => {
    if (seriesRef.current && data.length > 0) {
      // 颜色：最新价 vs 昨收
      const lastVal = data[data.length - 1].value;
      const color = lastVal > lastClose ? "#d81e06" : lastVal < lastClose ? "#009944" : "#b8b8b8";
      seriesRef.current.applyOptions({ color });
      seriesRef.current.setData(data.map((d) => ({ time: d.time as UTCTimestamp, value: d.value })));
      chartRef.current?.timeScale().fitContent();
    }
  }, [data, lastClose]);

  return <div ref={containerRef} style={{ width: "100%", height: 220 }} />;
}
