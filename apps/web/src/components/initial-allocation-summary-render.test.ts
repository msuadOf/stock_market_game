import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";

let vite: ViteDevServer;
let InitialAllocationSummary: typeof import("./InitialAllocationSummary.tsx").InitialAllocationSummary;
before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  ({ InitialAllocationSummary } = await vite.ssrLoadModule("/src/components/InitialAllocationSummary.tsx") as typeof import("./InitialAllocationSummary.tsx"));
});
after(async () => { if (vite) await vite.close(); });

test("实际分配显示真实整数余股占比、零持股人数及流通盘对账", { timeout: 10000 }, () => {
  const markup = renderToStaticMarkup(createElement(InitialAllocationSummary, { allocation: {
    stocks: [{ code: "600101", float_shares: 7, unallocated_shares: 0, categories: [
      { kind: "Retail", shares: 3, account_count: 5, zero_holders: 2 },
      { kind: "Inst", shares: 4, account_count: 1, zero_holders: 0 },
      { kind: "Hot", shares: 0, account_count: 0, zero_holders: 0 },
    ] }],
  } }));
  assert.match(markup, /600101/);
  assert.match(markup, /42\.86%/);
  assert.match(markup, /57\.14%/);
  assert.match(markup, /<td>5<\/td><td>2<\/td>/);
  assert.match(markup, /整数对账一致/);
  assert.match(markup, /不是预计比例/);
  const restored = renderToStaticMarkup(createElement(InitialAllocationSummary, { allocation: null }));
  assert.match(restored, /现有持仓不能冒充初始持仓/);
  assert.doesNotMatch(restored, /整数对账一致/);
});
