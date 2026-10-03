import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import type { KlinePoint } from "../components/PriceChart.tsx";
import type { MarketSnap, TradeEvent } from "../types/engine.ts";
import type { MobileStockDetail as DetailComponent } from "./MobileStockDetail.tsx";

let vite: ViteDevServer;
let MobileStockDetail: typeof DetailComponent;

before(async () => {
  vite = await createServer({
    configFile: false,
    appType: "custom",
    server: { middlewareMode: true, ws: false },
    optimizeDeps: { noDiscovery: true },
  });
  ({ MobileStockDetail } = await vite.ssrLoadModule("/src/mobile/MobileStockDetail.tsx") as typeof import("./MobileStockDetail.tsx"));
});

after(async () => {
  if (vite) await vite.close();
});

const market: MarketSnap = {
  last_price: 1_000,
  last_close: 1_000,
  best_bid: 999,
  best_ask: 1_001,
  bids: [[999, 250], [998, 500]],
  asks: [[1_001, 500], [1_002, 1_000]],
};

const candle: KlinePoint = {
  time: 0 as KlinePoint["time"],
  open: 10,
  high: 10.2,
  low: 9.8,
  close: 10.1,
  volume: 250,
  tradeStats: { turnoverCents: "69900", tradeCount: 2 },
};

const trade: TradeEvent = { seq: 1, code: "600101", price: 1_000, qty: 100, maker: 1, taker: 2 };

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
  assert.match(html, /成交额<\/span><b>699元<\/b>/);
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


test("分时 projection 经真实组件保留 null 竞价量、阶段独立高度与算术均价", () => {
  const html = renderDetail("分时", "盘口", {
    auctionPoints: [{ time: 0, value: null, volume: 100_000, buy: false }, { time: 99, value: 12, volume: 50_000, buy: true }],
    minutePoints: [{ time: 119, value: 10, volume: 100, buy: false }, { time: 120, value: 8, volume: 200, buy: true }],
    gameDay: 3,
  });
  assert.match(html, /均价:9\.00/);
  assert.match(html, /data-intraday-signature="3:continuous:120:8:200"/);
  assert.match(html, /data-auction-count="2"/);
  const heights = [...html.matchAll(/<i[^>]*style="left:[^;]+;height:([^%]+)%"/g)].map(match => Number(match[1]));
  assert.deepEqual(heights, [100, 50, 50, 100]);
  assert.match(html, /msd-auction-dot/);
});

test("K 线 projection 经真实组件保持实体、影线和成交量共享固定槽位", () => {
  const html = renderDetail("日K", "盘口");
  assert.match(html, /data-kline-signature="0:10:10\.2:9\.8:10\.1:250"/);
  const upper = html.match(/class="upper-wick" x1="([^"]+)" x2="([^"]+)" y1="([^"]+)" y2="([^"]+)"/);
  const lower = html.match(/class="lower-wick" x1="([^"]+)" x2="([^"]+)" y1="([^"]+)" y2="([^"]+)"/);
  assert.ok(upper); assert.ok(lower);
  assert.equal(Number(upper[1]), 390 / 72 / 2);
  assert.equal(Number(upper[1]), Number(lower[1]));
  assert.ok(Math.abs(Number(upper[3]) - 8) < 1e-10);
  assert.ok(Math.abs(Number(upper[4]) - 49.5) < 1e-10);
  assert.ok(Math.abs(Number(lower[3]) - 91) < 1e-10);
  assert.ok(Math.abs(Number(lower[4]) - 174) < 1e-10);
  assert.match(html, /class="msd-k-volume"[^>]*>[\s\S]*?y="9"/);
});
