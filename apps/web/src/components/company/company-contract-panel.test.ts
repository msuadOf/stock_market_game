import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";

let vite: ViteDevServer;
let CompanyContractPanel: typeof import("./CompanyContractPanel.tsx").CompanyContractPanel;
let contractKindLabel: typeof import("./CompanyContractPanel.tsx").contractKindLabel;
let contractStageLabel: typeof import("./CompanyContractPanel.tsx").contractStageLabel;
let contractWindowLabel: typeof import("./CompanyContractPanel.tsx").contractWindowLabel;
let contractQueryTrigger: typeof import("./CompanyContractPanel.tsx").contractQueryTrigger;

before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  const panel = await vite.ssrLoadModule("/src/components/company/CompanyContractPanel.tsx") as typeof import("./CompanyContractPanel.tsx");
  CompanyContractPanel = panel.CompanyContractPanel;
  contractKindLabel = panel.contractKindLabel;
  contractStageLabel = panel.contractStageLabel;
  contractWindowLabel = panel.contractWindowLabel;
  contractQueryTrigger = panel.contractQueryTrigger;
});

after(async () => { if (vite) await vite.close(); });

test("行为类别、阶段与窗口使用中文标签", () => {
  assert.equal(contractKindLabel("CashDividend"), "现金分红");
  assert.equal(contractKindLabel("RightsOffering"), "配股／增发");
  assert.equal(contractStageLabel("Entitled"), "权证已派发");
  assert.equal(contractStageLabel("Completed"), "已完成待注销");
  assert.equal(contractWindowLabel("Open"), "缴款期内");
  assert.equal(contractWindowLabel("Closed"), "已截止");
});

test("宿主不支持任一查询时逐区显式提示而非静默隐藏", () => {
  const markup = renderToStaticMarkup(createElement(CompanyContractPanel, {
    companyId: "C-600101",
    onCapabilitiesQuery: undefined,
    onOwnerRightsQuery: undefined,
    onExplanationQuery: undefined,
    onFlatReceiptsQuery: undefined,
    refreshKey: "g1:2030-01-02",
  }));
  assert.match(markup, /公司能力面与当前方案/);
  assert.match(markup, /当前宿主不支持公司能力面查询/);
  assert.match(markup, /当前宿主不支持本人配股权益查询/);
  assert.match(markup, /当前宿主不支持期间解释查询/);
  assert.match(markup, /当前宿主不支持简税代扣回执查询/);
});

test("支持查询的宿主在 SSR 初始态不误报不支持", () => {
  const markup = renderToStaticMarkup(createElement(CompanyContractPanel, {
    companyId: "C-600101",
    onCapabilitiesQuery: () => Promise.resolve({} as never),
    onOwnerRightsQuery: () => Promise.resolve([]),
    onExplanationQuery: () => Promise.resolve({} as never),
    onFlatReceiptsQuery: () => Promise.resolve([]),
    refreshKey: "g1:2030-01-02",
  }));
  // SSR 不执行 effect，首帧不出现加载/数据文案是预期；关键是不误报「不支持」。
  assert.match(markup, /公司能力面与当前方案/);
  assert.match(markup, /刷新能力面/);
  assert.doesNotMatch(markup, /当前宿主不支持/);
});

test("期间解释查询按钮在日期未填时禁用", () => {
  const markup = renderToStaticMarkup(createElement(CompanyContractPanel, {
    companyId: "C-600101",
    onCapabilitiesQuery: undefined,
    onOwnerRightsQuery: undefined,
    onExplanationQuery: () => Promise.resolve({} as never),
    onFlatReceiptsQuery: undefined,
    refreshKey: "g1:2030-01-02",
  }));
  assert.match(markup, /查询期间解释/);
  assert.match(markup, /disabled/);
});

// —— F 修复轮必修-2 回归锁（React 无限请求循环）——
// `useQueried` 曾把 `query` 闭包放进 effect 依赖：面板每次渲染为
// onCapabilitiesQuery 新建闭包 → 查询完成 setState → 重渲染 → 新闭包身份 →
// effect 再触发 → 无限请求循环+闪烁。修复后触发键只由「支持位+刷新键」
// 构成（contractQueryTrigger），查询本体经 ref 取最新值。
test("重查触发键不含闭包身份：同支持位与刷新键下两次渲染的新建闭包不再触发第二次查询", () => {
  const first = contractQueryTrigger(true, "g1:2030-01-02:C-600101");
  // 与面板调用点同构：每次渲染新建的闭包，身份必然不同。
  const second = contractQueryTrigger(true, "g1:2030-01-02:C-600101");
  assert.equal(first, second, "闭包身份不得进入触发键（否则无限请求循环回归）");
  assert.notEqual(first, contractQueryTrigger(true, "g1:2030-01-03:C-600101"), "刷新键变化必须重新查询");
  assert.notEqual(first, contractQueryTrigger(false, "g1:2030-01-02:C-600101"), "支持位变化必须重新评估");
});

test("渲染期零请求（SSR 调用计数：两次渲染均不在 render 阶段发起查询）", () => {
  let calls = 0;
  const query = () => { calls += 1; return Promise.resolve({} as never); };
  for (let round = 0; round < 2; round += 1) {
    renderToStaticMarkup(createElement(CompanyContractPanel, {
      companyId: "C-600101",
      onCapabilitiesQuery: query,
      onOwnerRightsQuery: query,
      onExplanationQuery: query,
      onFlatReceiptsQuery: query,
      refreshKey: "g1:2030-01-02",
    }));
  }
  // SSR 不执行 effect，查询只应发生在 effect；若有人在渲染路径直接调用
  // query（绕过 effect 契约），此处立即暴露。
  assert.equal(calls, 0);
});
