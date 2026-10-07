import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";

let vite: ViteDevServer;
let CompanyPreferencesInput: typeof import("./CompanyPreferencesInput.tsx").CompanyPreferencesInput;
let cashPreferenceFromFields: typeof import("./CompanyPreferencesInput.tsx").cashPreferenceFromFields;
let stockPreferenceFromFields: typeof import("./CompanyPreferencesInput.tsx").stockPreferenceFromFields;
let centsToYuanText: typeof import("./CompanyPreferencesInput.tsx").centsToYuanText;
type CompanyPreferencesCompany = import("./CompanyPreferencesInput.tsx").CompanyPreferencesCompany;

before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  const module = await vite.ssrLoadModule("/src/components/company/CompanyPreferencesInput.tsx") as typeof import("./CompanyPreferencesInput.tsx");
  CompanyPreferencesInput = module.CompanyPreferencesInput;
  cashPreferenceFromFields = module.cashPreferenceFromFields;
  stockPreferenceFromFields = module.stockPreferenceFromFields;
  centsToYuanText = module.centsToYuanText;
});

after(async () => { if (vite) await vite.close(); });

const COMPANY: CompanyPreferencesCompany = {
  company: "C-600101",
  kind: "Industrial",
  generation: {} as CompanyPreferencesCompany["generation"],
  finance: {} as CompanyPreferencesCompany["finance"],
  preferences: {
    cash_dividend: { target_payout_bp: 3000, min_distributable_profit: "50000000", cycles_between_proposals: 2 },
    stock_distribution: null,
  },
};

test("元↔分换算保持两位小数且拒绝非元格式输入", () => {
  assert.equal(centsToYuanText("50000000"), "500000.00");
  assert.equal(centsToYuanText("5"), "0.05");
  assert.equal(centsToYuanText("0"), "0.00");
});

test("现金分红偏好由表单字段构造并显式拒绝越域值", () => {
  assert.deepEqual(cashPreferenceFromFields("3000", "500000.00", "2"), {
    ok: true,
    value: { target_payout_bp: 3000, min_distributable_profit: "50000000", cycles_between_proposals: 2 },
  });
  assert.equal(cashPreferenceFromFields("0", "500000.00", "2").ok, false);
  assert.equal(cashPreferenceFromFields("10001", "500000.00", "2").ok, false);
  assert.equal(cashPreferenceFromFields("3000", "0", "2").ok, false);
  assert.equal(cashPreferenceFromFields("3000", "500000.005", "2").ok, false);
  assert.equal(cashPreferenceFromFields("3000", "-1", "2").ok, false);
  assert.equal(cashPreferenceFromFields("3000", "500000.00", "0").ok, false);
  assert.equal(cashPreferenceFromFields("abc", "500000.00", "2").ok, false);
});

test("送转偏好由表单字段构造并显式拒绝越域值", () => {
  assert.deepEqual(stockPreferenceFromFields("500000.00", "100000", "5000000", "4"), {
    ok: true,
    value: { min_distributable_profit: "50000000", shares_per_existing_share_micros: 100000, max_cumulative_expansion_micros: 5000000, cycles_between_proposals: 4 },
  });
  assert.equal(stockPreferenceFromFields("0", "100000", "5000000", "4").ok, false);
  assert.equal(stockPreferenceFromFields("500000.00", "0", "5000000", "4").ok, false);
  assert.equal(stockPreferenceFromFields("500000.00", "10000001", "5000000", "4").ok, false);
  assert.equal(stockPreferenceFromFields("500000.00", "100000", "1000000001", "4").ok, false);
  assert.equal(stockPreferenceFromFields("500000.00", "100000", "5000000", "x").ok, false);
});

test("偏好编辑器如实呈现未配置=不自动产生方案的语义与既有配置", () => {
  const markup = renderToStaticMarkup(createElement(CompanyPreferencesInput, {
    companies: [COMPANY, { ...COMPANY, company: "C-600102", preferences: { cash_dividend: null, stock_distribution: null } }],
    onApply: () => { throw new Error("SSR 不触发回调"); },
  }));
  assert.match(markup, /C-600101/);
  assert.match(markup, /C-600102/);
  assert.match(markup, /未配置.*不自动产生.*方案/);
  assert.match(markup, /仅对新游戏生效/);
  assert.match(markup, /value="3000"/);
  assert.match(markup, /value="500000\.00"/);
  // 两类偏好各公司都有独立启用开关。
  assert.equal(markup.match(/启用现金分红自动提案/g)?.length, 2);
  assert.equal(markup.match(/启用送转自动提案/g)?.length, 2);
});

test("配置 JSON 无效时显式提示不可编辑而非静默隐藏", () => {
  const markup = renderToStaticMarkup(createElement(CompanyPreferencesInput, { companies: null, onApply: () => undefined }));
  assert.match(markup, /偏好编辑暂不可用/);
  assert.match(markup, /JSON 当前无效.*请先修正/);
});
