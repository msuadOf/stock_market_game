import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import { MobileIntradayProjection, intradayChartX } from "../mobile/market-model.ts";

let vite: ViteDevServer;
let component: typeof import("./DesktopIntradayChart.tsx");
before(async () => {
  vite = await createServer({ root: new URL("../../", import.meta.url).pathname, configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  component = await vite.ssrLoadModule("/src/components/DesktopIntradayChart.tsx") as typeof component;
});
after(async () => { if (vite) await vite.close(); });
const base = { market: { last_close: "1000" }, minutePoints: [], auctionPoints: [], trades: [], elapsedMinutes: 0, totalMinutes: 240, gameDay: 0, gameTick: 0 };
function render(inputs = base) {
  return renderToStaticMarkup(createElement(component.DesktopIntradayChart, { projection: MobileIntradayProjection.fromInputs(inputs), lastClose: "1000", indicator: "volume", result: { kind: "idle" } }));
}
test("空分时展示昨收、全天时间轴及等待状态，不伪造成交曲线", { timeout: 10000 }, () => {
  const html = render();
  for (const text of ["09:15", "09:30", "11:30/13:00", "15:00", "昨收 10.00", "等待行情", "0.00%", "竞价累计量", "分钟成交量"]) assert.ok(html.includes(text), text);
  assert.ok(!html.includes('class="intraday-price-line"'));
});
test("竞价缺价使用参考轴，连续价格与成交量共享固定槽位，未来时段留白", { timeout: 10000 }, () => {
  const inputs = { ...base, auctionPoints: [{ time: 0, value: 11, volume: 200, buy: true }, { time: 1, value: null, volume: 0, buy: false }, { time: 2, value: 9, volume: 100, buy: false }], minutePoints: [{ time: 0, value: 10, volume: 100 }, { time: 29, value: 12, volume: 500 }] };
  const p = MobileIntradayProjection.fromInputs(inputs);
  const html = renderToStaticMarkup(createElement(component.DesktopIntradayChart, { projection: p, lastClose: "1000", indicator: "volume", result: { kind: "idle" } }));
  assert.equal((html.match(/class="intraday-auction-point"/g) || []).length, 2);
  assert.ok(html.includes(`points="16,66.66666666666666 ${intradayChartX({ phase: "continuous", minute: 29 })},0"`));
  assert.ok(html.includes(`x1="${intradayChartX({ phase: "continuous", minute: 29 })}"`));
  assert.ok(intradayChartX({ phase: "continuous", minute: 29 }) < 30);
});

test("小幅 MACD 使用实际数值范围，全零居中且全负可见", { timeout: 10000 }, () => {
  const p = MobileIntradayProjection.fromInputs({ ...base, minutePoints: [{ time: 0, value: 10 }, { time: 1, value: 10.01 }] });
  for (const [values, expected] of [[[0.01, -0.01], [5, 95]], [[0, 0], [50, 50]], [[-0.01, -0.02], [50, 95]]] as const) {
    const array = [...values];
    const html = renderToStaticMarkup(createElement(component.DesktopIntradayChart, { projection: p, lastClose: "1000", indicator: "macd", result: { kind: "ready", value: { macd: { dif: array, dea: array, histogram: array }, priceKdj: { k: array, d: array, j: array }, candleKdj: { k: [], d: [], j: [] } } } }));
    assert.ok(html.includes(`points="${intradayChartX({ phase: "continuous", minute: 0 })},${expected[0]} ${intradayChartX({ phase: "continuous", minute: 1 })},${expected[1]}"`));
  }
});

test("昨收文字保留精确分值，不由近似坐标反算", { timeout: 10000 }, () => {
  const lastClose = "9007199254740993";
  const p = MobileIntradayProjection.fromInputs({ ...base, market: { last_close: lastClose } });
  const html = renderToStaticMarkup(createElement(component.DesktopIntradayChart, { projection: p, lastClose, indicator: "none", result: { kind: "idle" } }));
  assert.ok(html.includes("昨收 90071992547409.93"));
});

test("分时上下界贴合已有最高最低价，单边行情不强制包含昨收", { timeout: 10000 }, () => {
  const p = MobileIntradayProjection.fromInputs({ ...base, minutePoints: [{ time: 0, value: 11 }, { time: 1, value: 12 }] });
  const html = renderToStaticMarkup(createElement(component.DesktopIntradayChart, { projection: p, lastClose: "1000", indicator: "none", result: { kind: "idle" } }));
  assert.ok(html.includes('points="16,100 16.351464435146443,0"'));
  assert.ok(html.includes('top:0%">12.00'));
  assert.ok(html.includes('top:100%">11.00'));
  assert.ok(!html.includes('class="intraday-baseline"'));
});

test("唯一价格不伪造高低价，横线居中且不产生非法坐标", { timeout: 10000 }, () => {
  const p = MobileIntradayProjection.fromInputs({ ...base, minutePoints: [{ time: 0, value: 11 }, { time: 1, value: 11 }] });
  const html = renderToStaticMarkup(createElement(component.DesktopIntradayChart, { projection: p, lastClose: "1000", indicator: "none", result: { kind: "idle" } }));
  assert.ok(html.includes('points="16,50 16.351464435146443,50"'));
  assert.ok(!html.includes('NaN'));
});

test("当日权威 OHLC 极值纳入价域，不因分钟采样而遗漏盘中高低点", { timeout: 10000 }, () => {
  const p = MobileIntradayProjection.fromInputs({ ...base, minutePoints: [{ time: 0, value: 11 }] });
  const html = renderToStaticMarkup(createElement(component.DesktopIntradayChart, { projection: p, lastClose: "1000", dayRange: { high: 13, low: 9, volume: 100 }, indicator: "none", result: { kind: "idle" } }));
  assert.ok(html.includes('top:0%">13.00'));
  assert.ok(html.includes('top:100%">9.00'));
  assert.ok(html.includes('points="16,50"'));
});


test("零成交 OHLC 占位不把昨收强行并入竞价价域，重新渲染不保留旧极值", { timeout: 10000 }, () => {
  const p = MobileIntradayProjection.fromInputs({ ...base, auctionPoints: [{ time: 0, value: 11, volume: 100, buy: true }, { time: 1, value: 12, volume: 200, buy: true }] });
  const renderRange = (high: number, low: number, volume: number) => renderToStaticMarkup(createElement(component.DesktopIntradayChart, { projection: p, lastClose: "1000", dayRange: { high, low, volume }, indicator: "none", result: { kind: "idle" } }));
  assert.ok(renderRange(20, 8, 100).includes('top:0%">20.00'));
  const html = renderRange(10, 10, 0);
  assert.ok(html.includes('top:0%">12.00'));
  assert.ok(html.includes('top:100%">11.00'));
  assert.ok(!html.includes('class="intraday-baseline"'));
});

test("竞价 null 槽使用昨收参考轴，更新点只标有效指示变化", { timeout: 10000 }, () => {
  const p = MobileIntradayProjection.fromInputs({ ...base, auctionPoints: [{ time: 0, value: null, volume: 0, buy: false }, { time: 1, value: 11, volume: 100, buy: true }, { time: 2, value: null, volume: 0, buy: false }] });
  const html = renderToStaticMarkup(createElement(component.DesktopIntradayChart, { projection: p, lastClose: "1000", indicator: "none", result: { kind: "idle" } }));
  assert.ok(html.includes("无指示价时沿昨收0%参考轴显示"));
  assert.ok(!html.includes("intraday-no-price-slot"));
  assert.ok(html.includes('points="0,100 0.16161616161616163,0 0.32323232323232326,100"'));
  assert.equal((html.match(/class="intraday-auction-point"/g) || []).length, 1);
  assert.ok(html.includes(`cx="${intradayChartX({ phase: "auction", minute: 1 })}"`));
  assert.ok(html.includes('class="intraday-auction-line"'));
});

test("连续竞价有成交也只绘折线，不添加粗点", { timeout: 10000 }, () => {
  const p = MobileIntradayProjection.fromInputs({ ...base, minutePoints: [{ time: 0, value: 10, volume: 100 }, { time: 1, value: 11, volume: 200 }] });
  const html = renderToStaticMarkup(createElement(component.DesktopIntradayChart, { projection: p, lastClose: "1000", indicator: "none", result: { kind: "idle" } }));
  assert.ok(html.includes('class="intraday-price-line"'));
  assert.ok(!html.includes('<circle'));
});
