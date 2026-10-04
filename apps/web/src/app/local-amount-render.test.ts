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
let MarketRuntimeProvider: typeof import("./MarketRuntimeProvider.tsx").MarketRuntimeProvider;

before(async () => {
  vite = await createServer({
    configFile: false,
    appType: "custom",
    server: { middlewareMode: true, ws: false },
    optimizeDeps: { noDiscovery: true },
  });
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
  const book = renderView(createElement(views.ConnectedChartPanel, { chartPeriod: "分时", setChartPeriod() {}, klineDays: 20, setKlineDays() {} }));
  assert.match(book, /五档盘口（手）/);
  assert.match(book, /class="ob-qty">5<\/span>/);
  assert.match(book, /class="ob-qty">2\.5<\/span>/);
});

test("审计G46：一档、两档、五档卖盘标签对应真实rank，卖一靠近买一", () => {
  for (const count of [1, 2, 5]) {
    const active = structuredClone(snapshot);
    active.markets["600101"].asks = Array.from({ length: count }, (_, index) => [String(1001 + index), 100]);
    const book = renderView(createElement(views.ConnectedChartPanel, { chartPeriod: "分时", setChartPeriod() {}, klineDays: 20, setKlineDays() {} }), active);
    const rows = [...book.matchAll(/ob-row ob-ask[^>]*><span class="ob-label">卖(\d)<\/span><span class="ob-price[^"]*">([^<]+)</g)];
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
  const html = renderView(createElement(views.TradeMarketControls, { activeSetup: setup, tradeCode: "300999", setPriceText() {}, setQtyText() {} }), active);
  assert.match(html, /跌停 8\.00/); assert.match(html, /涨停 12\.00/);
  assert.doesNotMatch(html, /缺少.*交易规则/);
});

test("G65：行情分类SSR公开当前分类而非只有active class", { timeout: 10000 }, () => {
  const html = renderView(createElement(views.ConnectedMarketPanel, { onSelect() {} }));
  assert.match(html, /aria-current="page"[^>]*>自选/);
});
