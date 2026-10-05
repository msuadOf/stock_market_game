import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import type { TradeConfirmationTable as TableComponent } from "./TradeConfirmationTable.tsx";

let vite: ViteDevServer;
let TradeConfirmationTable: typeof TableComponent;

before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  ({ TradeConfirmationTable } = await vite.ssrLoadModule("/src/components/TradeConfirmationTable.tsx") as typeof import("./TradeConfirmationTable.tsx"));
});

after(async () => {
  await vite?.close();
});

test("成交交割单逐项展示真实成交额与三项实际费用", () => {
  const row = {
    receipt_id: "12",
    civil_date: "2030-01-02",
    code: "600000",
    side: "Buy",
    price: "1000",
    quantity_shares: 100,
    gross: "100000",
    actual_fees: { commission: "500", stamp_tax: "0", transfer_fee: "1" },
  } as const;
  const html = renderToStaticMarkup(createElement(TradeConfirmationTable, { rows: [row], loading: false, error: null, onRefresh() {} }));
  assert.match(html, /2030-01-02/);
  assert.match(html, /600000/);
  assert.match(html, /买入/);
  assert.match(html, /成交额/);
  assert.match(html, /100股/);
  assert.match(html, /1000\.00元/);
  assert.match(html, /佣金/);
  assert.match(html, /印花税/);
  assert.match(html, /过户费/);
  assert.match(html, /5\.00元/);
});

test("交割单空状态和查询失败均显式展示，不用近期公开 Trade 填造", () => {
  const empty = renderToStaticMarkup(createElement(TradeConfirmationTable, { rows: [], loading: false, error: null, onRefresh() {} }));
  assert.match(empty, /暂无本人真实成交/);
  const failure = renderToStaticMarkup(createElement(TradeConfirmationTable, { rows: [], loading: false, error: "宿主查询失败", onRefresh() {} }));
  assert.match(failure, /宿主查询失败/);
  assert.match(failure, /role="alert"/);
});
