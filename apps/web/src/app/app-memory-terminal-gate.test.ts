import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { after, before, test } from "node:test";
import * as React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import ts from "typescript";
import { createServer, type ViteDevServer } from "vite";
import type { HostFailure } from "../host/host-update.ts";

let vite: ViteDevServer;
let terminal: typeof import("./WasmMemoryTerminalError.tsx");
let host: typeof import("./HostStatusViews.tsx");
before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  terminal = await vite.ssrLoadModule("/src/app/WasmMemoryTerminalError.tsx") as typeof terminal;
  host = await vite.ssrLoadModule("/src/app/HostStatusViews.tsx") as typeof host;
});
after(async () => { if (vite) await vite.close(); });

// 执行 AppShell 真实 early-return AST，避免为了错误页启动完整行情与宿主副作用。
function renderError(error: HostFailure | string, returningToStartup = false, memoryHandoverPanel: unknown = null): string {
  const file = ts.createSourceFile("App.tsx", readFileSync(new URL("../App.tsx", import.meta.url), "utf8"), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const shell = file.statements.find((node): node is ts.FunctionDeclaration => ts.isFunctionDeclaration(node) && node.name?.text === "AppShell");
  assert.ok(shell?.body);
  const statements = shell.body.statements;
  const start = statements.findIndex((node) => ts.isVariableStatement(node) && node.declarationList.declarations.some((entry) => ts.isIdentifier(entry.name) && entry.name.text === "selectHost"));
  const end = statements.findIndex((node) => ts.isVariableStatement(node) && node.declarationList.declarations.some((entry) => ts.isIdentifier(entry.name) && entry.name.text === "currentSpeedMetrics"));
  assert.ok(start >= 0 && end > start);
  const body = statements.slice(start + 1, end).map((node) => node.getText(file)).join("\n");
  const compiled = ts.transpileModule(`function render() { ${body} }`, { compilerOptions: { jsx: ts.JsxEmit.React, target: ts.ScriptTarget.ES2023 } }).outputText;
  const forbidden = () => assert.fail("渲染错误页不得调用恢复动作");
  const environment = {
    React, ...terminal, FatalHostError: host.FatalHostError,
    Button: ({ children, onClick }: { children: React.ReactNode; onClick: () => void }) => React.createElement("button", { onClick }, children),
    error, returningToStartup, startupReturnError: null, notice: null,
    memoryHandoverPanel, MemoryHandoverPanel: () => React.createElement("div", null, "不应显示交接面板"),
    ready: false, hostBaselineReady: false, hasSnapshot: false,
    selectHost: forbidden, handleNewGame: forbidden, recoverFromFile: forbidden,
    window: { location: { reload: forbidden } },
  };
  const view = new Function(...Object.keys(environment), `${compiled}\nreturn render();`)(...Object.values(environment)) as React.ReactElement;
  return renderToStaticMarkup(view);
}

for (const code of ["MEMORY_RESIDUAL_EXHAUSTED", "MEMORY_HANDOVER_CORRUPT"]) {
  test(`App实际错误分支对${code}只留终局面，不暴露同链恢复入口`, () => {
    const html = renderError({ code, where: "wasm-memory-storage.recovery", message: "真实失败", recoverable: true, recoveryActions: ["创建新游戏"] });
    assert.match(html, /全新标签页手动打开原网址/);
    assert.match(html, /不可直接恢复/);
    assert.match(html, /不要使用浏览器恢复或复制标签页/);
    assert.equal((html.match(/<button\b/g) || []).length, 1);
    assert.doesNotMatch(html, /宿主启动恢复|存档错误恢复|>刷新页面重试<|>返回启动选择<|>创建新游戏<|>选择其他日终存档</);
  });
}
test("终局优先于重选进行中，保留明确终局详情", () => {
  const html = renderError({ code: "MEMORY_HANDOVER_CORRUPT", where: "memory.recovery", message: "交接记录校验失败" }, true);
  assert.match(html, /内存残留账无法安全恢复/);
  assert.match(html, /交接记录校验失败/);
  assert.doesNotMatch(html, /正在结束当前启动/);
});
test("终局guard高于非空交接面板，不显示继续/别档/新局/取消入口", () => {
  const html = renderError(
    { code: "MEMORY_HANDOVER_CORRUPT", where: "memory.recovery", message: "交接记录校验失败" },
    false,
    { kind: "frozen" },
  );
  assert.match(html, /内存残留账无法安全恢复/);
  assert.match(html, /全新标签页手动打开原网址/);
  assert.doesNotMatch(html, /不应显示交接面板|>继续<|>选择其他日终存档<|>创建新游戏<|>取消</);
  assert.equal((html.match(/<button\b/g) || []).length, 1);
});
test("其他结构化或字符串错误保留既有明确恢复入口", () => {
  for (const error of ["普通启动失败", { code: "MEMORY_MEASUREMENT_NOT_READY", where: "memory.measurement", message: "缺少真实测量" }]) {
    const html = renderError(error);
    assert.match(html, /刷新页面重试/); assert.match(html, /返回启动选择/);
    assert.match(html, /创建新游戏/); assert.match(html, /选择其他日终存档/);
    assert.equal((html.match(/<button\b/g) || []).length, 5);
  }
});
