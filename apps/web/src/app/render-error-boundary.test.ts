import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import { readFileSync } from "node:fs";

let vite: ViteDevServer;
let RenderErrorBoundary: typeof import("./RenderErrorBoundary.tsx").RenderErrorBoundary;
before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  ({ RenderErrorBoundary } = await vite.ssrLoadModule("/src/app/RenderErrorBoundary.tsx") as typeof import("./RenderErrorBoundary.tsx"));
});
after(async () => { if (vite) await vite.close(); });

test("审计G50：React错误状态实际展示原因、位置、复制反馈及日内进度警告", () => {
  const boundary = new RenderErrorBoundary({ children: createElement("span", null, "健康界面") });
  assert.equal(renderToStaticMarkup(boundary.render()), "<span>健康界面</span>");
  boundary.state = RenderErrorBoundary.getDerivedStateFromError(new Error("控件布局失败 token=private-secret"));
  const html = renderToStaticMarkup(boundary.render());
  assert.match(html, /role="alert"/);
  assert.match(html, /界面无法继续显示/);
  assert.match(html, /UI_RENDER_FAILED/);
  assert.match(html, /render-app.React/);
  assert.match(html, /控件布局失败/);
  assert.match(html, /复制错误反馈/);
  assert.match(html, /未保存的日内进度/);
  assert.doesNotMatch(html, /private-secret|健康界面|行情引擎或协议无法继续/);
});

test("审计G50：非Error异常也有明确出口，真实root在Provider与App外包裹boundary", () => {
  const boundary = new RenderErrorBoundary({ children: null });
  boundary.state = RenderErrorBoundary.getDerivedStateFromError({ explanation: "异常对象" });
  assert.match(renderToStaticMarkup(boundary.render()), /非 Error 异常/);
  const source = readFileSync(new URL("../render-app.tsx", import.meta.url), "utf8");
  assert.ok(source.indexOf("<RenderErrorBoundary>") < source.indexOf("<Provider"));
  assert.ok(source.indexOf("</RenderErrorBoundary>") > source.indexOf("</Provider>"));
});

test("审计G50：非法 Error.message 显式诊断且保留原 cause", () => {
  const error = new Error("原消息");
  Object.defineProperty(error, "message", { value: 42 });
  const boundary = new RenderErrorBoundary({ children: null });
  boundary.state = RenderErrorBoundary.getDerivedStateFromError(error);
  assert.equal(boundary.state.failure?.cause, error);
  assert.match(renderToStaticMarkup(boundary.render()), /message 不是字符串/);
});

test("审计G50：异常 message 访问器不执行且 fallback 仍可显示", () => {
  let reads = 0;
  const error = new Error("原消息");
  Object.defineProperty(error, "message", { get() { reads += 1; throw new Error("getter 失败"); } });
  const boundary = new RenderErrorBoundary({ children: null });
  boundary.state = RenderErrorBoundary.getDerivedStateFromError(error);
  assert.equal(boundary.state.failure?.cause, error);
  assert.match(renderToStaticMarkup(boundary.render()), /message 是访问器/);
  assert.equal(reads, 0);
});
