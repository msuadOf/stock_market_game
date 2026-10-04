import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createServer, type ViteDevServer } from "vite";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { aggregateCandles, MobileKlineProjection } from "../mobile/market-model.ts";
let KlineDetails: typeof import("./MarketKlinePanel.tsx").KlineDetails;
let MacdPanel: typeof import("./MarketKlinePanel.tsx").MacdPanel;
let vite: ViteDevServer;
let MarketKlinePanel: typeof import("./MarketKlinePanel.tsx").MarketKlinePanel;
before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  ({ MarketKlinePanel, MacdPanel, KlineDetails } = await vite.ssrLoadModule("/src/components/MarketKlinePanel.tsx"));
});
after(async () => { if (vite) await vite.close(); });

test("共用 K 线包含五条可选均线、手机版蜡烛与量柱及缩放控件", { timeout: 10000 }, () => {
  const dailyCandles = Array.from({ length: 75 }, (_, i) => ({ time: (i * 86400) as import("lightweight-charts").UTCTimestamp, open: 10, high: 12, low: 9, close: 11, volume: 250 }));
  const html = renderToStaticMarkup(createElement(MarketKlinePanel, { dailyCandles, period: "日K", indicatorCalculator: null }));
  assert.match(html, /msd-candle-chart/);
  assert.match(html, /msd-k-volume/);
  assert.match(html, /MA60:11.000/);
  assert.equal((html.match(/aria-label="MA\d+：[^"]*"/g) ?? []).length, 5);
  assert.match(html, /msd-chart-tools/);
});

test("共用 MACD 全零居中，小幅值撑满纵轴并按当前窗口槽位绘制", { timeout: 10000 }, () => {
  const candles = Array.from({ length: 35 }, (_, i) => ({ time: i as import("lightweight-charts").UTCTimestamp, open: 10, high: 12, low: 9, close: 11 }));
  const projection = MobileKlineProjection.fromInputs(candles, { capacity: 30, offsetFromEnd: 0 }, { kind: "idle" });
  const render = (value: number) => renderToStaticMarkup(createElement(MacdPanel, { projection, result: { kind: "ready", value: { macd: { dif: candles.map(() => value), dea: candles.map(() => 0), histogram: candles.map(() => value) }, candleKdj: { k: [], d: [], j: [] }, priceKdj: { k: [], d: [], j: [] } } } }));
  const zero = render(0);
  assert.equal((zero.match(/<line /g) ?? []).length, 30);
  assert.match(zero, /y1="36" y2="36"/);
  const small = render(.001);
  assert.match(small, /y1="68" y2="4"/);
  assert.match(small, new RegExp(`x1="${projection.slotFor(0).center}"`));
  assert.doesNotMatch(small, /NaN|Infinity/);
});
test("共用 MACD 显式展示宿主不可用与真实错误", { timeout: 10000 }, () => {
  const projection = MobileKlineProjection.fromInputs([], { capacity: 72, offsetFromEnd: 0 }, { kind: "idle" });
  for (const result of [{ kind: "unavailable" }, { kind: "error", message: "测试错误" }] as const) {
    const html = renderToStaticMarkup(createElement(MacdPanel, { projection, result }));
    assert.match(html, /role="alert"/);
    assert.match(html, result.kind === "error" ? /测试错误/ : /尚未就绪/);
    assert.doesNotMatch(html, /<svg/);
  }
});

test("详情读取所选蜡烛，涨跌使用前收，股转手且不虚构成交额", { timeout: 10000 }, () => {
 const html = renderToStaticMarkup(createElement(KlineDetails, { candle: { time: 86400 as import("lightweight-charts").UTCTimestamp, open: 10, high: 12, low: 9, close: 11, volume: 250 }, previousClose: 10, averages: [{ days: 5, color: "red", value: 10.1234 }], onClose() {} }));
 assert.match(html, /第2个交易日/); assert.match(html, /\+10.00%/); assert.match(html, /2.5手/);
 assert.match(html, /成交额<\/dt><dd>—/); assert.match(html, /10.123/); assert.match(html, /关闭K线详细信息/);
});

test("详情优先精确Cents，聚合周期明确标注起始日", { timeout: 10000 }, () => {
 const html = renderToStaticMarkup(createElement(KlineDetails, { period: "周K", candle: { time: 0 as import("lightweight-charts").UTCTimestamp, open: 0, high: 0, low: 0, close: 0, rawPrices: {open:"1001",high:"1010",low:"999",close:"1005"} }, previousRawClose: "1000", averages: [], onClose() {} }));
 assert.match(html, /周K起始日/); assert.match(html, /10.05/); assert.match(html, /\+0.05/); assert.match(html, /\+0.50%/);
});

test("真实周K聚合保留高位精确OHLC与前收差额", { timeout: 10000 }, () => {
 const daily = Array.from({length:10}, (_, index) => ({ time: index * 86400 as import("lightweight-charts").UTCTimestamp, open: 1, high: 1, low: 1, close: 1, rawPrices: {open:"900719925474099301", high:"900719925474099399", low:"900719925474099300", close:index < 5 ? "900719925474099301" : "900719925474099302"} }));
 const weekly = aggregateCandles(daily, "周K");
 assert.equal(weekly[1].rawPrices?.high, "900719925474099399");
 const html = renderToStaticMarkup(createElement(KlineDetails, {period:"周K", candle:weekly[1], previousRawClose:weekly[0].rawPrices!.close, averages:[], onClose() {}}));
 assert.match(html, /9007199254740993.02/); assert.match(html, /\+0.01/);
});
