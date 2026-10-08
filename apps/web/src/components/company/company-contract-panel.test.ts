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
let rightsSubscriptionCandidates: typeof import("./CompanyContractPanel.tsx").rightsSubscriptionCandidates;
let rightsSubscriptionMaxShares: typeof import("./CompanyContractPanel.tsx").rightsSubscriptionMaxShares;
let rightsSubscriptionInputValid: typeof import("./CompanyContractPanel.tsx").rightsSubscriptionInputValid;
let selectSubscriptionCandidate: typeof import("./CompanyContractPanel.tsx").selectSubscriptionCandidate;

before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  const panel = await vite.ssrLoadModule("/src/components/company/CompanyContractPanel.tsx") as typeof import("./CompanyContractPanel.tsx");
  CompanyContractPanel = panel.CompanyContractPanel;
  contractKindLabel = panel.contractKindLabel;
  contractStageLabel = panel.contractStageLabel;
  contractWindowLabel = panel.contractWindowLabel;
  contractQueryTrigger = panel.contractQueryTrigger;
  rightsSubscriptionCandidates = panel.rightsSubscriptionCandidates;
  rightsSubscriptionMaxShares = panel.rightsSubscriptionMaxShares;
  rightsSubscriptionInputValid = panel.rightsSubscriptionInputValid;
  selectSubscriptionCandidate = panel.selectSubscriptionCandidate;
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
    // G 批起认购提交同为可选支持位：全支持宿主不得在任何区域误报「不支持」。
    onRightsSubscription: () => Promise.resolve({ event_id: "e", account: "0", requested_shares: "41", submitted_on: "2030-01-08" }),
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

// —— G 批配股认购入口：候选/上限/输入校验纯函数与 SSR 静态态 ——

type RightsView = Parameters<typeof rightsSubscriptionCandidates>[0][number];

function rightsView(overrides: Partial<RightsView> & { readonly event_id: string }): RightsView {
  return {
    stock: "600101",
    issuer: "C-600101",
    stage: "Entitled",
    payment_window: "Open",
    price_per_share: "500",
    payment_start_on: "2030-01-08",
    payment_deadline_on: "2030-01-10",
    ex_rights_on: "2030-01-13",
    settlement_on: "2030-01-15",
    owner_entitlement: null,
    open_subscription_remaining_shares: null,
    queued_subscription: null,
    settled_subscription: null,
    ...overrides,
  } as RightsView;
}

test("认购候选只含缴款期内且本人未提交认购的方案", () => {
  const views = [
    rightsView({ event_id: "open-entitled", owner_entitlement: { rights_shares: "50", lock_until: null } }),
    rightsView({ event_id: "open-openquota", open_subscription_remaining_shares: "100" }),
    rightsView({ event_id: "closed", payment_window: "Closed", owner_entitlement: { rights_shares: "50", lock_until: null } }),
    rightsView({ event_id: "before-open", payment_window: "BeforeOpen", owner_entitlement: { rights_shares: "50", lock_until: null } }),
    rightsView({ event_id: "queued", queued_subscription: { requested_shares: "10", submitted_on: "2030-01-08" } }),
    rightsView({ event_id: "settled", settled_subscription: { requested_shares: "50", paid_shares: "30", paid_amount: "15000", waived_shares: "20" } }),
    rightsView({ event_id: "no-right-no-quota" }),
    // 复核 minor-2：公开配售剩余额度为 "0"（额度已用尽）不得作为候选。
    rightsView({ event_id: "exhausted-openquota", open_subscription_remaining_shares: "0" }),
  ];
  assert.deepEqual(rightsSubscriptionCandidates(views).map((view) => view.event_id), ["open-entitled", "open-openquota"]);
});

test("显式选择的认购事件退出候选后不得静默回落到其他事件（复核 minor-1）", () => {
  const first = rightsView({ event_id: "event-a", owner_entitlement: { rights_shares: "50", lock_until: null } });
  const second = rightsView({ event_id: "event-b", owner_entitlement: { rights_shares: "30", lock_until: null } });
  // 无显式选择时默认取首个候选。
  assert.equal(selectSubscriptionCandidate([first, second], "")?.event_id, "event-a");
  assert.equal(selectSubscriptionCandidate([first, second], "event-b")?.event_id, "event-b");
  // 显式选择的事件已不在候选（如刚提交成功后刷新退出候选）：必须返回
  // undefined 交回「重新选择」状态，不得回落到 event-a——否则旧股数会被
  // 静默提交到另一个事件。
  assert.equal(selectSubscriptionCandidate([first], "event-b"), undefined);
  assert.equal(selectSubscriptionCandidate([], "event-b"), undefined);
});

test("认购上限优先具名权利，其次公开配售剩余额度", () => {
  assert.equal(rightsSubscriptionMaxShares(rightsView({ event_id: "e", owner_entitlement: { rights_shares: "50", lock_until: null }, open_subscription_remaining_shares: "100" })), "50");
  assert.equal(rightsSubscriptionMaxShares(rightsView({ event_id: "e", open_subscription_remaining_shares: "100" })), "100");
  assert.equal(rightsSubscriptionMaxShares(rightsView({ event_id: "e" })), null);
});

test("认购输入校验只接受不超过上限的正整数（规范十进制字符串）", () => {
  assert.equal(rightsSubscriptionInputValid("41", "50"), true);
  assert.equal(rightsSubscriptionInputValid("50", "50"), true);
  assert.equal(rightsSubscriptionInputValid("51", "50"), false);
  assert.equal(rightsSubscriptionInputValid("0", "50"), false);
  assert.equal(rightsSubscriptionInputValid("", "50"), false);
  assert.equal(rightsSubscriptionInputValid("4.5", "50"), false);
  assert.equal(rightsSubscriptionInputValid("-1", "50"), false);
  assert.equal(rightsSubscriptionInputValid("041", "50"), false);
  assert.equal(rightsSubscriptionInputValid(" 41", "50"), false);
});

test("宿主不支持认购提交时显式提示而非静默隐藏", () => {
  const markup = renderToStaticMarkup(createElement(CompanyContractPanel, {
    companyId: "C-600101",
    onCapabilitiesQuery: undefined,
    onOwnerRightsQuery: () => Promise.resolve([]),
    onRightsSubscription: undefined,
    onExplanationQuery: undefined,
    onFlatReceiptsQuery: undefined,
    refreshKey: "g1:2030-01-02",
  }));
  assert.match(markup, /本人配股权益/);
  assert.match(markup, /当前宿主不支持配股认购提交/);
});

test("支持认购提交的宿主在 SSR 初始态不误报不支持", () => {
  const markup = renderToStaticMarkup(createElement(CompanyContractPanel, {
    companyId: "C-600101",
    onCapabilitiesQuery: undefined,
    onOwnerRightsQuery: () => Promise.resolve([]),
    onRightsSubscription: () => Promise.resolve({ event_id: "e", account: "0", requested_shares: "41", submitted_on: "2030-01-08" }),
    onExplanationQuery: undefined,
    onFlatReceiptsQuery: undefined,
    refreshKey: "g1:2030-01-02",
  }));
  assert.match(markup, /本人配股权益/);
  assert.doesNotMatch(markup, /当前宿主不支持配股认购提交/);
});
