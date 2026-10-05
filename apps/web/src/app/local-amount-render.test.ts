import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { configureStore } from "@reduxjs/toolkit";
import { createElement, type ReactElement } from "react";
import { Provider } from "react-redux";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import type { Snapshot } from "../types/engine.ts";
import { store, type RootState } from "../store/store.ts";
import { DEFAULT_SETUP } from "../config/defaults.ts";

let vite: ViteDevServer;
let views: typeof import("./LocalRefreshViews.tsx");
let QuickTradingPanel: typeof import("./QuickTradingPanel.tsx").QuickTradingPanel;
let QuickTrading: typeof import("./quick-trading.ts").QuickTrading;
let MarketRuntimeProvider: typeof import("./MarketRuntimeProvider.tsx").MarketRuntimeProvider;

before(async () => {
  vite = await createServer({
    configFile: false,
    appType: "custom",
    server: { middlewareMode: true, ws: false },
    optimizeDeps: { noDiscovery: true },
  });
  ({ QuickTradingPanel } = await vite.ssrLoadModule("/src/app/QuickTradingPanel.tsx"));
  ({ QuickTrading } = await vite.ssrLoadModule("/src/app/quick-trading.ts"));
  views = await vite.ssrLoadModule("/src/app/LocalRefreshViews.tsx") as typeof views;
  ({ MarketRuntimeProvider } = await vite.ssrLoadModule("/src/app/MarketRuntimeProvider.tsx") as typeof import("./MarketRuntimeProvider.tsx"));
});

after(async () => {
  if (vite) await vite.close();
});

const snapshot: Snapshot = {
  seq: 1,
  tick: 0,
  day: 0,
  phase: "Continuous",
  markets: {
    "600101": {
      last_price: "1000",
      last_close: "1000",
      best_bid: "999",
      best_ask: "1001",
      bids: [["999", 250]],
      asks: [["1001", 500]],
    },
  },
  accounts: {
    "0": {
      cash: "12345",
      reserved_cash: "345",
      reserved_sell_qty: { "600101": 50 },
      positions: { "600101": { qty: 250, t1_locked: 100, invested_cents: "200000", recovered_cents: "0" } },
    },
  },
  daily_candles: {},
  active_daily_candles: {},
};

function renderView(view: ReactElement, activeSnapshot = snapshot): string {
  const state: RootState = {
    ...store.getState(),
    snapshot: { snapshot: activeSnapshot, lastSeq: 1, generation: "1", playerWorkingOrders: {}, playerOrdersReady: false },
    trades: { items: [{ seq: 1, code: "600101", price: "1000", qty: 250, maker: 1, taker: 2 }] },
  };
  const testStore = configureStore({ reducer: () => state });
  return renderToStaticMarkup(createElement(Provider, {
    store: testStore,
    children: createElement(MarketRuntimeProvider, {
      autoOrderManagerRef: { current: null },
      setNotice() {},
      hostRef: { current: null },
      children: view,
    }),
  }));
}

const browserFixture = { favorites: ["600101"], ready: true, error: null, view: "watchlist" as const, query: "", sortRules: [], setSortRules() {}, setView() {}, setQuery() {}, toggleFavorite() {}, reload() {} };

test("账户金额以元显示，持仓和 T+1 可卖股数仍以股显示", () => {
  const assets = renderView(createElement(views.DesktopAssets));
  assert.match(assets, /总资产<\/span><span class="value">2623\.45<\/span>/);
  assert.match(assets, /可用资金<\/span><span class="value">120<\/span>/);
  assert.match(assets, /总盈亏<\/span><span class="value up">\+500<\/span>/);
  const positions = renderView(createElement(views.PositionsPanel, { onOpenMarket() {} }));
  assert.match(positions, /持仓市值<\/span><b>2500元<\/b>/);
  assert.match(positions, /<td class="num">250<\/td><td class="num">100<\/td>/);
  assert.match(positions, /<td class="num">2500元<\/td>/);
});

test("桌面逐笔与五档盘口以手显示权威股数，不把零股量舍成整手", () => {
  const trades = renderView(createElement(views.TradesPanel));
  assert.match(trades, /成交量（手）<\/th>/);
  assert.match(trades, /<td class="num">2\.5<\/td>/);
  const book = renderView(createElement(views.ConnectedChartPanel, { chartPeriod: "分时", setChartPeriod() {}, klineDays: 20, browser: browserFixture }));
  assert.match(book, /五档盘口<span>价格（元） \/ 数量（手）<\/span>/);
  assert.match(book, /class="msd-book-depth sell"[^>]*><span>5<\/span>/);
  assert.match(book, /class="msd-book-depth buy"[^>]*><span>2\.5<\/span>/);
});

test("审计G46：一档、两档、五档卖盘标签对应真实rank，卖一靠近买一", () => {
  for (const count of [1, 2, 5]) {
    const active = structuredClone(snapshot);
    active.markets["600101"].asks = Array.from({ length: count }, (_, index) => [String(1001 + index), 100]);
    const book = renderView(createElement(views.ConnectedChartPanel, { chartPeriod: "分时", setChartPeriod() {}, klineDays: 20, browser: browserFixture }), active);
    const rows = [...book.matchAll(/msd-book-row[^>]*><span>卖(\d)<\/span><b class="[^"]*">([\d.]+)</g)];
    assert.deepEqual(rows.map(row => [Number(row[1]), row[2]]), Array.from({ length: count }, (_, index) => [count - index, ((1000 + count - index) / 100).toFixed(2)]));
  }
});

test("审计G49：持仓成本半偶到分，浮盈使用同一每股成本口径", () => {
  for (const [invested, recovered, cost, pnl] of [[200100, 0, "10.00", "+2"], [0, 200100, "-10.00", "+4002"]] as const) {
    const active = structuredClone(snapshot);
    active.accounts["0"].positions["600101"] = { qty: 200, t1_locked: 0, invested_cents: String(invested), recovered_cents: String(recovered) };
    active.markets["600101"].last_price = "1001";
    const html = renderView(createElement(views.PositionsPanel, { onOpenMarket() {} }), active);
    assert.match(html, new RegExp(`<td class="num">${cost.replace(".", "\\.")}<\\/td>`));
    assert.ok(html.includes(`${pnl}元`), html);
  }
});

test("Q16：持仓SSR保留真实负净成本金额与浮盈金额，不显示误导收益率", { timeout: 10000 }, () => {
  const active = structuredClone(snapshot);
  active.accounts["0"].positions["600101"] = { qty: 200, t1_locked: 0, invested_cents: "200000", recovered_cents: "400000" };
  active.markets["600101"].last_price = "1001";
  const html = renderView(createElement(views.PositionsPanel, { onOpenMarket() {} }), active);
  assert.match(html, /<td class="num">-10\.00<\/td>/);
  assert.match(html, /<td class="num up">\+4002元<\/td>/);
  const positionRow = html.match(/<tr><td class="mono">600101[^<]*<\/td>[\s\S]*?<\/tr>/)?.[0];
  assert.ok(positionRow, html);
  assert.doesNotMatch(positionRow, /%/);
});

test("G68：快捷涨跌停从当前setup读取非默认证券的创业板规则", { timeout: 10000 }, () => {
  const active = structuredClone(snapshot);
  active.markets = { "300999": active.markets["600101"]! };
  const setup = { ...DEFAULT_SETUP, stocks: [{ ...DEFAULT_SETUP.stocks[0]!, code: "300999", category: "ChiNext" as const }] };
  const trading = new QuickTrading({ setup: () => setup, snapshot: () => active, autoAllowed: () => true, submit: async () => {} });
  trading.edit("300999", "Buy", { priceMode: "highest" });
  trading.edit("300999", "Sell", { priceMode: "lowest" });
  const html = renderView(createElement(QuickTradingPanel, { trading, setup, code: "300999", side: "Buy", onSideChange() {}, onSelect() {}, notice() {} }), active);
  assert.match(html, /预留估算边界 8\.00元/); assert.match(html, /预留估算边界 12\.00元/);
  assert.doesNotMatch(html, /缺少.*交易规则/);
});

test("快捷面板显示现金与跨股可买手数都扣除待受理买单预占", { timeout: 10000 }, async () => {
  const active = structuredClone(snapshot);
  active.accounts["0"].cash = "300000";
  active.accounts["0"].reserved_cash = "0";
  active.markets["002156"] = { ...active.markets["600101"], last_price: "2000", last_close: "2000", best_bid: "1999", best_ask: "2000" };
  let reject!: (error: Error) => void;
  const trading = new QuickTrading({ setup: () => DEFAULT_SETUP, snapshot: () => active, autoAllowed: () => true, submit: () => new Promise<void>((_, fail) => { reject = fail; }) });
  const submitting = trading.submit("600101", "Buy");
  try {
    const html = renderView(createElement(QuickTradingPanel, { trading, setup: DEFAULT_SETUP, code: "002156", side: "Buy", onSideChange() {}, onSelect() {}, notice() {} }), active);
    assert.match(html, /可买 0 手 · 可用 1994\.99元（已扣待受理买单预占）/);
    assert.doesNotMatch(html, /可用 3000元/);
  } finally { reject(new Error("短测试释放待处理提交")); await submitting; }
});

test("G65：行情分类SSR公开当前分类而非只有active class", { timeout: 10000 }, () => {
  const html = renderView(createElement(views.ConnectedMarketPanel, { onSelect() {}, browser: browserFixture }));
  assert.match(html, /aria-pressed="true"[^>]*>自选<\/button>/);
});

test("低价股票的手机列表仍显示精确两位元价格，不额外补第三位", { timeout: 10000 }, () => {
  const active = structuredClone(snapshot);
  active.markets["600101"].last_price = "855";
  active.markets["600101"].last_close = "900";
  const html = renderView(createElement(views.ConnectedMarketPanel, { onSelect() {}, browser: browserFixture }), active);
  assert.match(html, /<small>8\.55<\/small>/);
  assert.doesNotMatch(html, /<small>8\.550<\/small>/);
});
