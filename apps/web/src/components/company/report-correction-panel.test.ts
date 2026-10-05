import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import type { ReportCorrectionPanel as Panel } from "./ReportCorrectionPanel.tsx";

let vite: ViteDevServer;
let ReportCorrectionPanel: typeof Panel;
before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  ({ ReportCorrectionPanel } = await vite.ssrLoadModule("/src/components/company/ReportCorrectionPanel.tsx") as typeof import("./ReportCorrectionPanel.tsx"));
});
after(async () => { await vite?.close(); });

test("真实更正组件明确市场控制、日终公开和非注资边界，不填造经济凭证", { timeout: 10000 }, () => {
  const html = renderToStaticMarkup(createElement(ReportCorrectionPanel, { companyId: "C-600000", reportId: "2", control: { submitReportCorrection: async () => {}, cancelReportCorrection: async () => {}, queryReportCorrections: async () => ({ pending: [], completed: {} }) } }));
  for (const text of ["市场控制", "日终", "失败", "换档", "AccountingAmount", "元", "不是注资", "JournalEntry", "取消", "已完成"]) assert.ok(html.includes(text), text);
  assert.ok(html.includes("</textarea>"));
  assert.ok(!html.includes("CashRevenue"));
  assert.ok(!html.includes("10.00"));
});
