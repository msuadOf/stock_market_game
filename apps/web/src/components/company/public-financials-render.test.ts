import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import { publicReportGold } from "./public-report-fixture.ts";
import { reportStatementRows } from "./company-presentation.ts";

let vite: ViteDevServer;
let notes: typeof import("./ReportNotes.tsx");
let table: typeof import("./FinancialStatementTable.tsx");

before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  notes = await vite.ssrLoadModule("/src/components/company/ReportNotes.tsx") as typeof notes;
  table = await vite.ssrLoadModule("/src/components/company/FinancialStatementTable.tsx") as typeof table;
});

after(async () => { if (vite) await vite.close(); });

test("附注渲染单体范围、更正原因与已公布行业科目明细", () => {
  const html = renderToStaticMarkup(createElement(notes.ReportNotes, { report: publicReportGold() }));
  assert.match(html, /单体.*C-600101/);
  assert.match(html, /已公布更正说明/);
  assert.match(html, /银行存款/);
  assert.match(html, /期初净借方/);
  assert.match(html, /年初至今净借方变动/);
  assert.match(html, /1000\.00/);
  assert.doesNotMatch(html, /公开摘要未提供|只显示已公开摘要/);
});

test("Simple 来源与仿真来源共用完整四表附注，不以模式隐藏功能", { timeout: 10000 }, () => {
  for (const source of ["SimpleGenerated", "SimulationAccounting"] as const) {
    const html = renderToStaticMarkup(createElement(notes.ReportNotes, { report: { ...publicReportGold(), source } }));
    assert.match(html, new RegExp(source));
    assert.match(html, /报表附注与口径/);
    assert.match(html, /已披露科目明细/);
    assert.doesNotMatch(html, /此模式不支持|未实现.*股东分配/);
  }
});

test("利润表渲染当季、累计、比较列，缺历史有原因不填零", () => {
  const statement = reportStatementRows(publicReportGold().financials)[1]!;
  const html = renderToStaticMarkup(createElement(table.FinancialStatementTable, { statement, exactAmountsVisible: true }));
  assert.match(html, /当季/);
  assert.match(html, /年初至今累计/);
  assert.match(html, /上年同期/);
  assert.match(html, /无上年历史/);
  assert.match(html, /营业收入/);
  assert.match(html, /200\.00/);
});
