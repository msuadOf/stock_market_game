import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { fileURLToPath } from "node:url";
import { createElement, type ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import ts from "typescript";
import { createServer, type ViteDevServer } from "vite";
import type { ResponsiveGridLayoutProps } from "react-grid-layout";
import { createWorkspaceLayouts, WORKSPACE_PANEL_IDS, type WorkspaceBreakpoint } from "./workspace-layout.ts";

let vite: ViteDevServer;
let views: typeof import("./WorkspaceGrid.tsx");

before(async () => {
  vite = await createServer({
    configFile: false,
    appType: "custom",
    server: { middlewareMode: true, ws: false },
    optimizeDeps: { noDiscovery: true },
  });
  views = await vite.ssrLoadModule("/src/app/WorkspaceGrid.tsx") as typeof views;
});

after(async () => { if (vite) await vite.close(); });

function panels() {
  return WORKSPACE_PANEL_IDS.map((id) => createElement("article", { key: id, id, className: "panel" },
    createElement("input", { "aria-label": `${id}输入`, defaultValue: "100" }),
    createElement("canvas", { "aria-label": `${id}图表` }),
  ));
}

test("React 18 容器 ref 与桌面组件 createElement 接线通过类型校验", () => {
  const configPath = fileURLToPath(new URL("../../tsconfig.app.json", import.meta.url));
  const config = ts.readConfigFile(configPath, ts.sys.readFile);
  if (config.error) throw new Error(ts.flattenDiagnosticMessageText(config.error.messageText, "\n"));
  const parsed = ts.parseJsonConfigFileContent(config.config, ts.sys, fileURLToPath(new URL("../../", import.meta.url)));
  const program = ts.createProgram({
    rootNames: [
      fileURLToPath(new URL("./WorkspaceGrid.tsx", import.meta.url)),
      fileURLToPath(import.meta.url),
    ],
    options: { ...parsed.options, incremental: false, noEmit: true },
  });
  const diagnostics = [...parsed.errors, ...ts.getPreEmitDiagnostics(program)];
  assert.deepEqual(diagnostics.map((diagnostic) => ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n")), []);
});

test("独立旧工作台组件的六个面板都提供显式标题拖柄和缩放柄，子内容原样保留", () => {
  const html = renderToStaticMarkup(createElement(views.WorkspaceDesktopLayout, { width: 1400, layouts: createWorkspaceLayouts(), onLayoutChange: () => undefined }, panels()));
  assert.equal((html.match(/class="workspace-drag-handle"/g) ?? []).length, 6);
  assert.equal((html.match(/react-resizable-handle-se/g) ?? []).length, 6);
  assert.match(html, /拖动行情面板/);
  for (const id of WORKSPACE_PANEL_IDS) {
    assert.match(html, new RegExp(`id="${id}"`));
    assert.match(html, new RegExp(`aria-label="${id}输入" value="100"`));
    assert.match(html, new RegExp(`aria-label="${id}图表"`));
  }
});

test("桌面库配置只允许标题拖动，不抢正文、输入与图表手势；更新交给布局回调", () => {
  const layouts = createWorkspaceLayouts();
  const changes: unknown[] = [];
  const grid = views.WorkspaceDesktopLayout({
    width: 1000,
    layouts,
    children: panels(),
    onLayoutChange: (layout, nextLayouts) => changes.push([layout, nextLayouts]),
  }) as ReactElement<ResponsiveGridLayoutProps<WorkspaceBreakpoint>>;
  assert.equal(grid.props.width, 1000);
  assert.equal(grid.props.dragConfig?.enabled, true);
  assert.equal(grid.props.dragConfig?.handle, ".workspace-drag-handle");
  assert.ok(grid.props.dragConfig?.cancel?.includes(".workspace-panel-body"));
  assert.equal(grid.props.resizeConfig?.enabled, true);
  assert.deepEqual(grid.props.resizeConfig?.handles, ["se"]);
  const changed = layouts.medium.map((item) => ({ ...item, y: item.y + 1, h: item.h + 1 }));
  const nextLayouts = { ...layouts, medium: changed };
  grid.props.onLayoutChange!(changed, nextLayouts);
  assert.deepEqual(changes, [[changed, nextLayouts]]);
});

test("容器宽度决定桌面列数，小横屏并不切成移动详情或交易底页", () => {
  const layouts = createWorkspaceLayouts();
  const render = (width: number) => renderToStaticMarkup(createElement(views.WorkspaceDesktopLayout, {
    width, layouts, onLayoutChange: () => undefined,
  }, panels()));
  const wide = render(1400);
  const narrow = render(700);
  assert.equal((narrow.match(/workspace-drag-handle/g) ?? []).length, 6);
  assert.equal((narrow.match(/react-resizable-handle-se/g) ?? []).length, 6);
  assert.notEqual(wide, narrow);
  assert.match(narrow, /width:700px/);
  assert.doesNotMatch(narrow, /mobile-sheet|mobile-detail-page/);
});

test("纵屏仍是原 app-grid 直接子节点，保留移动筛选属性、交易底页与 ref", () => {
  const sheetRef = { current: null };
  const children = [
    createElement("article", { key: "market", id: "section-market", className: "panel market-panel" }, "行情"),
    createElement("article", { key: "order", ref: sheetRef, id: "section-order", className: "panel order-panel mobile-sheet", hidden: true }, "委托下单"),
    createElement("article", { key: "user", id: "section-user", className: "panel user-panel" }, "我的"),
  ];
  const props = { orientation: "portrait" as const, "data-mobile-tab": "market", "data-mobile-detail": "1", children };
  const html = renderToStaticMarkup(createElement(views.WorkspaceGrid, props));
  assert.equal(html, renderToStaticMarkup(createElement("div", {
    className: "app-grid", "data-mobile-tab": "market", "data-mobile-detail": "1",
  }, children)));
  assert.doesNotMatch(html, /react-grid|react-resizable|workspace-drag-handle/);
});

test("未知、重复与无标识面板直接拒绝，非法容器宽度不被默认值掩盖", () => {
  for (const children of [
    [createElement("article", { id: "unknown" })],
    [createElement("article", { id: "section-market" }), createElement("article", { id: "section-market" })],
    [createElement("article")],
  ]) {
    assert.throws(() => renderToStaticMarkup(createElement(views.WorkspaceGrid, {
      orientation: "landscape", children,
    })), /工作台.*面板/);
  }
  for (const width of [0, -1, NaN, Infinity]) {
    assert.throws(() => views.WorkspaceDesktopLayout({
      width, layouts: createWorkspaceLayouts(), children: panels(), onLayoutChange: () => undefined,
    }), /工作台.*宽度/);
  }
});
