import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import type { PlayerProposalResultView } from "../../host/player-proposals.ts";

let vite: ViteDevServer;
let PlayerProposalPanel: typeof import("./PlayerProposalPanel.tsx").PlayerProposalPanel;
let InGamePreferencesPanel: typeof import("./PlayerProposalPanel.tsx").InGamePreferencesPanel;
let proposalKindLabel: typeof import("./PlayerProposalPanel.tsx").proposalKindLabel;
let proposalClassLabel: typeof import("./PlayerProposalPanel.tsx").proposalClassLabel;

before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  const panel = await vite.ssrLoadModule("/src/components/company/PlayerProposalPanel.tsx") as typeof import("./PlayerProposalPanel.tsx");
  PlayerProposalPanel = panel.PlayerProposalPanel;
  InGamePreferencesPanel = panel.InGamePreferencesPanel;
  proposalKindLabel = panel.proposalKindLabel;
  proposalClassLabel = panel.proposalClassLabel;
});

after(async () => { if (vite) await vite.close(); });

test("提案类别与制度拒绝分类使用中文标签", () => {
  assert.equal(proposalKindLabel("CashDividend"), "现金分红");
  assert.equal(proposalKindLabel("RightsOffering"), "配股");
  assert.equal(proposalKindLabel("SecondaryOffering"), "增发（定向本人）");
  assert.equal(proposalKindLabel("ShareSplit"), "拆股／缩股");
  assert.equal(proposalClassLabel("BusinessCondition"), "制度条件不满足");
  assert.equal(proposalClassLabel("UnsupportedOperation"), "本局未支持");
});

test("宿主不支持玩家提案或偏好编辑时显式提示而非静默隐藏", () => {
  const markup = renderToStaticMarkup(createElement(PlayerProposalPanel, {
    companyId: "C-600101",
    onProposalSubmit: undefined,
    onCapabilitiesQuery: undefined,
    refreshKey: "g1:2030-01-02",
  }));
  assert.match(markup, /发起提案/);
  assert.match(markup, /当前宿主不支持玩家提案/);
  const preferencesMarkup = renderToStaticMarkup(createElement(InGamePreferencesPanel, {
    companyId: "C-600101",
    onPreferencesUpdate: undefined,
    onPreferencesQuery: undefined,
    refreshKey: "g1:2030-01-02",
  }));
  assert.match(preferencesMarkup, /公司行为偏好（局内）/);
  assert.match(preferencesMarkup, /当前宿主不支持偏好局内编辑/);
});

test("支持的宿主在 SSR 初始态渲染六类表单与游戏化简化说明，不误报不支持", () => {
  let calls = 0;
  const query = () => { calls += 1; return Promise.resolve({} as never); };
  const accepted: PlayerProposalResultView = { outcome: "accepted", kind: "CashDividend", identity: "x", approved_on: "2030-01-02", announced_on: "2030-01-02" };
  const markup = renderToStaticMarkup(createElement(PlayerProposalPanel, {
    companyId: "C-600101",
    onProposalSubmit: () => Promise.resolve(accepted),
    onCapabilitiesQuery: query,
    refreshKey: "g1:2030-01-02",
  }));
  assert.match(markup, /发起提案/);
  assert.match(markup, /游戏化简化/);
  assert.match(markup, /持仓即可、直接生效/);
  assert.match(markup, /每股税前红利（元）/);
  assert.match(markup, /提交提案/);
  assert.doesNotMatch(markup, /当前宿主不支持/);
  // SSR 不执行 effect，渲染阶段零请求（同 CompanyContractPanel 契约）。
  assert.equal(calls, 0);
});

test("偏好局内编辑面板呈现下一周期生效语义与未配置默认说明", () => {
  const markup = renderToStaticMarkup(createElement(InGamePreferencesPanel, {
    companyId: "C-600101",
    onPreferencesUpdate: () => Promise.resolve(),
    onPreferencesQuery: () => Promise.resolve({ cash_dividend: null, stock_distribution: null }),
    refreshKey: "g1:2030-01-02",
  }));
  assert.match(markup, /下一结算周期末日评估/);
  assert.match(markup, /同周期内已产生的提案不回滚/);
  assert.match(markup, /保存偏好修改/);
  assert.doesNotMatch(markup, /当前宿主不支持/);
});
