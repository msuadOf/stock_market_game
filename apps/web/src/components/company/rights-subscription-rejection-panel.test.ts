import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";

let vite: ViteDevServer;
let RightsSubscriptionRejectionPanel: typeof import("./RightsSubscriptionRejectionPanel.tsx").RightsSubscriptionRejectionPanel;
let rejectionSummary: typeof import("./RightsSubscriptionRejectionPanel.tsx").rejectionSummary;

before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  const panel = await vite.ssrLoadModule("/src/components/company/RightsSubscriptionRejectionPanel.tsx") as typeof import("./RightsSubscriptionRejectionPanel.tsx");
  RightsSubscriptionRejectionPanel = panel.RightsSubscriptionRejectionPanel;
  rejectionSummary = panel.rejectionSummary;
});

after(async () => { if (vite) await vite.close(); });

const RECEIPT = {
  event_id: "rights-event",
  account: "0",
  requested_shares: "41",
  submitted_on: "2030-01-07",
  rejected_on: "2030-01-07",
  reason: "公开配售剩余额度 40 股，申请 41 股超出额度",
};

test("拒绝回执摘要保留事件、数量与原因原文，不截断不静默", () => {
  assert.equal(rejectionSummary(RECEIPT), "rights-event：申请 41 股被拒（2030-01-07 提交、2030-01-07 拒绝）——公开配售剩余额度 40 股，申请 41 股超出额度");
});

test("宿主不支持查询时显式提示而非静默隐藏", () => {
  const markup = renderToStaticMarkup(createElement(RightsSubscriptionRejectionPanel, { onQuery: undefined, refreshKey: "g1:2030-01-02" }));
  assert.match(markup, /配股认购拒绝回执/);
  assert.match(markup, /当前宿主不支持配股认购拒绝回执查询/);
});

test("支持查询的宿主在 SSR 初始态等待查询且不误报不支持", () => {
  const markup = renderToStaticMarkup(createElement(RightsSubscriptionRejectionPanel, {
    onQuery: () => Promise.resolve([RECEIPT]),
    refreshKey: "g1:2030-01-02",
  }));
  assert.match(markup, /配股认购拒绝回执/);
  assert.doesNotMatch(markup, /当前宿主不支持/);
});
