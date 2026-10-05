import assert from "node:assert/strict";
import test from "node:test";
import type { createChart } from "lightweight-charts";
import { PriceChartRuntime, type PriceChartRuntimePorts } from "./price-chart-runtime.ts";
import { buildPriceChartIndicatorSource } from "./price-chart-indicators.ts";
import type { IndicatorResults } from "./indicator-results.ts";
import { KLINE_MOVING_AVERAGES, klineMovingAverage } from "./kline-moving-averages.ts";

function fixture(indicator = true) {
  const calls: string[] = [];
  const series: { id: string; data: unknown; options: unknown }[] = [];
  const charts: { widths: number[]; heights: number[]; removed: number; fits: number; attributionLogo: boolean | undefined }[] = [];
  let observerCallback: (() => void) | undefined;
  let windowCallback: (() => void) | undefined;
  let observed: readonly Element[] = [];
  let failedRemoval: string | null = null;
  const main = { clientWidth: 320, clientHeight: 180 } as HTMLDivElement;
  const secondary = indicator ? { clientWidth: 180, clientHeight: 60 } as HTMLDivElement : null;
  const ports: PriceChartRuntimePorts = {
    createChart: ((_container: string | HTMLElement, options?: Parameters<typeof createChart>[1]) => {
      const index = charts.length;
      calls.push(`create:${index}`);
      const chart = { widths: [options?.width as number], heights: [options?.height as number], removed: 0, fits: 0, attributionLogo: options?.layout?.attributionLogo };
      charts.push(chart);
      return {
        addSeries: (definition: { type: string }, options: unknown) => {
          const state = { id: `${index}:${definition.type}:${series.length}`, data: undefined as unknown, options };
          series.push(state);
          calls.push(`add:${state.id}`);
          return { id: state.id, setData: (data: unknown) => { state.data = data; }, applyOptions: (next: unknown) => { state.options = next; }, priceScale: () => ({ applyOptions: () => {} }) };
        },
        removeSeries: (series: { id: string }) => {
          calls.push(`removeSeries:${series.id}`);
          if (series.id === failedRemoval) throw new Error("series 删除失败");
        },
        applyOptions: ({ width, height }: { width: number; height: number }) => { chart.widths.push(width); chart.heights.push(height); },
        timeScale: () => ({ fitContent: () => { chart.fits += 1; } }),
        remove: () => { chart.removed += 1; calls.push(`remove:${index}`); },
      };
    }) as unknown as typeof createChart,
    observeContainers: (elements, callback) => { observed = elements; observerCallback = callback; return () => { calls.push("disconnect"); }; },
    resizeEvents: {
      addEventListener: (_name: "resize", callback: () => void) => { windowCallback = callback; calls.push("listen"); },
      removeEventListener: (_name: "resize", callback: () => void) => { assert.equal(callback, windowCallback); calls.push("unlisten"); },
    },
  };
  const runtime = PriceChartRuntime.create({ main, indicator: secondary }, ports);
  assert.ok(runtime);
  return { runtime, calls, series, charts, main, secondary, observed: () => observed, resizeObserver: () => observerCallback!(), resizeWindow: () => windowCallback!(), ports,
    failRemovalOn: (id: string | null) => { failedRemoval = id; } };
}

const points = [{ time: 0, value: 10, volume: 100, buy: true }, { time: 1, value: 12, volume: 200, buy: true }];
const candles = [1, 2, 3].map((day) => ({ time: (1893456000 + day * 86400) as import("lightweight-charts").UTCTimestamp, open: 10, high: 12, low: 9, close: 11, volume: 100 }));
const values = [1, 2];
const results: IndicatorResults = { macd: { dif: values, dea: values, histogram: values }, priceKdj: { k: values, d: values, j: values }, candleKdj: { k: [], d: [], j: [] } };

test("可编辑MA覆盖曲线复用period句柄，隐藏/删除和切分时清除旧线", { timeout: 10000 }, () => {
  const f = fixture();
  f.runtime.updateMovingAverageOverlays([{ period: 5, color: "#926b00", points: [{ time: 1, value: 10.5 }] }, { period: 60, color: "#28734b", points: [] }]);
  assert.deepEqual(f.series[3].data, [{ time: 1, value: 10.5 }]);
  assert.deepEqual(f.series[4].data, []);
  f.runtime.updateMovingAverageOverlays([{ period: 60, color: "#335eae", points: [{ time: 2, value: 12 }] }]);
  assert.ok(f.calls.includes(`removeSeries:${f.series[3].id}`));
  assert.equal(f.series.length, 5);
  assert.deepEqual(f.series[4].data, [{ time: 2, value: 12 }]);
  f.runtime.updateMovingAverageOverlays([]);
  assert.ok(f.calls.includes(`removeSeries:${f.series[4].id}`));
  f.runtime.dispose();
});

test("分时均价曲线覆盖主价格图且清空时不替代价格点", { timeout: 10000 }, () => {
  const f = fixture();
  f.runtime.updateAverageOverlay([{ time: 1, value: 10.25 }, { time: 2, value: 10.5 }]);
  assert.deepEqual(f.series[3].data, [{ time: 1, value: 10.25 }, { time: 2, value: 10.5 }]);
  f.runtime.updateAverageOverlay([]);
  assert.deepEqual(f.series[3].data, []);
  f.runtime.dispose();
});

test("创建、独立 resize 与 cleanup 保持注册和释放顺序", { timeout: 10000 }, () => {
  const f = fixture();
  assert.equal(f.charts.length, 2);
  assert.deepEqual(f.observed(), [f.main, f.secondary]);
  Object.defineProperty(f.main, "clientWidth", { value: 640 });
  Object.defineProperty(f.secondary!, "clientWidth", { value: 240 });
  f.resizeObserver();
  f.resizeWindow();
  assert.deepEqual(f.charts[0].widths, [320, 640, 640]);
  assert.deepEqual(f.charts[1].widths, [180, 240, 240]);
  f.runtime.dispose();
  assert.deepEqual(f.calls.slice(-4), ["disconnect", "unlisten", "remove:0", "remove:1"]);
  assert.deepEqual(f.charts.map((chart) => chart.removed), [1, 1]);
  const second = fixture();
  second.runtime.dispose();
  assert.deepEqual(second.charts.map((chart) => chart.removed), [1, 1]);
});

test("分时与权威日 K 切换清旧数据，窗口只裁剪输入日 K", { timeout: 10000 }, () => {
  const f = fixture();
  f.runtime.updatePrice(points, candles, 10, "分时", 2);
  assert.deepEqual(f.series[0].data, [{ time: 0, value: 10 }, { time: 1, value: 12 }]);
  assert.deepEqual(f.series[0].options, { color: "#d81e06" });
  f.runtime.updatePrice(points, candles, 10, "日K", 2);
  assert.deepEqual(f.series[0].data, []);
  assert.deepEqual(f.series[1].data, candles.slice(-2));
  assert.deepEqual(f.series[0].options, { visible: false });
  f.runtime.updatePrice([], [], 10, "分时", 2);
  assert.deepEqual(f.series[0].data, []);
  assert.deepEqual(f.series[1].data, []);
  assert.equal(f.charts[0].fits, 2);
  f.runtime.dispose();
});

test("指标切换删除前组 series，pending 与 error 清空当前结果", { timeout: 10000 }, () => {
  const f = fixture();
  const source = buildPriceChartIndicatorSource("分时", points, []);
  f.runtime.updateIndicator("volume", source, { kind: "idle" }, 20);
  assert.equal((f.series[2].data as unknown[]).length, 2);
  f.runtime.updateIndicator("macd", source, { kind: "pending" }, 20);
  assert.ok(f.calls.includes(`removeSeries:${f.series[2].id}`));
  assert.equal(f.series.length, 3);
  f.runtime.updateIndicator("macd", source, { kind: "ready", value: results }, 20);
  assert.equal(f.series.length, 6);
  f.runtime.updateIndicator("macd", source, { kind: "error", message: "测试错误" }, 20);
  assert.deepEqual(f.series.slice(3).map((series) => series.data), [[], [], []]);
  f.runtime.updateIndicator("kdj", source, { kind: "ready", value: results }, 20);
  for (const series of f.series.slice(3, 6)) assert.ok(f.calls.includes(`removeSeries:${series.id}`));
  f.runtime.updateIndicator("kdj", source, { kind: "pending" }, 20);
  assert.deepEqual(f.series.slice(6).map((series) => series.data), [[], [], []]);
  f.runtime.updateIndicator("none", source, { kind: "idle" }, 20);
  for (const series of f.series.slice(6)) assert.ok(f.calls.includes(`removeSeries:${series.id}`));
  f.runtime.updateIndicator("volume", source, { kind: "idle" }, 20);
  assert.equal(f.series.length, 10);
  assert.equal((f.series[9].data as unknown[]).length, 2);
  f.runtime.dispose();
});

test("缺主容器不创建，缺副容器保留主图，createChart 错误显式抛出", { timeout: 10000 }, () => {
  const f = fixture(false);
  assert.equal(f.charts.length, 1);
  assert.equal(PriceChartRuntime.create({ main: null, indicator: null }, f.ports), null);
  assert.equal(f.charts.length, 1);
  assert.throws(() => PriceChartRuntime.create({ main: f.main, indicator: null }, { ...f.ports, createChart: () => { throw new Error("创建失败"); } }), /创建失败/);
  f.runtime.dispose();
});

test("MACD 与 KDJ 逐项删除失败后重试不重复删除已释放句柄", { timeout: 10000 }, () => {
  for (const indicator of ["macd", "kdj"] as const) {
    const f = fixture();
    const source = buildPriceChartIndicatorSource("分时", points, []);
    f.runtime.updateIndicator(indicator, source, { kind: "ready", value: results }, 20);
    const order = indicator === "macd" ? [f.series[5], f.series[3], f.series[4]] : f.series.slice(3);
    f.failRemovalOn(order[1].id);
    assert.throws(() => f.runtime.updateIndicator("none", source, { kind: "idle" }, 20), /series 删除失败/);
    f.failRemovalOn(null);
    f.runtime.updateIndicator("none", source, { kind: "idle" }, 20);
    const removals = (id: string) => f.calls.filter((call) => call === `removeSeries:${id}`).length;
    assert.equal(removals(order[0].id), 1, "成功删除的句柄必须立即从 owner 清除");
    assert.equal(removals(order[1].id), 2);
    assert.equal(removals(order[2].id), 1);
    f.runtime.dispose();
  }
});


test("从预览进入个股后图表随容器高度扩展，隐藏面板不破坏图表尺寸", { timeout: 10000 }, () => {
  const f = fixture();
  Object.defineProperty(f.main, "clientHeight", { value: 420, configurable: true });
  Object.defineProperty(f.secondary!, "clientHeight", { value: 100 });
  f.resizeObserver();
  assert.equal(f.charts[0].heights.at(-1), 420);
  assert.equal(f.charts[1].heights.at(-1), 100);
  Object.defineProperty(f.main, "clientHeight", { value: 0 });
  f.resizeObserver();
  assert.equal(f.charts[0].heights.at(-1), 420);
  f.runtime.dispose();
});

test("日 K 预览放大后重新适配可见窗口，主副图同时适配", { timeout: 10000 }, () => {
  const f = fixture();
  f.runtime.updatePrice([], candles, 10, "日K", 20);
  f.runtime.updateIndicator("volume", buildPriceChartIndicatorSource("日K", [], candles), { kind: "idle" }, 20);
  const before = f.charts.map(chart => chart.fits);
  Object.defineProperty(f.main, "clientWidth", { value: 600 });
  Object.defineProperty(f.secondary!, "clientWidth", { value: 600 });
  f.resizeObserver();
  assert.deepEqual(f.charts.map(chart => chart.fits), before.map(count => count + 1));
  f.runtime.dispose();
});


test("主图与指标副图都关闭覆盖绘图区的品牌标志", { timeout: 10000 }, () => {
  const f = fixture();
  assert.deepEqual(f.charts.map(chart => chart.attributionLogo), [false, false]);
  f.runtime.dispose();
});

test("均线可多选且按完整交易日收盘价计算，切换不改K线数据或窗口", { timeout: 10000 }, () => {
  const f = fixture();
  const daily = Array.from({ length: 61 }, (_, index) => ({ ...candles[0], time: index as import("lightweight-charts").UTCTimestamp, close: index + 1, rawPrices: { open: "1000", high: "6200", low: "100", close: String((index + 1) * 100) } }));
  const plot = (input: typeof daily, periods: readonly number[]) => periods.map((period) => ({ period, color: KLINE_MOVING_AVERAGES.find((item) => item.days === period)!.color, points: klineMovingAverage(input, period, 360) }));
  f.runtime.updatePrice([], daily, 10, "日K", 360);
  const before = f.charts[0].fits;
  f.runtime.updateMovingAverageOverlays(plot(daily, [20, 60]));
  assert.equal(f.series.length, 5);
  assert.deepEqual((f.series[3].data as unknown[])[0], { time: 19, value: 10.5 });
  assert.deepEqual((f.series[4].data as unknown[])[0], { time: 59, value: 30.5 });
  assert.equal(f.charts[0].fits, before);
  assert.deepEqual(f.series[1].data, daily);
  f.runtime.updateMovingAverageOverlays(plot(daily, [60]));
  assert.ok(f.calls.includes(`removeSeries:${f.series[3].id}`));
  f.runtime.updateMovingAverageOverlays(plot(daily.slice(0, 10), [60]));
  assert.deepEqual(f.series[4].data, []);
  f.runtime.updateMovingAverageOverlays([]);
  assert.ok(f.calls.includes(`removeSeries:${f.series[4].id}`));
  f.runtime.dispose();
});

 test("均线采用参考图的五个周期及配色", { timeout: 10000 }, async () => {
  const { KLINE_MOVING_AVERAGES } = await import("./kline-moving-averages.ts");
  assert.deepEqual(KLINE_MOVING_AVERAGES.map(item => item.days), [5, 10, 20, 30, 60]);
  assert.deepEqual(KLINE_MOVING_AVERAGES.map(item => item.color), ["#ff8a00", "#1890ff", "#c82bb5", "#00bfd8", "#ff6500"]);
});
