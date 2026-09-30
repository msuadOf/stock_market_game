import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createElement, type ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";

let vite: ViteDevServer;
let views: typeof import("./HostStatusViews.tsx");

before(async () => {
  vite = await createServer({
    configFile: false,
    appType: "custom",
    server: { middlewareMode: true, ws: false },
    optimizeDeps: { noDiscovery: true },
  });
  views = await vite.ssrLoadModule("/src/app/HostStatusViews.tsx") as typeof views;
});

after(async () => {
  if (vite) await vite.close();
});

test("致命错误显示完整上下文并提供可调用的恢复操作", () => {
  let retries = 0;
  const props = {
    error: "协议错误 SequenceGap @ remote.ws: 缺少事件 12",
    onRetry: () => { retries += 1; },
  };
  const html = renderToStaticMarkup(createElement(views.FatalHostError, props));
  assert.match(html, /role="alert" aria-live="assertive"/);
  assert.match(html, /行情引擎或协议无法继续/);
  assert.match(html, /<pre>协议错误 SequenceGap @ remote\.ws: 缺少事件 12<\/pre>/);
  assert.match(html, /刷新页面重试/);
  const children = views.FatalHostError(props).props.children as ReactElement<{ onClick?: () => void }>[];
  const retry = children.find((child) => typeof child.props.onClick === "function");
  assert.ok(retry);
  retry.props.onClick!();
  assert.equal(retries, 1);
});

test("倍速采样失败明确显示原始原因，正常状态不渲染错误", () => {
  assert.equal(renderToStaticMarkup(createElement(views.SpeedMetricsAlert, { error: null })), "");
  const html = renderToStaticMarkup(createElement(views.SpeedMetricsAlert, {
    error: "实际倍速读取失败：worker 请求 17 超时",
  }));
  assert.match(html, /class="speed-metrics-error" role="alert"/);
  assert.match(html, /实际倍速读取失败：worker 请求 17 超时/);
});

test("Publisher 控件只展示宿主提供的模式并将操作交付上层回调", () => {
  const changes: string[] = [];
  const props = {
    mode: "push" as const,
    modes: ["push", "pull"] as const,
    labels: { push: "推送", pull: "拉取" },
    onChange: (mode: string) => { changes.push(mode); },
  };
  assert.equal(renderToStaticMarkup(createElement(views.DeliveryModeControl, { ...props, mode: null })), "");
  assert.equal(renderToStaticMarkup(createElement(views.DeliveryModeControl, { ...props, modes: [] })), "");
  const html = renderToStaticMarkup(createElement(views.DeliveryModeControl, props));
  assert.match(html, /aria-label="客户端 Publisher 刷新模式"/);
  assert.match(html, /<option label="推送" value="push" selected="">推送<\/option>/);
  assert.match(html, /<option label="拉取" value="pull">拉取<\/option>/);
  const singleMode = renderToStaticMarkup(createElement(views.DeliveryModeControl, { ...props, modes: ["push"] }));
  assert.doesNotMatch(singleMode, /value="pull"/);
  const control = views.DeliveryModeControl(props);
  assert.ok(control);
  const select = control.props.children[1];
  select.props.onChange({ target: { value: "pull" } });
  assert.deepEqual(changes, ["pull"]);
});
