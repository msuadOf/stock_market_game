import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
let vite: ViteDevServer;
let component: typeof import("./RightsSubscriptionRejectionPanel.tsx");
before(async () => {
  vite = await createServer({ configFile: false, root: new URL("../../../", import.meta.url).pathname, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  component = await vite.ssrLoadModule("/src/components/company/RightsSubscriptionRejectionPanel.tsx") as typeof component;
});
after(async () => { await vite.close(); });
test("SSR保持未查询状态并且零宿主调用", () => {
  let calls = 0;
  const query = async (): Promise<never> => { calls++; throw new Error("SSR不应调用宿主"); };
  const html = renderToStaticMarkup(createElement(component.RightsSubscriptionRejectionPanel, { refreshKey: "g1", onQuery: query }));
  assert.match(html, /尚未查询配股认购拒绝回执/); assert.doesNotMatch(html, /本人没有配股认购拒绝回执/);
  assert.equal(calls, 0);
});
test("缺少callback明确显示当前宿主不支持", () => {
  const html = renderToStaticMarkup(createElement(component.RightsSubscriptionRejectionPanel, { refreshKey: "g1", onQuery: undefined }));
  assert.match(html, /当前宿主不支持/);
});
