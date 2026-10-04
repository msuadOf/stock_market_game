import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import type { KlinePoint } from "../components/PriceChart.tsx";
import type { MarketSnap, TradeEvent } from "../types/engine.ts";
import type { MobileStockDetail as DetailComponent } from "./MobileStockDetail.tsx";
import { initialMobileUiState, reduceMobileUi } from "./mobile-ui-state.ts";

let vite: ViteDevServer;
let MobileStockDetail: typeof DetailComponent;
let MobileDetailLayer: typeof import("./MobileDetailLayer.tsx").MobileDetailLayer;

before(async () => {
  vite = await createServer({
    configFile: false,
    appType: "custom",
    server: { middlewareMode: true, ws: false },
    optimizeDeps: { noDiscovery: true },
  });
  ({ MobileStockDetail } = await vite.ssrLoadModule("/src/mobile/MobileStockDetail.tsx") as typeof import("./MobileStockDetail.tsx"));
  ({ MobileDetailLayer } = await vite.ssrLoadModule("/src/mobile/MobileDetailLayer.tsx") as typeof import("./MobileDetailLayer.tsx"));
});

after(async () => {
  if (vite) await vite.close();
});

const market: MarketSnap = {
  last_price: "1000",
  last_close: "1000",
  best_bid: "999",
  best_ask: "1001",
  bids: [["999", 250], ["998", 500]],
  asks: [["1001", 500], ["1002", 1_000]],
};

test("审计G45：App共用详情层从自选打开会实际渲染，返回后撤下且保留自选", () => {
  const list = reduceMobileUi(initialMobileUiState, { type: "switch-primary", tab: "watchlist" });
  const detail = reduceMobileUi(list, { type: "open-detail", code: "600101" });
  const children = createElement("span", null, "实际详情内容");
  assert.match(renderToStaticMarkup(createElement(MobileDetailLayer, { ui: detail, children })), /mobile-detail-page.*实际详情内容/);
  const returned = reduceMobileUi(detail, { type: "back" });
  assert.equal(returned.primaryTab, "watchlist");
  assert.equal(renderToStaticMarkup(createElement(MobileDetailLayer, { ui: returned, children })), "");
});

const candle: KlinePoint = {
  time: 0 as KlinePoint["time"],
  open: 10,
  high: 10,
  low: 8,
  close: 8,
  rawPrices: { open: "1000", high: "1000", low: "800", close: "800" },
  volume: 250,
  tradeStats: { turnoverCents: "225000", tradeCount: 2 },
};

const trade: TradeEvent = { seq: 1, code: "600101", price: "1000", qty: 100, maker: 1, taker: 2 };

type DetailProps = Parameters<typeof DetailComponent>[0];

function renderDetail(period: "分时" | "日K" | "周K" | "月K", infoTab: "盘口" | "资金", overrides: Partial<DetailProps> = {}): string {
  return renderToStaticMarkup(createElement(MobileStockDetail, {
    code: "600101",
    name: "测试股份",
    market,
    minutePoints: [{ time: 120, value: 10, volume: 250, buy: true }],
    auctionPoints: [{ time: 99, value: 10, volume: 500, buy: true }],
    dailyCandles: [candle],
    activeDailyCandle: candle,
    indicatorCalculator: null,
    trades: [trade],
    elapsedMinutes: 121,
    totalMinutes: 240,
    klineDays: 20,
    period,
    infoTab,
    speed: 1,
    measuredSpeed: "实测 0x",
    running: false,
    gameDay: 0,
    gameTick: 901,
    onKlineDaysChange() {},
    onPeriodChange() {},
    onInfoTabChange() {},
    onSpeedChange() {},
    onPauseToggle() {},
    onBack() {},
    onPrevious() {},
    onNext() {},
    companyContent: null,
    ...overrides,
  }));
}

test("移动详情将权威股数接入盘口、逐笔和两种图表的手数显示", () => {
  const intraday = renderDetail("分时", "盘口");
  assert.match(intraday, /aria-label="五档盘口，数量单位为手"/);
  assert.match(intraday, /卖1[\s\S]*?>5<\/span>/);
  assert.match(intraday, /买1[\s\S]*?>2\.5<\/span>/);
  assert.match(intraday, /style="--depth:50%"/);
  assert.match(intraday, /当日成交量 <b>2\.5手<\/b>/);
  assert.match(intraday, /分时量（手）[\s\S]*?量:2\.5手/);
  assert.match(intraday, /aria-label="逐笔成交"[\s\S]*?>1<\/span>/);

  const daily = renderDetail("日K", "盘口");
  assert.match(daily, /成交量（手）[\s\S]*?量:2\.5手/);
  assert.match(daily, /aria-label="日K成交量，单位为手"/);
  assert.match(daily, /data-kline-count="1"/);
});

test("资金页展示权威成交额与成交笔数，不从截取的逐笔列表推算", () => {
  const html = renderDetail("分时", "资金");
  assert.match(html, /成交额<\/span><b>2250元<\/b>/);
  assert.match(html, /成交量（手）<\/span><b>2\.5<\/b>/);
  assert.match(html, /成交笔数<\/span><b>2<\/b>/);
  assert.match(html, /来自引擎权威撮合统计/);
});

test("分时图真实渲染压缩午休后的统一时间轴和权威时间槽", () => {
  const html = renderDetail("分时", "盘口");
  assert.equal((html.match(/11:30\/13:00/g) ?? []).length, 1);
  assert.match(html, /10:30[\s\S]*?11:30\/13:00[\s\S]*?14:00/);
  assert.match(html, /data-intraday-latest-minute="120"/);
  assert.match(html, /data-intraday-count="1"/);
  assert.match(html, /data-auction-volume-line-count="1"/);
});

test("G32：价格SVG、昨收中轴和0%标签共用价格绘图区，不含时间轴", { timeout: 10000 }, () => {
  const html = renderDetail("分时", "盘口");
  assert.match(html, /class="msd-price-plot">[\s\S]*?class="msd-scale msd-scale-mid"[\s\S]*?<svg[\s\S]*?<\/svg><\/div><div class="msd-time-axis"/);
});

test("G14：逐笔时间取自身tick，缺失明确显示而不借用当前时钟", { timeout: 10000 }, () => {
  const html = renderDetail("分时", "盘口", { trades: [{ ...trade, tick: 901 }, { ...trade, seq: 2, tick: 960 }, { ...trade, seq: 3 }] });
  assert.match(html, /09:30:01/); assert.match(html, /09:31:00/); assert.match(html, /成交时间缺失/);
});


test("分时 projection 经真实组件保留 null 竞价量、阶段独立高度与真实日内均价", () => {
  const html = renderDetail("分时", "盘口", {
    auctionPoints: [{ time: 0, value: null, volume: 100_000, buy: false }, { time: 99, value: 12, volume: 50_000, buy: true }],
    minutePoints: [
      { time: 119, value: 10, volume: 100, cumulativeTurnoverCents: "100000", cumulativeVolumeShares: 100 },
      { time: 120, value: 8, volume: 200, cumulativeTurnoverCents: "260000", cumulativeVolumeShares: 300 },
    ],
    activeDailyCandle: { ...candle, volume: 300, tradeStats: { turnoverCents: "260000", tradeCount: 2 } },
    gameDay: 3,
  });
  assert.match(html, /均价:8\.67/);
  assert.match(html, /data-intraday-signature="3:continuous:120:8:200"/);
  assert.match(html, /data-auction-count="2"/);
  const heights = [...html.matchAll(/<rect class="msd-minute-volume-mark [^"]+"[^>]*height="([^%]+)%"/g)].map(match => Number(match[1]));
  assert.deepEqual(heights, [100, 50, 50, 100]);
  assert.match(html, /msd-auction-dot/);
});

test("没有真实成交额统计或成交量时，组件明确显示不可用或暂无成交", () => {
  const unavailable = renderDetail("分时", "盘口", { activeDailyCandle: { ...candle, tradeStats: undefined } });
  assert.ok(unavailable.includes("均价:不支持（缺少真实成交额统计）"));
  const missingShares = renderDetail("分时", "盘口", { activeDailyCandle: { ...candle, volume: undefined } });
  assert.ok(missingShares.includes("均价:不支持（缺少真实成交股数）"));
  const noTrades = renderDetail("分时", "盘口", { activeDailyCandle: { ...candle, volume: 0, tradeStats: { turnoverCents: "0", tradeCount: 0 } } });
  assert.match(noTrades, /均价:暂无成交/);
});

test("G12：真实分时量renderer绘制红色空心和绿色实心，不改变半像素槽宽", { timeout: 10000 }, () => {
  const html = renderDetail("分时", "盘口", { auctionPoints: [], minutePoints: [{ time: 0, value: 11, volume: 100, buy: true }, { time: 1, value: 10, volume: 200, buy: false }] });
  assert.match(html, /class="msd-minute-volume-mark rise"[^>]*width="0\.5"[^>]*fill="none"[^>]*stroke="var\(--msd-rise\)"/);
  assert.match(html, /class="msd-minute-volume-mark fall"[^>]*width="0\.5"[^>]*fill="var\(--msd-fall\)"[^>]*stroke="none"/);
});

test("审计G47：实际SVG不跨null连接，有效单点仍绘制", () => {
  const html = renderDetail("分时", "盘口", { auctionPoints: [11, null, 12, 13, null, 14].map((value, time) => ({ time, value, volume: time * 100, buy: value !== null })) });
  assert.equal((html.match(/class="msd-auction-line"/g) ?? []).length, 3);
  assert.equal((html.match(/class="msd-auction-dot"/g) ?? []).length, 2);
  const lines = [...html.matchAll(/class="msd-auction-line" points="([^"]+)"/g)];
  assert.deepEqual(lines.map(line => line[1].split(" ").length), [1, 2, 1]);
});

test("K 线 projection 经真实组件保持实体、影线和成交量共享固定槽位", () => {
  const html = renderDetail("日K", "盘口");
  assert.match(html, /data-kline-signature="0:10:10:8:8:250"/);
  const upper = html.match(/class="upper-wick" x1="([^"]+)" x2="([^"]+)" y1="([^"]+)" y2="([^"]+)"/);
  const lower = html.match(/class="lower-wick" x1="([^"]+)" x2="([^"]+)" y1="([^"]+)" y2="([^"]+)"/);
  assert.ok(upper); assert.ok(lower);
  assert.equal(Number(upper[1]), 390 / 72 / 2);
  assert.equal(Number(upper[1]), Number(lower[1]));
  assert.ok(Math.abs(Number(upper[3]) - 8) < 1e-10);
  assert.ok(Math.abs(Number(upper[4]) - 8) < 1e-10);
  assert.ok(Math.abs(Number(lower[3]) - 174) < 1e-10);
  assert.ok(Math.abs(Number(lower[4]) - 174) < 1e-10);
  assert.match(html, /class="msd-k-volume"[^>]*>[\s\S]*?y="9"/);
});
