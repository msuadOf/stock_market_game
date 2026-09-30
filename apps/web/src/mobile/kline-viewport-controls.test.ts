import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createElement, type ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import { MOBILE_KLINE_DEFAULT_CAPACITY, reduceKlineViewport, type KlineViewportAction } from "./market-model.ts";

let vite: ViteDevServer;
let KlineViewportControls: typeof import("./KlineViewportControls.tsx").KlineViewportControls;

before(async () => {
  vite = await createServer({
    configFile: false,
    appType: "custom",
    server: { middlewareMode: true, ws: false },
    optimizeDeps: { noDiscovery: true },
  });
  ({ KlineViewportControls } = await vite.ssrLoadModule("/src/mobile/KlineViewportControls.tsx") as typeof import("./KlineViewportControls.tsx"));
});

after(async () => {
  if (vite) await vite.close();
});

test("K 线默认最新窗口不可复位，放大、缩小或左移后可复位", () => {
  const total = 180;
  const initial = { capacity: MOBILE_KLINE_DEFAULT_CAPACITY, offsetFromEnd: 0 };
  const props = { total, viewport: initial, onAction() {} };
  const initialHtml = renderToStaticMarkup(createElement(KlineViewportControls, props));
  assert.match(initialHtml, /aria-label="复位K线窗口"[^>]*disabled=""/);
  for (const action of ["zoom-in", "zoom-out", "pan-left"] as const) {
    const viewport = reduceKlineViewport(initial, total, action);
    const html = renderToStaticMarkup(createElement(KlineViewportControls, { ...props, viewport }));
    assert.match(html, /aria-label="复位K线窗口"/);
    assert.doesNotMatch(html, /aria-label="复位K线窗口"[^>]*disabled/);
    assert.deepEqual(reduceKlineViewport(viewport, total, "reset"), initial);
  }
});

test("K 线控件交付真实 reset 操作，并在最大缩小时仍可复位", () => {
  const actions: KlineViewportAction[] = [];
  const element = KlineViewportControls({
    total: 180,
    viewport: { capacity: 120, offsetFromEnd: 0 },
    onAction: (action) => { actions.push(action); },
  });
  const buttons = element.props.children as ReactElement<{ "aria-label": string; disabled: boolean; onClick: () => void }>[];
  const reset = buttons.find((button) => button.props["aria-label"] === "复位K线窗口");
  const zoomOut = buttons.find((button) => button.props["aria-label"] === "缩小K线");
  assert.ok(reset);
  assert.ok(zoomOut);
  assert.equal(zoomOut.props.disabled, true);
  assert.equal(reset.props.disabled, false);
  reset.props.onClick();
  assert.deepEqual(actions, ["reset"]);
});
