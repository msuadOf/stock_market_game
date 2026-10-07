import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";

let vite: ViteDevServer;
let PreferenceRejectionsPanel: typeof import("./PreferenceRejectionsPanel.tsx").PreferenceRejectionsPanel;
let rejectionKindLabel: typeof import("./PreferenceRejectionsPanel.tsx").rejectionKindLabel;
let rejectionSummary: typeof import("./PreferenceRejectionsPanel.tsx").rejectionSummary;

before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  const panel = await vite.ssrLoadModule("/src/components/company/PreferenceRejectionsPanel.tsx") as typeof import("./PreferenceRejectionsPanel.tsx");
  PreferenceRejectionsPanel = panel.PreferenceRejectionsPanel;
  rejectionKindLabel = panel.rejectionKindLabel;
  rejectionSummary = panel.rejectionSummary;
});

after(async () => { if (vite) await vite.close(); });

const REJECTION = {
  company: "C-600101",
  evaluated_on: "2030-01-31",
  kind: "CashDividend" as const,
  detail: "可分配利润 120.00 元低于门槛 500.00 元，不提案",
};

test("偏好提案类别使用中文标签且覆盖两类别", () => {
  assert.equal(rejectionKindLabel("CashDividend"), "现金分红");
  assert.equal(rejectionKindLabel("StockDistribution"), "送转");
});

test("拒绝台账摘要保留评估日与拒绝原因原文", () => {
  assert.equal(rejectionSummary(REJECTION), "2030-01-31 现金分红提案被拒：可分配利润 120.00 元低于门槛 500.00 元，不提案");
});

test("宿主不支持查询时显式提示而非静默隐藏", () => {
  const markup = renderToStaticMarkup(createElement(PreferenceRejectionsPanel, { companyId: "C-600101", onQuery: undefined, refreshKey: "g1:2030-01-02" }));
  assert.match(markup, /偏好提案拒绝台账/);
  assert.match(markup, /当前宿主不支持偏好拒绝台账查询/);
});

test("支持查询的宿主在 SSR 初始态等待查询且不误报不支持", () => {
  const markup = renderToStaticMarkup(createElement(PreferenceRejectionsPanel, {
    companyId: "C-600101",
    onQuery: () => Promise.resolve([REJECTION]),
    refreshKey: "g1:2030-01-02",
  }));
  assert.match(markup, /偏好提案拒绝台账/);
  assert.doesNotMatch(markup, /当前宿主不支持/);
});
