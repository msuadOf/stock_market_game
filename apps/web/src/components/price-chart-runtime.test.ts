import assert from "node:assert/strict";
import test from "node:test";
import type { createChart } from "lightweight-charts";
import { PriceChartRuntime, type PriceChartRuntimePorts } from "./price-chart-runtime.ts";
import { buildPriceChartIndicatorSource } from "./price-chart-indicators.ts";
import type { IndicatorResults } from "./indicator-results.ts";

function fixture(indicator = true) {
  const calls: string[] = [];
  const series: { id: string; data: unknown; options: unknown }[] = [];
  const charts: { widths: number[]; removed: number; fits: number }[] = [];
  let observerCallback: (() => void) | undefined;
  let windowCallback: (() => void) | undefined;
  let observed: readonly Element[] = [];
  let failedRemoval: string | null = null;
  const main = { clientWidth: 320 } as HTMLDivElement;
  const secondary = indicator ? { clientWidth: 180 } as HTMLDivElement : null;
  const ports: PriceChartRuntimePorts = {
    createChart: ((_container: string | HTMLElement, options?: Parameters<typeof createChart>[1]) => {
      const index = charts.length;
      calls.push(`create:${index}`);
      const chart = { widths: [options?.width as number], removed: 0, fits: 0 };
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
        applyOptions: ({ width }: { width: number }) => { chart.widths.push(width); },
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
const candles = [1, 2, 3].map((time) => ({ time: time as import("lightweight-charts").UTCTimestamp, open: 10, high: 12, low: 9, close: 11, volume: 100 }));
const values = [1, 2];
const results: IndicatorResults = { macd: { dif: values, dea: values, histogram: values }, priceKdj: { k: values, d: values, j: values }, candleKdj: { k: [], d: [], j: [] } };

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
