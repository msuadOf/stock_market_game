import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { fileURLToPath } from "node:url";
import { createElement, type ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { DEFAULT_SETUP } from "../config/defaults.ts";
import { commandDayEndArchiveFixture } from "./command-host-test-fixture.ts";

let vite: import("vite").ViteDevServer;
let panel: typeof import("./MemoryHandoverPanel.tsx");

before(async () => {
  const { createServer } = await import("vite");
  vite = await createServer({ root: fileURLToPath(new URL("../..", import.meta.url)), configFile: false, appType: "custom", mode: "production", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  panel = await vite.ssrLoadModule("/src/app/MemoryHandoverPanel.tsx") as typeof panel;
});

after(async () => { if (vite) await vite.close(); });

const frozen = {
  kind: "frozen" as const,
  code: "MEMORY_RESIDUAL_RELOAD_HANDOVER" as const,
  residualBytes: 256,
  carriedResidualBytes: 512,
  thresholdEffective: 256,
  reloadCount: 1,
  pendingIntent: { kind: "load" as const, archive: commandDayEndArchiveFixture() },
  last_committed_tick: 45,
  day: 3,
  handoverNonce: "ab".repeat(16),
};

function props(overrides: Partial<Parameters<typeof panel.MemoryHandoverPanel>[0]> = {}) {
  return {
    handover: frozen,
    selectedIntent: { kind: "new" as const, setup: { ...DEFAULT_SETUP, start_date: "2030-01-02" }, seed: "9007199254740993" },
    busy: false,
    error: null,
    onContinue() {},
    onSelectFile() {},
    onSelectNew() {},
    onCancel() {},
    ...overrides,
  };
}

test("分开显示冻结目标X与当前选择Y的存档/新局摘要", () => {
  const archive = commandDayEndArchiveFixture();
  const html = renderToStaticMarkup(createElement(panel.MemoryHandoverPanel, props({
    handover: { ...frozen, pendingIntent: { kind: "load", archive } },
  })));
  assert.match(html, /aria-label="重载后存档确认"/);
  assert.match(html, /重载前的选择/);
  assert.match(html, /Memory 残留已计入保守账/);
  assert.match(html, /本次 256 bytes/);
  assert.match(html, /累计 512 bytes/);
  assert.doesNotMatch(html, /（[XY]）/);
  assert.ok(archive.civil_clock.settled_through);
  assert.match(html, new RegExp(archive.civil_clock.settled_through));
  assert.match(html, new RegExp(archive.seed));
  assert.match(html, new RegExp(`tick ${archive.snapshot.tick}`));
  assert.match(html, /当前选择/);
  assert.match(html, /新局/);
  assert.match(html, /2030-01-02/);
  assert.match(html, /9007199254740993/);
});

test("冻结新局与lost目标均有明确摘要", () => {
  const newTarget = { kind: "new" as const, setup: { ...DEFAULT_SETUP, start_date: "2031-04-05" }, seed: "42" };
  const newHtml = renderToStaticMarkup(createElement(panel.MemoryHandoverPanel, props({ handover: { ...frozen, pendingIntent: newTarget }, selectedIntent: null })));
  assert.match(newHtml, /重载前的选择/);
  assert.match(newHtml, /2031-04-05/);
  assert.match(newHtml, /seed 42/);
  assert.match(newHtml, /没有当前选择目标/);
  const lostHtml = renderToStaticMarkup(createElement(panel.MemoryHandoverPanel, props({ handover: { kind: "lost", reason: "carrier-unavailable" } })));
  assert.match(lostHtml, /重载前的选择/);
  assert.match(lostHtml, /目标不可用/);
});

test("按钮回调可直接触发，busy期间与无选择时禁用重复动作", () => {
  let continued = 0;
  let selected = 0;
  let cancelled = 0;
  let selectedNew = 0;
  const view = panel.MemoryHandoverPanel(props({ onContinue: () => { continued += 1; }, onSelectFile: () => { selected += 1; }, onSelectNew: () => { selectedNew += 1; }, onCancel: () => { cancelled += 1; } }));
  const buttons = (view.props.children as ReactElement[]).find((child) => child.type === "div")!.props.children as ReactElement[];
  buttons[0].props.onClick();
  buttons[1].props.onClick();
  buttons[2].props.onClick();
  buttons[3].props.onClick();
  assert.deepEqual([continued, selected, selectedNew, cancelled], [1, 1, 1, 1]);

  const busyView = panel.MemoryHandoverPanel(props({ busy: true }));
  const busyButtons = (busyView.props.children as ReactElement[]).find((child) => child.type === "div")!.props.children as ReactElement[];
  assert.deepEqual(busyButtons.map((button) => button.props.disabled), [true, true, true, true]);
  const noSelectionView = panel.MemoryHandoverPanel(props({ selectedIntent: null }));
  const noSelectionButtons = (noSelectionView.props.children as ReactElement[]).find((child) => child.type === "div")!.props.children as ReactElement[];
  assert.equal(noSelectionButtons[0].props.disabled, true);
});

test("错误以 alert 展示，固定操作文案保持可访问", () => {
  const html = renderToStaticMarkup(createElement(panel.MemoryHandoverPanel, props({ error: "存档确认失败：内存载体不可用" })));
  assert.match(html, /role="alert"/);
  assert.match(html, /存档确认失败：内存载体不可用/);
  assert.match(html, /继续所选目标/);
  assert.match(html, /选择其他日终存档/);
  assert.match(html, /选择当前配置新局/);
  assert.match(html, /取消，返回启动选择/);
});
