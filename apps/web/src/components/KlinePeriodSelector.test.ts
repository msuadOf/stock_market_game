import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createElement, type ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";

let vite: ViteDevServer;
let Selector: typeof import("./KlinePeriodSelector.tsx").KlinePeriodSelector;
before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  ({ KlinePeriodSelector: Selector } = await vite.ssrLoadModule("/src/components/KlinePeriodSelector.tsx"));
});
after(async () => { if (vite) await vite.close(); });

test("Desktop和Mobile同一个折叠周期表单包含所有已定周期，没有5/20交易日口径", { timeout: 10000 }, () => {
  const html = renderToStaticMarkup(createElement(Selector, { value: "季K", onChange() {} }));
  assert.match(html, /<details[^>]*><summary>更多周期/);
  for (const label of ["日K", "周K", "月K", "季K", "年K", "120分钟K", "60分钟K", "30分钟K", "15分钟K", "5分钟K", "1分钟K"]) assert.ok(html.includes(`>${label}</option>`), label);
  assert.doesNotMatch(html, /trading-days|交易日聚合|即将开放|disabled/);
  assert.match(html, /value="季K" selected/);
});

test("周期选择真实回调，拒绝未知输入而不回退日K", { timeout: 10000 }, () => {
  const selected: string[] = [];
  const root = Selector({ value: "日K", onChange: (period) => selected.push(period) });
  const label = (root.props.children as ReactElement[])[1];
  const select = (label.props.children as ReactElement[])[1];
  const change = select.props.onChange;
  change({ currentTarget: { value: "120分钟K" } });
  assert.deepEqual(selected, ["120分钟K"]);
  assert.throws(() => change({ currentTarget: { value: "5交易日K" } }), /未知/);
});
