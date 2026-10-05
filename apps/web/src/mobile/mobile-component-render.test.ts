import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createElement } from "react";
let ChartSettingsFixture: typeof import("../test-support/ChartSettingsFixture.tsx").ChartSettingsFixture;
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
  ({ ChartSettingsFixture } = await vite.ssrLoadModule("/src/test-support/ChartSettingsFixture.tsx"));
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
  time: 1893456000 as KlinePoint["time"],
  open: 10,
  high: 10,
  low: 8,
  close: 8,
  rawPrices: { open: "1000", high: "1000", low: "800", close: "800" },
  volume: 250,
  tradeStats: { turnoverCents: "225000", tradeCount: 2 },
};

const trade: TradeEvent = { seq: 1, code: "600101", price: "1000", qty: 100, maker: "1", taker: "2" };

test("移动KDJ严格遵守所选指标源，不支持时明确显示而不调用Rust", { timeout: 10000 }, () => {
  assert.match(renderDetail("日K", "盘口"), /前端.*不支持.*KDJ/);
  assert.match(renderDetail("日K", "盘口", { indicatorDataSource: "rust" }), /Rust.*不支持.*KDJ/);
  const dailyCandles = [2, 3, 4, 7, 8].map((day) => ({ ...candle, time: (Date.parse(`2030-01-${String(day).padStart(2, "0")}T00:00:00Z`) / 1000) as KlinePoint["time"] }));
  const rust = renderDetail("日K", "盘口", { indicatorDataSource: "rust", dailyCandles });
  assert.match(rust, /Rust.*不支持.*MA/);
  assert.doesNotMatch(rust, /<polyline class="ma5"/);
  assert.match(renderDetail("日K", "盘口", { dailyCandles }), /<polyline class="ma5" aria-label="MA5曲线"[^>]*points="[^"]+,174"/);
  const insufficient = renderDetail("日K", "盘口");
  assert.match(insufficient, /MA5：历史不足/);
  assert.doesNotMatch(insufficient, /<polyline class="ma5"/);
});

type DetailProps = Parameters<typeof DetailComponent>[0];

test("普通成员的移动详情禁用共享市场暂停与倍速控制", { timeout: 10000 }, () => {
  const markup = renderDetail("分时", "盘口", { canControl: false, running: false });
  assert.match(markup, /data-state="paused"[^>]*disabled=""/);
  assert.match(markup, /<select aria-label="模拟速度"[^>]*disabled=""/);
});

function renderDetail(period: "分时" | "日K" | "周K" | "月K", infoTab: "盘口" | "资金", overrides: Partial<DetailProps> = {}): string {
  return renderToStaticMarkup(createElement(ChartSettingsFixture, null, createElement(MobileStockDetail, {
    code: "600101",
    name: "测试股份",
    market,
    minutePoints: [{ time: 120, value: 10, volume: 250, buy: true }],
    auctionPoints: [{ time: 99, value: 10, volume: 500, buy: true }],
    dailyCandles: [candle],
    activeDailyCandle: candle,
    indicatorCalculator: null,
    indicatorDataSource: "frontend",
    indicatorCapabilities: { intradayAverage: true, macd: false, priceKdj: false, candleKdj: false },
    calculateIntradayAverageCurve: async ({ samples }) => samples.map((input) => ({ turnoverCents: input.turnoverCents, volumeShares: input.volumeShares })),
    onIndicatorDataSourceChange() {},
    confirmations: [],
    confirmationLoading: false,
    confirmationError: null,
    confirmationQueried: false,
    onRefreshConfirmations() {},
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
  })));
}

test("手机信息菜单只提供已接入的财务、盘口和资金，不展示参考软件占位入口", { timeout: 10000 }, () => {
  const html = renderDetail("分时", "资金");
  const list = html.match(/class="msd-info-tabs"[^>]*>([\s\S]*?)<\/div>/)?.[1];
  assert.ok(list);
  assert.deepEqual([...list.matchAll(/role="tab"[\s\S]*?>(.*?)<\/button>/g)].map((item) => item[1]), ["财务", "盘口", "资金"]);
  assert.doesNotMatch(html, /info-(看点|资讯|社区|简况)|msd-placeholder/);
});

test("手机分时09:30连接已存在的两个阶段端点，不额外生成粗点", { timeout: 10000 }, () => {
  const overrides = { auctionPoints: [{ time: 99, value: 11, volume: 100, buy: true }], minutePoints: [{ time: 0, value: 12, volume: 200 }], elapsedMinutes: 1 };
  const html = renderDetail("分时", "盘口", overrides);
  assert.match(html, /class="msd-session-join" points="16,[\d.]+ 16,[\d.]+"/);
  assert.equal((html.match(/class="msd-auction-dot"/g) || []).length, 1);
  for (const missing of [{ ...overrides, auctionPoints: [] }, { ...overrides, minutePoints: [] }, { ...overrides, minutePoints: [{ time: 1, value: 12 }] }, { ...overrides, minutePoints: [{ time: 0, value: 11 }] }]) {
    assert.doesNotMatch(renderDetail("分时", "盘口", missing), /class="msd-session-join"/);
  }
});

test("移动详情将权威股数接入盘口、逐笔和两种图表的手数显示", () => {
  const intraday = renderDetail("分时", "盘口");
  assert.match(intraday, /aria-label="五档盘口，数量单位为手"/);
  assert.match(intraday, /卖1[\s\S]*?>5<\/span>/);
  assert.match(intraday, /买1[\s\S]*?>2\.5<\/span>/);
  assert.match(intraday, /style="--depth:50%"/);
  assert.match(intraday, /当日成交量 <b>2\.5手<\/b>/);
  assert.match(intraday, /分时量（手）[\s\S]*?量:2\.5手/);
  assert.match(intraday, /aria-label="600101 最近逐笔成交，数量单位为手"[\s\S]*?<tr data-trade-seq="1"><td data-time-missing="true">成交时间缺失<\/td><td class="flat">10\.00<\/td><td>1<\/td><\/tr>/);

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

test("选中 Rust 但能力关闭时明确提示不支持且不回退前端", () => {
  const html = renderDetail("分时", "盘口", {
    indicatorDataSource: "rust",
    indicatorCapabilities: { intradayAverage: false, macd: false, priceKdj: false, candleKdj: false },
  });
  assert.match(html, /所选Rust指标源不支持分时均价，未回退到另一数据源/);
});

test("移动详情提供共享指标源选择及本人交割单的按需查询入口", () => {
  const html = renderDetail("分时", "盘口");
  assert.match(html, /aria-label="指标数据源"/);
  assert.match(html, /交割单按需查询，不会使用公开成交事件补造/);
  assert.match(html, /查询本人交割单/);
});

test("G12：真实分时量renderer绘制红色空心和绿色实心，不改变半像素槽宽", { timeout: 10000 }, () => {
  const html = renderDetail("分时", "盘口", { auctionPoints: [], minutePoints: [{ time: 0, value: 11, volume: 100, buy: true }, { time: 1, value: 10, volume: 200, buy: false }] });
  assert.match(html, /class="msd-minute-volume-mark rise"[^>]*width="0\.5"[^>]*fill="none"[^>]*stroke="var\(--msd-rise\)"/);
  assert.match(html, /class="msd-minute-volume-mark fall"[^>]*width="0\.5"[^>]*fill="var\(--msd-fall\)"[^>]*stroke="none"/);
});

test("竞价实际SVG将null显示在0%参考轴，有效更新绘粗点且原始数据不变", () => {
  const auctionPoints = [11, null, 12, 13, null, 14].map((value, time) => ({ time, value, volume: time * 100, buy: value !== null }));
  const html = renderDetail("分时", "盘口", { auctionPoints, minutePoints: [] });
  assert.equal((html.match(/class="msd-auction-line"/g) ?? []).length, 1);
  assert.equal((html.match(/class="msd-auction-dot"/g) ?? []).length, 4);
  const line = html.match(/class="msd-auction-line" points="([^"]+)"/);
  assert.ok(line);
  const coordinates = line[1].split(" ");
  assert.equal(coordinates.length, 6);
  assert.equal(Number(coordinates[1].split(",")[1]), 50);
  assert.equal(Number(coordinates[4].split(",")[1]), 50);
  assert.equal(auctionPoints[1].value, null);
  assert.equal(auctionPoints[4].value, null);
  assert.equal((html.match(/class="msd-minute-volume-mark /g) ?? []).length, 6);
});

test("K 线 projection 经真实组件保持实体、影线和成交量共享固定槽位", () => {
  const html = renderDetail("日K", "盘口");
  assert.match(html, /data-kline-signature="1893456000:10:10:8:8:250"/);
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

test("移动端连续竞价有成交也保持普通折线", () => {
  const html = renderDetail("分时", "盘口", { auctionPoints: [], minutePoints: [{ time: 0, value: 10, volume: 100 }, { time: 1, value: 11, volume: 200 }] });
  assert.ok(html.includes('class="msd-price-line"'));
  assert.ok(!html.includes('class="msd-auction-dot"'));
  assert.ok(!html.includes('本分钟有成交'));
});

test("零成交日的开高低显示未形成，不把昨收占位当作成交价格", { timeout: 10000 }, () => {
  const html = renderDetail("分时", "资金", { activeDailyCandle: { ...candle, volume: 0, tradeStats: undefined } });
  assert.match(html, /高 <b class="flat">--<\/b>/);
  assert.match(html, /低 <b class="flat">--<\/b>/);
  assert.match(html, /开 <b class="flat">--<\/b>/);
  assert.match(html, /成交额<\/span><b>0元<\/b>/);
});

test("个股成交明细筛选证券并使用精确价格、小数手和权威成交时间", { timeout: 10000 }, () => {
  const html = renderDetail("分时", "盘口", { trades: [
    { ...trade, code: "002156", seq: 10, tick: 902, price: "9999" },
    { ...trade, seq: 9, tick: 901, price: "1001", qty: 250 },
  ] });
  assert.doesNotMatch(html, /99\.99/);
  assert.match(html, /data-trade-seq="9"/);
  assert.match(html, /09:30:01/);
  assert.match(html, /10\.01/);
  assert.match(html, />2\.5<\/td>/);
  assert.match(html, /最近成交缓存/);
});

test("明细先筛选股票再截取七笔，展开入口显示当前股票缓存数量", { timeout: 10000 }, () => {
  const ownTrades = Array.from({ length: 9 }, (_, index) => ({ ...trade, seq: 20 - index, tick: 920 - index }));
  const html = renderDetail("分时", "盘口", { trades: [{ ...trade, seq: 30, code: "002156" }, ...ownTrades] });
  const rows = [...html.matchAll(/data-trade-seq="(\d+)"/g)].map(match => Number(match[1]));
  assert.deepEqual(rows, [20, 19, 18, 17, 16, 15, 14]);
  assert.match(html, /aria-label="显示缓存明细（9笔）"[^>]*aria-expanded="false"/);
  assert.match(html, /aria-label="价格（元）"/);
  assert.match(html, /aria-label="量（手）"/);
});
