import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";

let vite: ViteDevServer;
let DividendTaxPanel: typeof import("./DividendTaxPanel.tsx").DividendTaxPanel;
let outstandingText: typeof import("./DividendTaxPanel.tsx").outstandingText;
let stockStatusLabel: typeof import("./DividendTaxPanel.tsx").stockStatusLabel;

before(async () => {
  vite = await createServer({
    configFile: false,
    appType: "custom",
    server: { middlewareMode: true, ws: false },
    optimizeDeps: { noDiscovery: true },
  });
  const panel = await vite.ssrLoadModule("/src/components/company/DividendTaxPanel.tsx") as typeof import("./DividendTaxPanel.tsx");
  DividendTaxPanel = panel.DividendTaxPanel;
  outstandingText = panel.outstandingText;
  stockStatusLabel = panel.stockStatusLabel;
});

after(async () => {
  await vite.close();
});

const STATUS = {
  mode: "AShareIndividual" as const,
  identity: "Personal" as const,
  stocks: [{ stock: "600101", status: "IndividualPublicMarket" as const }],
};

test("未清税额恒为整数分时以元展示，非整数分数显式保留不取整", () => {
  assert.equal(outstandingText({ account: "0", stock: "600101", outstanding: { numerator: "12345", denominator: "1" }, needs_funds: false, cause: "Cleared" }), "123.45 元");
  assert.equal(outstandingText({ account: "0", stock: "600101", outstanding: { numerator: "7", denominator: "3" }, needs_funds: true, cause: "InsufficientAvailableCash" }), "7/3 分");
});

test("各证券税账状态按模式区分口径", () => {
  assert.equal(stockStatusLabel(STATUS, "600101"), "个人差别化税账已配置");
  assert.equal(stockStatusLabel(STATUS, "300260"), "无股东名册");
  assert.equal(stockStatusLabel({ ...STATUS, mode: "Exempt", stocks: [{ stock: "600101", status: "TreatmentNotConfigured" }] }, "600101"), "不扣税模式");
  assert.equal(stockStatusLabel({ ...STATUS, stocks: [{ stock: "600101", status: "TreatmentNotConfigured" }] }, "600101"), "未配置个人税账（不产生个人税事实）");
  assert.equal(
    stockStatusLabel({ ...STATUS, mode: "FlatWithholding", stocks: [{ stock: "600101", status: "FlatWithholding" }] }, "600101"),
    "简税代扣（付款日按开局比例直接扣）",
  );
});

test("宿主不支持查询时显式提示而非静默隐藏", () => {
  const markup = renderToStaticMarkup(createElement(DividendTaxPanel, { onQuery: undefined, refreshKey: "g1:2030-01-02" }));
  assert.match(markup, /当前宿主不支持股息税状态查询/);
});

test("支持查询的宿主在 SSR 初始态等待查询且错误通道保留", () => {
  const markup = renderToStaticMarkup(createElement(DividendTaxPanel, {
    onQuery: () => Promise.resolve({ status: STATUS, outstanding: [] }),
    refreshKey: "g1:2030-01-02",
  }));
  assert.match(markup, /股息税状态/);
  assert.doesNotMatch(markup, /当前宿主不支持/);
});
