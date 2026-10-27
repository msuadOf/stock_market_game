import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
let vite: ViteDevServer;
let component: typeof import("./CompanyContractPanel.tsx");
before(async () => {
  vite = await createServer({ configFile: false, root: new URL("../../../", import.meta.url).pathname, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  component = await vite.ssrLoadModule("/src/components/company/CompanyContractPanel.tsx") as typeof component;
});
after(async () => { await vite.close(); });
test("SSR保持未查询状态并且零宿主调用", () => {
  let calls = 0;
  const query = async (): Promise<never> => { calls++; throw new Error("SSR不应调用宿主"); };
  const props = { companyId: 2, refreshKey: "g1", onCapabilitiesQuery: query, onOwnerRightsQuery: query, onExplanationQuery: query, onFlatReceiptsQuery: query };
  const html = renderToStaticMarkup(createElement(component.CompanyContractPanel, props));
  assert.match(html, /尚未查询公司能力面/); assert.match(html, /尚未查询期间解释/);
  assert.doesNotMatch(html, /提交认购/);
  assert.equal(calls, 0);
});
test("缺少callback明确显示当前宿主不支持", () => {
  const html = renderToStaticMarkup(createElement(component.CompanyContractPanel, { companyId: 2, refreshKey: "g1", onCapabilitiesQuery: undefined, onOwnerRightsQuery: undefined, onExplanationQuery: undefined, onFlatReceiptsQuery: undefined }));
  assert.match(html, /当前宿主不支持/);
});
