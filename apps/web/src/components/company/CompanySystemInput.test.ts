import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";

let vite: ViteDevServer;
let CompanySystemInput: typeof import("./CompanySystemInput.tsx").CompanySystemInput;
before(async () => {
  vite = await createServer({ root: new URL("../../../", import.meta.url).pathname, configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  ({ CompanySystemInput } = await vite.ssrLoadModule("/src/components/company/CompanySystemInput.tsx") as typeof import("./CompanySystemInput.tsx"));
});
after(async () => { if (vite) await vite.close(); });
function render(value: string, seed = "42") {
  return renderToStaticMarkup(createElement(CompanySystemInput, { value, seed, origin: "custom", onChange() {}, onSeedChange() {}, onRegenerate() {}, onSettlementCycleChange() {} }));
}
test("Simple 配置非法与 Simulation 明确报错，不承诺价格倒推或已交付自动结算", () => {
  const simulation = render('{"mode":"Simulation"}');
  assert.match(simulation, /role="alert"/);
  assert.match(simulation, /Simulation 尚未实现/);
  assert.match(render('{"mode":"Simple","config":{}}'), /公司基本面设置无效/);
  assert.doesNotMatch(simulation, /PE|PB|以开局价格/);
  assert.match(simulation, /账面/);
  assert.match(simulation, /日终自动提案尚未接通/);
});
test("非法 u64 seed 可见且拒绝，JSON 错误保留 raw 草稿", () => {
  assert.match(render("unfinished {", "18446744073709551616"), /aria-invalid="true"/);
  assert.match(render("unfinished {"), /unfinished \{/);
  assert.match(render("unfinished {", "01"), /seed 必须/);
});
