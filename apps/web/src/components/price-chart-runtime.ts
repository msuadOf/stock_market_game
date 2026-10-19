import {
  createChart, LineSeries, CandlestickSeries, HistogramSeries, ColorType, CrosshairMode,
  type UTCTimestamp, type IChartApi, type ISeriesApi,
} from "lightweight-charts";
import type { KlinePoint, PricePoint } from "./PriceChart.tsx";
import { formatLotAmount } from "../utils/format.ts";
import { observeChartContainers } from "./chart-resize.ts";
import { type buildPriceChartIndicatorSource, priceChartIndicatorData, priceChartVolumeData } from "./price-chart-indicators.ts";
import type { IndicatorResultState } from "./useIndicatorResults.ts";

export type PriceChartIndicator = "none" | "volume" | "macd" | "kdj" | "intradayAverage";
interface ChartContainers { readonly main: HTMLDivElement | null; readonly indicator: HTMLDivElement | null }
export interface PriceChartRuntimePorts {
  readonly createChart: typeof createChart;
  readonly observeContainers: typeof observeChartContainers;
  readonly resizeEvents: {
    addEventListener(type: "resize", listener: () => void): void;
    removeEventListener(type: "resize", listener: () => void): void;
  };
}

/** 仅持有 Lightweight Charts 句柄及 resize 资源；输入行情仍由调用方拥有。 */
export class PriceChartRuntime {
  private chart: IChartApi | null = null;
  private indicatorChart: IChartApi | null = null;
  private priceSeries: ISeriesApi<"Line"> | null = null;
  private averageSeries: ISeriesApi<"Line"> | null = null;
  private movingAverageSeries = new Map<number, ISeriesApi<"Line">>();
  private candleSeries: ISeriesApi<"Candlestick"> | null = null;
  private volumeSeries: ISeriesApi<"Histogram"> | null = null;
  private macdHistogram: ISeriesApi<"Histogram"> | null = null;
  private macdDif: ISeriesApi<"Line"> | null = null;
  private macdDea: ISeriesApi<"Line"> | null = null;
  private kdjK: ISeriesApi<"Line"> | null = null;
  private kdjD: ISeriesApi<"Line"> | null = null;
  private kdjJ: ISeriesApi<"Line"> | null = null;
  private disconnectResizeObserver: (() => void) | null = null;
  private readonly handleResize = () => this.resize();

  static create(containers: ChartContainers, ports?: PriceChartRuntimePorts): PriceChartRuntime | null {
    const main = containers.main;
    if (main === null) return null;
    return new PriceChartRuntime({ main, indicator: containers.indicator }, ports ?? {
      createChart, observeContainers: observeChartContainers, resizeEvents: window,
    });
  }

  private readonly containers: ChartContainers & { readonly main: HTMLDivElement };
  private readonly ports: PriceChartRuntimePorts;

  private constructor(containers: ChartContainers & { readonly main: HTMLDivElement }, ports: PriceChartRuntimePorts) {
    this.containers = containers;
    this.ports = ports;
    const chart = ports.createChart(containers.main, {
      width: containers.main.clientWidth,
      height: 180,
      layout: {
        attributionLogo: false,
        background: { type: ColorType.Solid, color: "transparent" },
        textColor: "#222",
        fontFamily: '"Microsoft YaHei", sans-serif',
        fontSize: 11,
      },
      grid: { vertLines: { color: "rgba(0,0,0,0.04)" }, horzLines: { color: "rgba(0,0,0,0.04)" } },
      rightPriceScale: { borderColor: "#ddd", minimumWidth: 64 },
      // 游戏分时图只表达相对行情，不展示真实日历；否则内部序号会被渲染为 1970 年日期。
      timeScale: { visible: false },
      crosshair: { mode: CrosshairMode.Normal },
    });
    const priceSeries = chart.addSeries(LineSeries, {
      color: "#d81e06",
      lineWidth: 2,
      priceFormat: { type: "price", precision: 2, minMove: 0.01 },
    });
    this.chart = chart;
    this.priceSeries = priceSeries;

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
    this.candleSeries = candleSeries;

    // 量能副图（默认显示）
    if (containers.indicator) {
      const volChart = ports.createChart(containers.indicator, {
        width: containers.indicator.clientWidth,
        height: 60,
        layout: {
        attributionLogo: false,
          background: { type: ColorType.Solid, color: "transparent" },
          textColor: "#888",
          fontFamily: '"Microsoft YaHei", sans-serif',
          fontSize: 10,
        },
        grid: { vertLines: { visible: false }, horzLines: { visible: false } },
        rightPriceScale: { visible: true, minimumWidth: 64 },
        timeScale: { visible: false },
      });
      this.indicatorChart = volChart;
      const volSeries = volChart.addSeries(HistogramSeries, {
        priceFormat: { type: "custom", minMove: 0.01, formatter: (value: number) => `${formatLotAmount(value)}手` },
        priceScaleId: "right",
      });
      volSeries.priceScale().applyOptions({ scaleMargins: { top: 0.2, bottom: 0 } });
      this.volumeSeries = volSeries;
    }

    this.disconnectResizeObserver = ports.observeContainers(
      [containers.main, containers.indicator].filter((element): element is HTMLDivElement => element !== null),
      this.handleResize,
    );
    ports.resizeEvents.addEventListener("resize", this.handleResize);
  }

  resize(): void {
    const { main, indicator } = this.containers;
    // display:none 的其他工作区暂时没有尺寸，保留图表直到重新显示。
    if (main.clientWidth > 0 && main.clientHeight > 0) {
      this.chart?.applyOptions({ width: main.clientWidth, height: main.clientHeight });
      this.chart?.timeScale().fitContent();
    }
    if (indicator !== null && indicator.clientWidth > 0 && indicator.clientHeight > 0) {
      this.indicatorChart?.applyOptions({ width: indicator.clientWidth, height: indicator.clientHeight });
      this.indicatorChart?.timeScale().fitContent();
    }
  }

  updatePrice(data: readonly PricePoint[], dailyCandles: readonly KlinePoint[] | undefined, lastClose: number, chartType: "分时" | "日K", klineDays: number): void {
    if (chartType === "日K") {
      // 显示蜡烛图、隐藏分时线
      this.priceSeries?.setData([]);
      this.priceSeries?.applyOptions({ visible: false });
      this.candleSeries?.applyOptions({ visible: true });

      // 日 K 只使用引擎权威 OHLC，不从当日分时缓存合成历史。
      if (this.candleSeries) {
        const candles = dailyCandles ?? [];
        const visible = candles.slice(-Math.max(1, klineDays));
        // 每次完整写入可正确处理：盘中蜡烛更新、跨日新增、以及 20/60/120/240/360 窗口切换。
        // 最多 360 根，远低于图表库的性能阈值，可靠性比只更新最后一根更重要。
        this.candleSeries.setData(visible);
        this.chart?.timeScale().fitContent();
      }
    } else {
      // 分时模式：显示折线、隐藏蜡烛图
      this.candleSeries?.setData([]);
      this.priceSeries?.applyOptions({ visible: true });
      this.candleSeries?.applyOptions({ visible: false });

      if (this.priceSeries) {
        if (data.length === 0) {
          this.priceSeries.setData([]);
          return;
        }
        const lastVal = data[data.length - 1].value;
        const color = lastVal > lastClose ? "#d81e06" : lastVal < lastClose ? "#009944" : "#b8b8b8";
        this.priceSeries.applyOptions({ color });

        this.priceSeries.setData(data.map((d) => ({ time: d.time as UTCTimestamp, value: d.value })));
        this.chart?.timeScale().fitContent();
      }
    }
  }

  updateAverageOverlay(points: readonly { readonly time: number; readonly value: number }[]): void {
    if (this.chart === null) return;
    if (this.averageSeries === null) {
      this.averageSeries = this.chart.addSeries(LineSeries, {
        color: "#e6a400",
        lineWidth: 1,
        priceFormat: { type: "price", precision: 2, minMove: 0.01 },
      });
    }
    this.averageSeries.setData(points.map((point) => ({ time: point.time as UTCTimestamp, value: point.value })));
  }

  updateMovingAverageOverlays(lines: readonly { readonly period: number; readonly color: string; readonly points: readonly { readonly time: number; readonly value: number }[] }[]): void {
    if (this.chart === null) return;
    const requested = new Set(lines.map((line) => line.period));
    for (const [period, series] of this.movingAverageSeries) {
      if (!requested.has(period)) { this.chart.removeSeries(series); this.movingAverageSeries.delete(period); }
    }
    for (const line of lines) {
      let series = this.movingAverageSeries.get(line.period);
      if (series === undefined) {
        series = this.chart.addSeries(LineSeries, { color: line.color, lineWidth: 1, title: `MA${line.period}`, priceFormat: { type: "price", precision: 2, minMove: 0.01 } });
        this.movingAverageSeries.set(line.period, series);
      }
      series.applyOptions({ color: line.color });
      series.setData(line.points.map((point) => ({ time: point.time as UTCTimestamp, value: point.value })));
    }
  }

  updateIndicator(indicator: PriceChartIndicator, indicatorSource: ReturnType<typeof buildPriceChartIndicatorSource>, indicatorResult: IndicatorResultState, klineDays: number): void {
    if (!this.indicatorChart) return;
    const chart = this.indicatorChart;

    // 清除旧 series（切换指标时）
    if (indicator !== "volume" && this.volumeSeries) {
      chart.removeSeries(this.volumeSeries);
      this.volumeSeries = null;
    }
    if (indicator !== "macd") {
      for (const field of ["macdHistogram", "macdDif", "macdDea"] as const) this.removeIndicatorSeries(chart, field);
    }
    if (indicator !== "kdj") {
      for (const field of ["kdjK", "kdjD", "kdjJ"] as const) this.removeIndicatorSeries(chart, field);
    }

    if (indicator === "volume") {
      if (!this.volumeSeries) {
        const s = chart.addSeries(HistogramSeries, { priceFormat: { type: "custom", minMove: 0.01, formatter: (value: number) => `${formatLotAmount(value)}手` }, priceScaleId: "right" });
        s.priceScale().applyOptions({ scaleMargins: { top: 0.2, bottom: 0 } });
        this.volumeSeries = s;
      }
      this.volumeSeries.setData(priceChartVolumeData(indicatorSource, klineDays));
    } else if (indicator === "macd") {
      if (indicatorResult.kind !== "ready") {
        [this.macdHistogram, this.macdDif, this.macdDea].forEach((series) => series?.setData([]));
        return;
      }
      const plotted = priceChartIndicatorData(indicatorSource, indicatorResult.value, klineDays);
      if (!this.macdDif) {
        this.macdDif = chart.addSeries(LineSeries, { color: "#d85b73", lineWidth: 1, priceScaleId: "right" });
        this.macdDea = chart.addSeries(LineSeries, { color: "#6ca6e8", lineWidth: 1, priceScaleId: "right" });
        this.macdHistogram = chart.addSeries(HistogramSeries, { priceScaleId: "right" });
        this.macdHistogram.priceScale().applyOptions({ scaleMargins: { top: 0.3, bottom: 0.1 } });
      }
      this.macdDif!.setData(plotted.dif);
      this.macdDea!.setData(plotted.dea);
      this.macdHistogram!.setData(plotted.histogram);
    } else if (indicator === "kdj") {
      if (indicatorResult.kind !== "ready") {
        [this.kdjK, this.kdjD, this.kdjJ].forEach((series) => series?.setData([]));
        return;
      }
      const plotted = priceChartIndicatorData(indicatorSource, indicatorResult.value, klineDays);
      if (!this.kdjK) {
        this.kdjK = chart.addSeries(LineSeries, { color: "#e6a400", lineWidth: 1, priceScaleId: "right" });
        this.kdjD = chart.addSeries(LineSeries, { color: "#c56ae6", lineWidth: 1, priceScaleId: "right" });
        this.kdjJ = chart.addSeries(LineSeries, { color: "#4ea15f", lineWidth: 1, priceScaleId: "right" });
        this.kdjK.priceScale().applyOptions({ scaleMargins: { top: 0.1, bottom: 0.1 } });
      }
      this.kdjK!.setData(plotted.k);
      this.kdjD!.setData(plotted.d);
      this.kdjJ!.setData(plotted.j);
    }
    chart.timeScale().fitContent();
  }

  private removeIndicatorSeries(chart: IChartApi, field: "macdHistogram" | "macdDif" | "macdDea" | "kdjK" | "kdjD" | "kdjJ"): void {
    const series = this[field];
    if (series === null) return;
    chart.removeSeries(series);
    // 每个成功释放的句柄立即清除；后续删除抛错时保留原先的部分更新语义。
    this[field] = null;
  }

  dispose(): void {
    this.disconnectResizeObserver?.();
    this.disconnectResizeObserver = null;
    this.ports.resizeEvents.removeEventListener("resize", this.handleResize);
    this.chart?.remove();
    this.indicatorChart?.remove();
    this.chart = this.indicatorChart = null;
    this.priceSeries = null;
    this.movingAverageSeries.clear();
    this.candleSeries = null;
    this.volumeSeries = this.macdHistogram = null;
    this.macdDif = this.macdDea = this.kdjK = this.kdjD = this.kdjJ = null;
  }
}
