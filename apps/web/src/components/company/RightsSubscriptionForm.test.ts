import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
let vite: ViteDevServer;
let form: typeof import("./RightsSubscriptionForm.tsx");
before(async () => {
  vite = await createServer({ configFile: false, root: new URL("../../../", import.meta.url).pathname, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  form = await vite.ssrLoadModule("/src/components/company/RightsSubscriptionForm.tsx") as typeof form;
});
after(async () => { await vite.close(); });
const view = { event_id: "event-1", payment_window: "Open", owner_entitlement: { rights_shares: "10", lock_until: null }, open_subscription_remaining_shares: null, queued_subscription: null, settled_subscription: null } as const;
test("认购数量严格使用正数及 u64 十进制股数", { timeout: 10000 }, () => {
  for (const value of ["1", "18446744073709551615"]) assert.equal(form.validSubscriptionShares(value), true);
  for (const value of ["", "0", "01", "-1", "1.0", " 1", "18446744073709551616"]) assert.equal(form.validSubscriptionShares(value), false);
});
test("仅开放且本人有额度、尚无认购记录时可提交", { timeout: 10000 }, () => {
  assert.equal(form.canSubscribe(view), true);
  assert.equal(form.canSubscribe({ ...view, payment_window: "Closed" }), false);
  assert.equal(form.canSubscribe({ ...view, owner_entitlement: null }), false);
  assert.equal(form.canSubscribe({ ...view, owner_entitlement: null, open_subscription_remaining_shares: "0" }), false);
  assert.equal(form.canSubscribe({ ...view, owner_entitlement: null, open_subscription_remaining_shares: "5" }), true);
  assert.equal(form.canSubscribe({ ...view, queued_subscription: { requested_shares: "1", submitted_on: "2030-01-02" } }), false);
  assert.equal(form.canSubscribe({ ...view, settled_subscription: { requested_shares: "1", paid_shares: "1", paid_amount: "1", waived_shares: "0" } }), false);
});
test("不支持认购的宿主明确展示原因且无提交按钮", { timeout: 10000 }, () => {
  const html = renderToStaticMarkup(createElement(form.RightsSubscriptionForm, { view, onSubscribe: undefined, onAccepted: () => {} }));
  assert.match(html, /当前宿主不支持配股认购/);
  assert.doesNotMatch(html, /提交认购/);
});
test("受理并非缴款，初始数量为空", { timeout: 10000 }, () => {
  const html = renderToStaticMarkup(createElement(form.RightsSubscriptionForm, { view, onSubscribe: async () => {}, onAccepted: () => {} }));
  assert.match(html, /提交认购/);
  assert.match(html, /排队受理不代表缴款成功/);
  assert.match(html, /disabled/);
});
