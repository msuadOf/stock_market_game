import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import React, { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { createServer, type ViteDevServer } from "vite";
import { Provider } from "react-redux";
import { useCompanyQuery } from "./use-company-query.ts";
import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { createRemoteHost } from "../../host/remote-host.ts";
import { market } from "../../host/protocol-test-fixtures.ts";

/** 仅实现 ReactDOM 所需的内存 DOM；hooks、调度、effect 与 cleanup 均由真实 React 执行。 */
class MemoryNode {
  nodeType: number;
  nodeName: string;
  tagName: string;
  namespaceURI = "http://www.w3.org/1999/xhtml";
  ownerDocument: MemoryDocument;
  parentNode: MemoryNode | null = null;
  childNodes: MemoryNode[] = [];
  style: Record<string, unknown> = {};
  attributes: Record<string, string> = {};
  private content = "";
  constructor(name: string, document: MemoryDocument, type = 1) {
    this.nodeName = name.toUpperCase(); this.tagName = this.nodeName;
    this.ownerDocument = document; this.nodeType = type;
  }
  get firstChild() { return this.childNodes[0] ?? null; }
  get lastChild(): MemoryNode | null { return this.childNodes.at(-1) ?? null; }
  get textContent(): string { return this.content + this.childNodes.map((child) => child.textContent).join(""); }
  set textContent(value: string) { this.content = value; this.childNodes.forEach((child) => { child.parentNode = null; }); this.childNodes = []; }
  get nodeValue() { return this.content; }
  set nodeValue(value: string) { this.content = value; }
  appendChild(child: MemoryNode) { if (child.parentNode) child.parentNode.removeChild(child); this.childNodes.push(child); child.parentNode = this; return child; }
  removeChild(child: MemoryNode) { this.childNodes.splice(this.childNodes.indexOf(child), 1); child.parentNode = null; return child; }
  insertBefore(child: MemoryNode, before: MemoryNode) { if (child.parentNode) child.parentNode.removeChild(child); this.childNodes.splice(this.childNodes.indexOf(before), 0, child); child.parentNode = this; return child; }
  setAttribute(key: string, value: string) { this.attributes[key] = String(value); }
  removeAttribute(key: string) { delete this.attributes[key]; }
  addEventListener() {}
  removeEventListener() {}
}
class MemoryDocument {
  nodeType = 9;
  activeElement: MemoryNode | null = null;
  defaultView = { HTMLElement: MemoryNode, HTMLIFrameElement: class {} };
  createElement(name: string) { return new MemoryNode(name, this); }
  createElementNS(_namespace: string, name: string) { return this.createElement(name); }
  createTextNode(value: string) { const node = new MemoryNode("#text", this, 3); node.nodeValue = value; return node; }
  addEventListener() {}
  removeEventListener() {}
}
const document = new MemoryDocument();
const savedWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
Object.defineProperty(globalThis, "window", { configurable: true, value: document.defaultView });
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let vite: ViteDevServer;
let tax: typeof import("./DividendTaxPanel.tsx");
let contract: typeof import("./CompanyContractPanel.tsx");
let preferences: typeof import("./PreferenceRejectionsPanel.tsx");
let rejections: typeof import("./RightsSubscriptionRejectionPanel.tsx");
let companyPanel: typeof import("./CompanyPanel.tsx");
let connected: typeof import("../../app/LocalRefreshViews.tsx");
let marketProvider: typeof import("../../app/MarketRuntimeProvider.tsx");
let appStore: typeof import("../../store/store.ts");
let baselineActions: typeof import("../../store/host-baseline.ts");
before(async () => {
  vite = await createServer({ configFile: false, root: new URL("../../../", import.meta.url).pathname, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  [tax, contract, preferences, rejections, companyPanel] = await Promise.all([
    vite.ssrLoadModule("/src/components/company/DividendTaxPanel.tsx"),
    vite.ssrLoadModule("/src/components/company/CompanyContractPanel.tsx"),
    vite.ssrLoadModule("/src/components/company/PreferenceRejectionsPanel.tsx"),
    vite.ssrLoadModule("/src/components/company/RightsSubscriptionRejectionPanel.tsx"),
    vite.ssrLoadModule("/src/components/company/CompanyPanel.tsx"),
  ]) as [typeof tax, typeof contract, typeof preferences, typeof rejections, typeof companyPanel];
  [connected, marketProvider, appStore, baselineActions] = await Promise.all([
    vite.ssrLoadModule("/src/app/LocalRefreshViews.tsx"), vite.ssrLoadModule("/src/app/MarketRuntimeProvider.tsx"),
    vite.ssrLoadModule("/src/store/store.ts"), vite.ssrLoadModule("/src/store/host-baseline.ts"),
  ]) as [typeof connected, typeof marketProvider, typeof appStore, typeof baselineActions];
});
after(async () => { await vite.close(); if (savedWindow) Object.defineProperty(globalThis, "window", savedWindow); else Reflect.deleteProperty(globalThis, "window"); Reflect.deleteProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT"); });
function mounted() {
  const container = document.createElement("div");
  const root: Root = createRoot(container as unknown as Element);
  return { container, render: async (element: React.ReactNode) => { await act(async () => { root.render(element); }); }, unmount: async () => { await act(async () => root.unmount()); } };
}
function deferred<T>() {
  let resolve!: (value: T) => void; let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function Probe({ query, refreshKey }: { query: (() => Promise<string>) | undefined; refreshKey: string }) {
  const [data, error, loading, refresh] = useCompanyQuery(query, refreshKey);
  return createElement("div", null, loading ? "loading" : error === null ? data === null ? "idle" : data : error, createElement("button", { onClick: refresh }, "refresh"));
}
function elements(node: MemoryNode): MemoryNode[] { return [node, ...node.childNodes.flatMap(elements)]; }
async function click(container: MemoryNode, text: string) {
  const button = elements(container).find((node) => node.tagName === "BUTTON" && node.textContent === text);
  assert.ok(button, `未找到按钮：${text}`);
  const key = Object.keys(button).find((name) => name.startsWith("__reactProps$")); assert.ok(key);
  const props = (button as unknown as Record<string, { onClick: () => void }>)[key]!;
  await act(async () => props.onClick());
}

test("移除宿主回调立即清除旧owner事实，迟到成功不能污染新面板", async () => {
  const view = mounted(); const request = deferred<string>();
  await view.render(createElement(Probe, { query: () => request.promise, refreshKey: "owner-a" }));
  await act(async () => request.resolve("旧账户税表"));
  assert.match(view.container.textContent, /旧账户税表/);
  await view.render(createElement(Probe, { query: undefined, refreshKey: "owner-b" }));
  assert.doesNotMatch(view.container.textContent, /旧账户税表/);
  await view.unmount();
});

test("股息税无实际划收回执时保留未划收额，不宣称资金不足、已尝试或结清", async () => {
  const view = mounted();
  await view.render(createElement(tax.DividendTaxPanel, { refreshKey: "owner-b", onQuery: async () => ({ status: { mode: "AShareIndividual" as const, identity: "Personal" as const, stocks: [] }, outstanding: [{ account: "9", stock: "600001", outstanding: { numerator: "3", denominator: "2" }, latest_collection: null }] }) }));
  assert.match(view.container.textContent, /3\/2 分/);
  assert.match(view.container.textContent, /尚无实际划收回执/);
  assert.doesNotMatch(view.container.textContent, /资金不足|已结清|已尝试/);
  await view.unmount();
});

test("同一上下文逆序resolve/reject只保留后发成功；同步抛错显式展示", async () => {
  const view = mounted(); const first = deferred<string>(); const second = deferred<string>(); let calls = 0;
  const query = () => ++calls === 1 ? first.promise : second.promise;
  await view.render(createElement(Probe, { query, refreshKey: "g1" }));
  await click(view.container, "refresh");
  await act(async () => second.resolve("后发成功"));
  await act(async () => first.reject(new Error("过期错误")));
  assert.match(view.container.textContent, /后发成功/); assert.doesNotMatch(view.container.textContent, /过期错误/);
  await view.render(createElement(Probe, { query: () => { throw new Error("同步错误详情"); }, refreshKey: "g1" }));
  assert.match(view.container.textContent, /同步错误详情/);
  await view.unmount();
});

test("callback更换、refreshKey变化与卸载使旧请求成功和错误全部失效", async () => {
  const view = mounted(); const old = deferred<string>(); const next = deferred<string>(); const third = deferred<string>();
  await view.render(createElement(Probe, { query: () => old.promise, refreshKey: "a" }));
  const provider = () => next.promise;
  await view.render(createElement(Probe, { query: provider, refreshKey: "b" }));
  await act(async () => next.resolve("新owner事实"));
  await act(async () => old.resolve("旧owner迟到"));
  assert.match(view.container.textContent, /新owner事实/); assert.doesNotMatch(view.container.textContent, /旧owner迟到/);
  await view.render(createElement(Probe, { query: () => third.promise, refreshKey: "b" }));
  await view.unmount(); await act(async () => third.reject(new Error("卸载后错误")));
  assert.equal(view.container.textContent, "");
});

test("稳定provider不因内部setState重复请求；tuple避免合法NUL上下文碰撞", async () => {
  const view = mounted(); let calls = 0;
  function TupleProbe({ parts }: { parts: readonly string[] }) {
    const [data] = useCompanyQuery(provider, parts); return createElement("div", null, data);
  }
  const provider = async () => `查询${++calls}`;
  await view.render(createElement(TupleProbe, { parts: ["a\0b", "c"] }));
  await view.render(createElement(TupleProbe, { parts: ["a\0b", "c"] }));
  assert.equal(calls, 1);
  await view.render(createElement(TupleProbe, { parts: ["a", "b\0c"] }));
  assert.equal(calls, 2);
  await view.unmount();
});

test("切owner期间旧税表及旧错误不会渗入新的税面板", async () => {
  const view = mounted(); const old = deferred<import("./DividendTaxPanel.tsx").DividendTaxQueryResult>();
  const pending = deferred<import("./DividendTaxPanel.tsx").DividendTaxQueryResult>();
  await view.render(createElement(tax.DividendTaxPanel, { onQuery: () => old.promise, refreshKey: "owner-a:g1" }));
  await view.render(createElement(tax.DividendTaxPanel, { onQuery: () => pending.promise, refreshKey: "owner-b:g1" }));
  await act(async () => old.reject(new Error("旧账户税错")));
  assert.doesNotMatch(view.container.textContent, /旧账户税错/);
  assert.match(view.container.textContent, /正在查询/);
  await act(async () => pending.resolve({ status: { mode: "Exempt", identity: "NonIndividualPending", stocks: [] }, outstanding: [] }));
  assert.match(view.container.textContent, /印花税.*佣金与过户费照付/);
  assert.match(view.container.textContent, /差别计税未实现/);
  await view.unmount();
});

test("历史部分收缴回执展示当时资金与精确分数，不能外推当前结清", async () => {
  const view = mounted();
  await view.render(createElement(tax.DividendTaxPanel, { refreshKey: "g2", onQuery: async () => ({ status: { mode: "AShareIndividual" as const, identity: "Personal" as const, stocks: [] }, outstanding: [{ account: "9", stock: "600001", outstanding: { numerator: "9007199254740993", denominator: "1" }, latest_collection: { operation_seq: 2, event_id: "tax-event", day: "2026-10-20", available_cash: "12", collected: "12", remaining_cash: "0", outstanding: { numerator: "3", denominator: "2" }, needs_funds: true } }] }) }));
  assert.match(view.container.textContent, /90071992547409\.93 元（9007199254740993 分）/);
  assert.match(view.container.textContent, /2026-10-20.*可用现金 12 分.*实收 12 分.*3\/2 分.*该回执时资金不足/);
  assert.doesNotMatch(view.container.textContent, /已结清/);
  await view.unmount();
});

const capability = {
  revenue: true, net_income: true, equity: true, cash_flow: true, full_financial_statements: true,
  cash_settlement: false, unsupported_reason: "实际投资者结算尚未接线",
  issued_shares: "9007199254740993", par_value_per_share: { Available: { cents: "100" } },
  registered_capital: { Unavailable: { reason: "未配置面值事实" } },
  distributable_profit: { Available: { accumulated_after_loss_yuan: "12.01", statutory_reserve_yuan: "1.00", available_for_distribution_yuan: "11.01", reserve_basis_year: null } },
  active_plans: [], owner_rights: [],
  action_readiness: (["CashDividend", "StockDistribution", "RightsOffering", "IssuerRepurchase", "ShareSplit"] as const).map((kind) => ({ kind, ready: false, blockers: ["写命令尚未接线"] })),
} satisfies import("../../types/generated/CompanyCapabilities.ts").CompanyCapabilities;

function contractProps() {
  return { companyId: 2, refreshKey: "owner:g1", onCapabilitiesQuery: async () => capability,
    onOwnerRightsQuery: async () => [], onFlatReceiptsQuery: async () => [], onExplanationQuery: undefined };
}

// 仅构造 React 消费边界，不将这些回调当作 Rust producer 或宿主验收。
function companyReadProps(host: import("../../host/engine-host.ts").EngineHost | null, owner: string | null = "9", refreshKey = "g1:9:2030-06-01") {
  return {
    allowCompanySelection: false, companyId: "实际发行人", initialCivilDate: "2030-06-01",
    companyState: {
      companyCatalog: [{ id: 7, code: "实际发行人", name: "真实选中公司", listed_stock: 0 }],
      stockCatalog: [{ id: 0, code: "600101" }], generation: 1,
      civilDate: "2030-06-01", dayStatus: null, revision: null, lastEventSeq: 0,
      selectedCompanyId: "实际发行人", companies: {}, readingByCompany: {},
    },
    onCompanyChange: () => {}, onReadingChange: () => {}, onQuery: () => {},
    onAdvanceCivilDay: async () => {}, contractReadContext: { host, owner, refreshKey },
  };
}

test("App 公司面板连接只读能力与本人税务，使用实际公司代码且不启用写命令", { timeout: 10_000 }, async () => {
  const calls: string[] = [];
  const host = {
    queryCompanyCapabilities: async (code: string) => { calls.push(code); return capability; },
    queryOwnerRightsOfferings: async () => [], queryOwnerFlatWithholdingReceipts: async () => [],
    queryAccountDividendTaxStatus: async () => ({ mode: "AShareIndividual", identity: "Personal", stocks: [] }),
    queryDividendTaxOutstandingViews: async () => [{ account: "9", stock: "600101", outstanding: { numerator: "3", denominator: "2" }, latest_collection: null }],
  } as unknown as import("../../host/engine-host.ts").EngineHost;
  const view = mounted();
  try {
    await view.render(createElement(companyPanel.CompanyPanel, companyReadProps(host)));
    assert.match(view.container.textContent, /公司能力面与当前方案/);
    assert.match(view.container.textContent, /9007199254740993.*股/);
    assert.match(view.container.textContent, /3\/2 分/);
    assert.deepEqual(calls, ["实际发行人"]);
    assert.doesNotMatch(view.container.textContent, /提交认购|提交提案|批准分红/);
  } finally { await view.unmount(); }
});

test("App 本人读面在退出身份时清旧事实，迟到成功不能写回且不再查询本人", { timeout: 10_000 }, async () => {
  const late = deferred<typeof capability>(); let calls = 0;
  const host = { queryCompanyCapabilities: () => { calls++; return late.promise; } } as unknown as import("../../host/engine-host.ts").EngineHost;
  const view = mounted();
  try {
    await view.render(createElement(companyPanel.CompanyPanel, companyReadProps(host)));
    await view.render(createElement(companyPanel.CompanyPanel, companyReadProps(host, null, "g2:no-owner:2030-06-01")));
    await act(async () => late.resolve(capability));
    assert.match(view.container.textContent, /尚未加入市场.*本人/);
    assert.doesNotMatch(view.container.textContent, /9007199254740993.*股/);
    assert.equal(calls, 1);
  } finally { await view.unmount(); }
});

test("App ConnectedCompanyPanel 实际消费 membership 就绪与 generation，身份失效立即撤销本人读面", { timeout: 10_000 }, async () => {
  const late = deferred<typeof capability>(); let calls = 0;
  const host = { capabilities: {}, queryCompanyCapabilities: () => { calls++; return late.promise; } } as unknown as import("../../host/engine-host.ts").EngineHost;
  const stock = { last_price: "1000", last_close: "1000", best_bid: null, best_ask: null, bids: [], asks: [] };
  appStore.store.dispatch(baselineActions.installHostBaseline({
    generation: "7", companyGeneration: 1, companyCatalog: [{ id: 7, code: "实际发行人", name: "当前公司", listed_stock: 0 }],
    stockCatalog: [{ id: 0, code: "600101" }], civilDate: "2030-06-01", revision: null,
    snapshot: { seq: 0, tick: 0, day: 0, phase: "Continuous", mirror_origin: { authority_epoch: 1, install_sequence: 2, source_tick: 0, source_generation: 0, day_end_stage: "tick", non_tick_revision: 0, mirror_phase: "Continuous" }, markets: { "600101": stock }, accounts: {}, daily_candles: { "600101": [] }, active_daily_candles: {} },
  }));
  const membership = { remote: true, accountId: "9", canControl: false, needsRejoin: false, generation: "7", ready: false };
  appStore.store.dispatch(appStore.setRemoteMembership(membership));
  const view = mounted();
  const panel = createElement(connected.ConnectedCompanyPanel, { stockContext: true, initialCivilDate: "2030-06-01", onCompanyQuery: () => {}, onAdvanceCivilDay: async () => {}, contractReadHost: host });
  const marketProps = { autoOrderManagerRef: { current: null }, hostRef: { current: host }, setNotice: () => {}, children: panel };
  const providerProps = { store: appStore.store, children: createElement(marketProvider.MarketRuntimeProvider, marketProps) };
  const content = createElement(Provider, providerProps);
  try {
    await view.render(content);
    assert.match(view.container.textContent, /尚未加入市场.*本人/);
    assert.equal(calls, 0);
    await act(async () => { appStore.store.dispatch(appStore.setRemoteMembership({ ...membership, ready: true })); });
    assert.equal(calls, 1);
    await act(async () => { appStore.store.dispatch(appStore.setRemoteMembership({ ...membership, ready: true, generation: "8" })); });
    await act(async () => late.resolve(capability));
    assert.match(view.container.textContent, /尚未加入市场.*本人/);
    assert.doesNotMatch(view.container.textContent, /9007199254740993.*股/);
  } finally { await view.unmount(); }
});

test("App ConnectedCompanyPanel 经真实 Remote host 展示 Rust 同局六种只读 JSON", { timeout: 10_000 }, async () => {
  const bytes = readFileSync(new URL("../../host/fixtures/company-read/wire-server.json", import.meta.url));
  assert.equal(createHash("sha256").update(bytes).digest("hex"), "399e7a0831069d7602accf6fa25b8c3ad9ea9bf1d18cd45613345e8277b40c1a");
  const rows = JSON.parse(bytes.toString("utf8")) as { generation: string; owner: string; value: { kind: string; data: unknown } }[];
  const byKind = new Map(rows.map(row => [row.value.kind, row]));
  const calls: { session_id: string; generation: string; query: { kind: string; data?: unknown } }[] = [];
  // 仅传输与 baseline 受控；查询响应逐字来自同一个真实 Rust server fixture。
  const fetchFn = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/api/new")) return new Response(JSON.stringify({ session_id: "fixture-session", session_token: "fixture-token" }));
    if (url.includes("/api/host-capabilities?")) return new Response(JSON.stringify({ npcDecisionDiagnostics: false }));
    if (url.endsWith("/api/running") && init?.method === "POST") return new Response(null);
    if (url.endsWith("/api/company-read-query")) {
      assert.equal(init?.method, "POST");
      assert.equal(new Headers(init?.headers).get("authorization"), "Bearer fixture-token");
      const request = JSON.parse(String(init?.body)) as typeof calls[number];
      assert.deepEqual(Object.keys(request).sort(), ["generation", "query", "session_id"]);
      assert.equal(request.session_id, "fixture-session"); assert.equal(request.generation, "1");
      calls.push(request);
      const row = byKind.get(request.query.kind); assert.ok(row);
      return new Response(JSON.stringify(row));
    }
    if (url.endsWith("/api/session?session_id=fixture-session") && init?.method === "DELETE") return new Response(null);
    throw new Error(`界面 fixture 不支持请求：${url}`);
  }) as typeof fetch;
  const socket = { readyState: 1, close() {}, send() {}, onmessage: null, onclose: null, onerror: null } as unknown as WebSocket;
  const host = await createRemoteHost({} as import("../../types/engine.ts").SessionSetup, 1n, { baseUrl: "http://127.0.0.1:3000", fetchFn, webSocketFactory: () => socket });
  const view = mounted();
  try {
    await host.start(() => undefined);
    const stockCatalog = [{ id: 0, code: "600101" }];
    const companyCatalog = [{ id: 0, code: "C-600101", name: "真实 fixture 公司", listed_stock: 0 }];
    const snapshot = { seq: 0, tick: 0, day: 0, phase: "Continuous" as const, mirror_origin: { authority_epoch: 1, install_sequence: 2, source_tick: 0, source_generation: 0, day_end_stage: "tick", non_tick_revision: 0, mirror_phase: "Continuous" }, markets: { "0": market() }, accounts: {}, daily_candles: { "0": [] }, active_daily_candles: {} };
    socket.onmessage!({ data: JSON.stringify({ Baseline: { timeline_generation: 1, snapshot, stock_catalog: stockCatalog, company_catalog: companyCatalog, civil_date: "2030-01-01", public_revision: 0, public_report_ids: [] } }) } as MessageEvent);
    appStore.store.dispatch(baselineActions.installHostBaseline({ generation: "1", companyGeneration: 1, companyCatalog, stockCatalog, civilDate: "2030-01-01", revision: null, snapshot: host.snapshot() }));
    appStore.store.dispatch(appStore.setRemoteMembership({ remote: true, accountId: "0", canControl: false, needsRejoin: false, generation: "1", ready: true }));
    const panel = createElement(connected.ConnectedCompanyPanel, { stockContext: true, initialCivilDate: "2030-01-01", onCompanyQuery: () => {}, onAdvanceCivilDay: async () => {}, contractReadHost: host });
    const runtimeProps = { autoOrderManagerRef: { current: null }, hostRef: { current: host }, setNotice: () => {}, children: panel };
    const providerProps = { store: appStore.store, children: createElement(marketProvider.MarketRuntimeProvider, runtimeProps) };
    await view.render(createElement(Provider, providerProps));
    assert.match(view.container.textContent, /10000000 股/);
    assert.match(view.container.textContent, /可分配 1710000\.00 元/);
    assert.match(view.container.textContent, /简税（分红付款日按开局比例代扣/);
    assert.match(view.container.textContent, /查询未返回本人个人股息税账记录/);
    assert.match(view.container.textContent, /当前仅支持只读查询/);
    const input = elements(view.container).find(node => node.attributes["aria-label"] === "期间解释期间末日"); assert.ok(input);
    const key = Object.keys(input).find(name => name.startsWith("__reactProps$")); assert.ok(key);
    const props = (input as unknown as Record<string, { onChange: (event: { currentTarget: { value: string } }) => void }>)[key]!;
    await act(async () => props.onChange({ currentTarget: { value: "2029-12-31" } }));
    await click(view.container, "查询期间解释");
    assert.match(view.container.textContent, /上一期：营收 1000000\.00 元/);
    assert.deepEqual(calls.map(call => call.query.kind).sort(), [...byKind.keys()].sort());
    assert.deepEqual(calls.find(call => call.query.kind === "CompanyCapabilities")!.query, { kind: "CompanyCapabilities", data: { company: 0 } });
    assert.deepEqual(calls.find(call => call.query.kind === "CompanyPeriodExplanation")!.query, { kind: "CompanyPeriodExplanation", data: { company: 0, period_end: "2029-12-31" } });
    assert.doesNotMatch(view.container.textContent, /提交认购|提交提案|批准分红|已结清|查询失败/);
  } finally { await view.unmount(); await host.dispose(); }
});

test("公司能力只读利润可用与实际结算不可用并列；真实空不冒查询失败", async () => {
  const view = mounted(); await view.render(createElement(contract.CompanyContractPanel, contractProps()));
  assert.match(view.container.textContent, /9007199254740993.*股/);
  assert.match(view.container.textContent, /100 分\/股/);
  assert.match(view.container.textContent, /不可用：未配置面值事实/);
  assert.match(view.container.textContent, /可分配 11.01 元/);
  assert.match(view.container.textContent, /实际投资者结算尚未接线/);
  assert.match(view.container.textContent, /写命令尚未接线/);
  assert.match(view.container.textContent, /没有未完成.*方案/);
  assert.doesNotMatch(view.container.textContent, /提交认购/);
  await view.unmount();
});

test("公司切换使能力旧成功与旧错误失效，provider每个上下文仅请求一次", async () => {
  const view = mounted(); const first = deferred<typeof capability>(); const second = deferred<typeof capability>(); let calls = 0;
  const provider = (company: number) => { calls++; return company === 2 ? first.promise : second.promise; };
  const props = { ...contractProps(), onCapabilitiesQuery: provider };
  await view.render(createElement(contract.CompanyContractPanel, props));
  await view.render(createElement(contract.CompanyContractPanel, { ...props, companyId: 3 }));
  await act(async () => second.resolve({ ...capability, issued_shares: "123" }));
  await act(async () => first.reject(new Error("旧公司错误")));
  assert.match(view.container.textContent, /123 股/); assert.doesNotMatch(view.container.textContent, /旧公司错误/);
  assert.equal(calls, 2); await view.unmount();
});

test("偏好与认购拒绝分别保留真实台账日期、原因和未完成首日日终错误", async () => {
  const view = mounted();
  await view.render(createElement(preferences.PreferenceRejectionsPanel, { companyId: 2, refreshKey: "g1", onQuery: async () => [{ company: 2, evaluated_on: "2026-09-30", kind: "CashDividend" as const, detail: "利润不足，不能形成提案" }] }));
  assert.match(view.container.textContent, /2026-09-30.*现金分红.*利润不足/); assert.match(view.container.textContent, /上一日终|最近一次已完成/);
  await view.render(createElement(rejections.RightsSubscriptionRejectionPanel, { refreshKey: "g1", onQuery: async () => [{ account: "9", event_id: "rights-event", requested_shares: "9007199254740993", submitted_on: "2026-10-19", rejected_on: "2026-10-20", reason: "公开配售额度耗尽" }] }));
  assert.match(view.container.textContent, /rights-event.*9007199254740993.*2026-10-19.*2026-10-20.*公开配售额度耗尽/);
  await view.render(createElement(preferences.PreferenceRejectionsPanel, { companyId: 3, refreshKey: "g2", onQuery: async () => { throw new Error("首个日终尚未完成"); } }));
  assert.match(view.container.textContent, /首个日终尚未完成/); assert.doesNotMatch(view.container.textContent, /没有偏好提案拒绝记录/);
  await view.unmount();
});

test("本人认购申请、排队、缴款及弃配独立展示，日程锁定与价格保留真实单位", async () => {
  const view = mounted();
  const rights = { event_id: "r1", stock: "600001", issuer: 2, stage: "Entitled" as const, payment_window: "Open" as const,
    price_per_share: "123", payment_start_on: "2026-10-10", payment_deadline_on: "2026-10-20", ex_rights_on: "2026-10-21", settlement_on: "2026-10-22", owner_entitlement: { rights_shares: "100", lock_until: "2027-01-01" }, open_subscription_remaining_shares: null,
    queued_subscription: { requested_shares: "90", submitted_on: "2026-10-11" }, settled_subscription: null };
  const props = { ...contractProps(), onOwnerRightsQuery: async () => [rights] };
  await view.render(createElement(contract.CompanyContractPanel, props));
  assert.match(view.container.textContent, /已排队 90 股（2026-10-11 提交）/);
  assert.match(view.container.textContent, /认购价 123 分\/股/);
  assert.match(view.container.textContent, /登记结算日 2026-10-22/);
  assert.match(view.container.textContent, /锁定期截止 2027-01-01/);
  await view.render(createElement(contract.CompanyContractPanel, { ...props, onOwnerRightsQuery: async () => [{ ...rights, queued_subscription: null, settled_subscription: { requested_shares: "90", paid_shares: "80", waived_shares: "10", paid_amount: "9840" } }] }));
  assert.match(view.container.textContent, /申请 90 股；已缴款认购 80 股、弃配 10 股、缴款 9840 分/);
  assert.doesNotMatch(view.container.textContent, /已排队 90/);
  assert.match(view.container.textContent, /无退款事实字段/);
  await view.unmount();
});

test("本人Flat回执保留精确分与bp，期间解释请求在修改日期和切公司时失效", async () => {
  const view = mounted(); const old = deferred<import("../../types/generated/PeriodChangeExplanation.ts").PeriodChangeExplanation>();
  const explanation = { previous: { revenue: "90071992547409.93", fixed_expense: "2.00", variable_expense: "3.00" }, cycle: "Quarterly" as const, environment_change_bp: 1, demand_contribution_bp: 2, revenue_segments: [], fixed_expense_segments: [], variable_expense_segments: [], revenue_noise_bp: 3, fixed_expense_noise_bp: 4, variable_expense_noise_bp: 5, restart_revenue: null, restart_source: null };
  const props = { ...contractProps(), onFlatReceiptsQuery: async () => [{ payment_id: "pay1", plan_id: "plan1", account: "9", paid_on: "2026-10-10", gross: "9007199254740993", rate_bp: 1000, withheld: "900719925474099" }], onExplanationQuery: (_company: number, day: string) => day === "2026-09-30" ? old.promise : Promise.resolve(explanation) };
  await view.render(createElement(contract.CompanyContractPanel, props));
  assert.match(view.container.textContent, /pay1plan12026-10-1090071992547409931000900719925474099/);
  async function changeDate(date: string) {
    const input = elements(view.container).find((node) => node.attributes["aria-label"] === "期间解释期间末日"); assert.ok(input);
    const key = Object.keys(input).find((name) => name.startsWith("__reactProps$")); assert.ok(key);
    const props = (input as unknown as Record<string, { onChange: (event: { currentTarget: { value: string } }) => void }>)[key]!;
    await act(async () => props.onChange({ currentTarget: { value: date } }));
  }
  await changeDate("2026-09-30"); await click(view.container, "查询期间解释");
  await changeDate("2026-12-31"); await click(view.container, "查询期间解释");
  assert.match(view.container.textContent, /90071992547409\.93 元/);
  await act(async () => old.reject(new Error("旧期间错误")));
  assert.doesNotMatch(view.container.textContent, /旧期间错误/);
  await view.render(createElement(contract.CompanyContractPanel, { ...props, companyId: 3 }));
  assert.match(view.container.textContent, /尚未查询期间解释/);
  assert.doesNotMatch(view.container.textContent, /上一期：营收/);
  await view.unmount();
});

test("同provider只切refreshKey也失效旧请求；移除callback后迟到错误不可写回", async () => {
  const view = mounted(); const old = deferred<string>(); const next = deferred<string>(); let calls = 0;
  const provider = () => ++calls === 1 ? old.promise : next.promise;
  await view.render(createElement(Probe, { query: provider, refreshKey: "owner-a:g1" }));
  await view.render(createElement(Probe, { query: provider, refreshKey: "owner-a:g2" }));
  await act(async () => next.resolve("新generation成功"));
  await act(async () => old.reject(new Error("旧generation错误")));
  assert.match(view.container.textContent, /新generation成功/); assert.doesNotMatch(view.container.textContent, /旧generation错误/);
  const removed = deferred<string>();
  await view.render(createElement(Probe, { query: () => removed.promise, refreshKey: "owner-a:g2" }));
  await view.render(createElement(Probe, { query: undefined, refreshKey: "owner-a:g2" }));
  await act(async () => removed.reject(new Error("已移除host错误")));
  assert.equal(view.container.textContent, "idlerefresh");
  await view.unmount();
});
