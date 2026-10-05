import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { configureStore } from "@reduxjs/toolkit";
import { createElement } from "react";
import { Provider } from "react-redux";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import type { KlinePoint } from "../components/PriceChart.tsx";
import { store, type RootState } from "../store/store.ts";
import type { MarketSnap, Snapshot } from "../types/engine.ts";
import type { MobileStockDetail as DetailComponent } from "./MobileStockDetail.tsx";

let vite: ViteDevServer;
let MobileStockDetail: typeof DetailComponent;
let MarketGrid: typeof import("../components/MarketGrid.tsx").MarketGrid;
let PositionsPanel: typeof import("../app/LocalRefreshViews.tsx").PositionsPanel;
let MarketRuntimeProvider: typeof import("../app/MarketRuntimeProvider.tsx").MarketRuntimeProvider;
let ChartSettingsFixture: typeof import("../test-support/ChartSettingsFixture.tsx").ChartSettingsFixture;

before(async () => {
  vite = await createServer({
    configFile: false,
    appType: "custom",
    server: { middlewareMode: true, ws: false },
    optimizeDeps: { noDiscovery: true },
  });
  ({ MobileStockDetail } = await vite.ssrLoadModule("/src/mobile/MobileStockDetail.tsx") as typeof import("./MobileStockDetail.tsx"));
  ({ MarketGrid } = await vite.ssrLoadModule("/src/components/MarketGrid.tsx") as typeof import("../components/MarketGrid.tsx"));
  ({ PositionsPanel } = await vite.ssrLoadModule("/src/app/LocalRefreshViews.tsx") as typeof import("../app/LocalRefreshViews.tsx"));
  ({ MarketRuntimeProvider } = await vite.ssrLoadModule("/src/app/MarketRuntimeProvider.tsx") as typeof import("../app/MarketRuntimeProvider.tsx"));
  ({ ChartSettingsFixture } = await vite.ssrLoadModule("/src/test-support/ChartSettingsFixture.tsx") as typeof import("../test-support/ChartSettingsFixture.tsx"));
});

after(async () => {
  if (vite) await vite.close();
});

const market: MarketSnap = {
  last_price: "1010",
  last_close: "1000",
  best_bid: "1001",
  best_ask: "1009",
  bids: [["1001", 100]],
  asks: [["1009", 100]],
};

const snapshot: Snapshot = {
  seq: 1,
  tick: 0,
  day: 0,
  phase: "Continuous",
  markets: { "600101": market },
  accounts: { "0": { cash: "100000", reserved_cash: "0", reserved_sell_qty: {}, positions: { "600101": { qty: 100, t1_locked: 0, invested_cents: "100000", recovered_cents: "0" } } } },
  daily_candles: {},
  active_daily_candles: {},
};

const candle: KlinePoint = {
  time: 1893456000 as KlinePoint["time"],
  open: 10,
  high: 10.1,
  low: 9.9,
  close: 10.1,
  rawPrices: { open: "1000", high: "1010", low: "990", close: "1010" },
  volume: 100,
};

function renderPortfolio(): string {
  const state: RootState = {
    ...store.getState(),
    snapshot: { snapshot, lastSeq: 1, generation: "1", playerWorkingOrders: {}, playerOrdersReady: false },
  };
  const testStore = configureStore({ reducer: () => state });
  return renderToStaticMarkup(createElement(Provider, {
    store: testStore,
    children: createElement(MarketRuntimeProvider, {
      autoOrderManagerRef: { current: null },
      setNotice() {},
      hostRef: { current: null },
      children: createElement(PositionsPanel, { onOpenMarket() {} }),
    }),
  }));
}

test("MobileStockDetail SSR 输出被对比度测试覆盖的真实行情消费者", { timeout: 10000 }, () => {
  const props: Parameters<typeof DetailComponent>[0] = {
    code: "600101",
    name: "测试股份",
    market,
    minutePoints: [],
    auctionPoints: [],
    dailyCandles: [candle],
    activeDailyCandle: candle,
    indicatorCalculator: null,
    indicatorDataSource: "frontend",
    indicatorCapabilities: { intradayAverage: true, macd: false, priceKdj: false, candleKdj: false },
    calculateIntradayAverageCurve: async ({ samples }) => samples.map((sample) => ({ turnoverCents: sample.turnoverCents, volumeShares: sample.volumeShares })),
    onIndicatorDataSourceChange() {},
    confirmations: [],
    confirmationLoading: false,
    confirmationError: null,
    confirmationQueried: false,
    onRefreshConfirmations() {},
    trades: [],
    elapsedMinutes: 0,
    totalMinutes: 240,
    klineDays: 20,
    period: "分时",
    infoTab: "盘口",
    speed: 1,
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
  };
  const html = renderToStaticMarkup(createElement(ChartSettingsFixture, null, createElement(MobileStockDetail, props)));
  const portfolioHtml = renderPortfolio();
  const marketGridHtml = renderToStaticMarkup(createElement(MarketGrid, {
    browser: { favorites: [], ready: true, error: null, view: "all", query: "", sortRules: [], setSortRules() {}, setView() {}, setQuery() {}, toggleFavorite() {}, reload() {} },
    markets: {
      "600101": market,
      "600102": { ...market, last_price: "990", best_bid: "989", best_ask: "991", bids: [["989", 100]], asks: [["991", 100]] },
    },
    selectedCode: null,
    onSelect() {},
    heldCodes: new Set<string>(),
    priceHistoryByCode: {},
  }));
  assert.match(html, /class="msd-header"[\s\S]*?class="msd-security-title"[\s\S]*?<strong>测试股份<\/strong><small>600101<\/small>/);
  assert.match(html, /class="msd-quote"[\s\S]*?class="msd-last rise"/);
  assert.match(html, /class="msd-day-prices"[\s\S]*?class="fall"/);
  assert.match(html, /class="msd-stock-stats"[\s\S]*?class="rise"[\s\S]*?class="fall"/);
  assert.match(portfolioHtml, /class="mobile-portfolio-summary"[\s\S]*?class="up"/);
  assert.match(marketGridHtml, /class="mobile-market-price up"[\s\S]*?<strong>\+1\.00%<\/strong>/);
  assert.match(marketGridHtml, /class="mobile-market-price down"[\s\S]*?<strong>-1\.00%<\/strong>/);
});
