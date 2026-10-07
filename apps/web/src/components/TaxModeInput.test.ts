import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";

let vite: ViteDevServer;
let TaxModeInput: typeof import("./TaxModeInput.tsx").TaxModeInput;
let MAX_FLAT_WITHHOLDING_BP: typeof import("./TaxModeInput.tsx").MAX_FLAT_WITHHOLDING_BP;

before(async () => {
  vite = await createServer({
    configFile: false,
    appType: "custom",
    server: { middlewareMode: true, ws: false },
    optimizeDeps: { noDiscovery: true },
  });
  const mod = await vite.ssrLoadModule("/src/components/TaxModeInput.tsx") as typeof import("./TaxModeInput.tsx");
  TaxModeInput = mod.TaxModeInput;
  MAX_FLAT_WITHHOLDING_BP = mod.MAX_FLAT_WITHHOLDING_BP;
});

after(async () => {
  await vite.close();
});

const NOOP = () => {};

/** 三层税制（2026-10-08 决策）的新局选项面：三选一（默认简税）、
 * 比例输入仅在简税时出现、非法比例不写草稿。 */
test("三层税务模式选项默认勾选简税并展示比例输入", () => {
  const markup = renderToStaticMarkup(createElement(TaxModeInput, {
    value: "FlatWithholding",
    onChange: NOOP,
    flatWithholdingBp: 1000,
    onFlatWithholdingBpChange: NOOP,
  }));
  assert.match(markup, /简税（分红到账时按比例直接扣，默认）/);
  assert.match(markup, /大 A 方式（个人差别化计税）/);
  assert.match(markup, /不扣税（连印花税也免）/);
  assert.match(markup, /代扣比例/);
  assert.match(markup, /1000bp/);
  const flatChecked = markup.match(/value="FlatWithholding"[^>]*checked/m) ?? markup.match(/checked[^>]*value="FlatWithholding"/m);
  assert.notEqual(flatChecked, null, "简税默认勾选");
});

test("非简税模式不展示比例输入但保留三选一", () => {
  for (const mode of ["AShareIndividual", "Exempt"] as const) {
    const markup = renderToStaticMarkup(createElement(TaxModeInput, {
      value: mode,
      onChange: NOOP,
      flatWithholdingBp: 1000,
      onFlatWithholdingBpChange: NOOP,
    }));
    assert.doesNotMatch(markup, /代扣比例/);
    assert.match(markup, /简税（分红到账时按比例直接扣，默认）/);
  }
});

test("比例上界常量与引擎契约一致", () => {
  assert.equal(MAX_FLAT_WITHHOLDING_BP, 10000);
});
