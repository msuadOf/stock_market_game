import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import React, { type ReactElement } from "react";
import { createServer, type ViteDevServer } from "vite";
import { DEFAULT_SETUP } from "../config/defaults.ts";

let vite: ViteDevServer;
let Screen: typeof import("./RemoteLoginScreen.tsx").RemoteLoginScreen;
before(async () => {
  vite = await createServer({ root: new URL("../..", import.meta.url).pathname, configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  Screen = (await vite.ssrLoadModule("/src/app/RemoteLoginScreen.tsx")).RemoteLoginScreen;
});
after(async () => { if (vite) await vite.close(); });

type NodeProps = { children?: React.ReactNode; onClick?: () => void; onChange?: (event: unknown) => void; disabled?: boolean; type?: string; role?: string };
function elements(node: React.ReactNode): ReactElement<NodeProps>[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!React.isValidElement<NodeProps>(node)) return [];
  return [node, ...elements(node.props.children)];
}
function label(node: React.ReactNode): string {
  if (Array.isArray(node)) return node.map(label).join("");
  if (React.isValidElement<NodeProps>(node)) return label(node.props.children);
  return typeof node === "string" ? node : "";
}

function harness(seed = "18446744073709551615") {
  const slots: unknown[] = [];
  const effects: (() => void)[] = [];
  const cleanup: (() => void)[] = [];
  let cursor = 0;
  const connected: unknown[] = [];
  let backs = 0;
  const dispatcher = {
    useState(initial: unknown) {
      const slot = cursor++;
      if (!(slot in slots)) slots[slot] = initial;
      return [slots[slot], (next: unknown) => { slots[slot] = next; }];
    },
    useRef(initial: unknown) { const slot = cursor++; if (!(slot in slots)) slots[slot] = { current: initial }; return slots[slot]; },
    useEffect(effect: () => (() => void), dependencies: unknown[]) {
      const slot = cursor++;
      if (!(slot in slots)) { slots[slot] = dependencies; effects.push(() => cleanup.push(effect())); }
    },
  };
  const internals = (React as unknown as { __SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED: { ReactCurrentDispatcher: { current: unknown } } }).__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED;
  let tree: React.ReactNode;
  function render() {
    cursor = 0;
    const previous = internals.ReactCurrentDispatcher.current;
    internals.ReactCurrentDispatcher.current = dispatcher;
    try { tree = Screen({ baseUrl: "https://example.test", setup: DEFAULT_SETUP, seed, companySystemDraft: JSON.stringify(DEFAULT_SETUP.company_system), onConnected: (...args) => { connected.push(args); }, onBack: () => { backs++; } }); }
    finally { internals.ReactCurrentDispatcher.current = previous; }
    while (effects.length) effects.shift()!();
    return tree;
  }
  function button(text: string) { const found = elements(tree).find((node) => node.type === "button" && label(node).includes(text)); assert.ok(found, text); return found; }
  render();
  return { render, button, connected, backs: () => backs, dispose: () => cleanup.forEach((effect) => effect()), text: () => label(tree), change: (type: string, value: string) => {
    const input = elements(tree).find((node) => node.type === "input" && (node.props.type === type || (type === "username" && node.props.type === undefined)));
    assert.ok(input); input.props.onChange!({ currentTarget: { value } });
  }, check: (text: string) => {
    const found = elements(tree).find((node) => node.type === "label" && label(node).includes(text));
    assert.ok(found);
    const input = elements(found).find((node) => node.type === "input");
    assert.ok(input); input.props.onChange!({ currentTarget: { checked: true } });
  } };
}

const token = "a".repeat(64);
const subject = { subject_id: "subject", username: null };
const context = { session_id: "market", setup: DEFAULT_SETUP, seed: "1", resumed: false, generation: "2", member: null, can_control: true, needs_rejoin: true };
async function settle() { await new Promise((resolve) => setTimeout(resolve, 15)); }

test("Controller 无资金账户可只进入公共控制；重入按钮待本人确认", { timeout: 10000 }, async () => {
  const previous = globalThis.fetch;
  const calls: string[] = [];
  globalThis.fetch = async (input) => { calls.push(String(input)); return Response.json(String(input).endsWith("guest") ? { subject, token } : { markets: [context] }, { status: String(input).endsWith("guest") ? 201 : 200 }); };
  const ui = harness();
  try {
    await settle(); ui.render();
    ui.button("明确创建匿名").props.onClick!();
    await settle(); ui.render();
    assert.equal(ui.button("确认重新加入并进入").props.disabled, true);
    ui.button("仅进入公共市场").props.onClick!();
    assert.equal(ui.connected.length, 1);
    assert.equal(calls.length, 2);
    assert.ok(!ui.text().includes("此身份不在当前加载的日终存档中"));
  } finally { ui.dispose(); globalThis.fetch = previous; }
});

test("重复登录只发一次；返回后旧认证响应不得查询市场或连接", { timeout: 10000 }, async () => {
  const previous = globalThis.fetch;
  let resolve: (response: Response) => void = () => { throw new Error("请求未开始"); };
  const calls: string[] = [];
  globalThis.fetch = async (input) => { calls.push(String(input)); return new Promise((finish) => { resolve = finish; }); };
  const ui = harness();
  try {
    await settle(); ui.render();
    ui.button("明确创建匿名").props.onClick!();
    ui.button("明确创建匿名").props.onClick!();
    assert.equal(calls.length, 1);
    ui.button("返回启动选择").props.onClick!();
    resolve(Response.json({ subject, token }, { status: 201 }));
    await settle(); ui.render();
    assert.equal(calls.length, 1);
    assert.equal(ui.connected.length, 0);
    assert.equal(ui.backs(), 1);
  } finally { ui.dispose(); globalThis.fetch = previous; }
});

test("启动自动验证记住的身份，失效 credential 显式错误且不创建 guest", { timeout: 10000 }, async () => {
  const previousFetch = globalThis.fetch;
  const previousFactory = globalThis.indexedDB;
  const { IndexedDbCredentialStore } = await vite.ssrLoadModule("/src/auth/credential-store.ts") as typeof import("../auth/credential-store.ts");
  const previousLoad = IndexedDbCredentialStore.prototype.load;
  globalThis.indexedDB = {} as IDBFactory;
  IndexedDbCredentialStore.prototype.load = async () => ({ server: "https://example.test", token, subject });
  const calls: string[] = [];
  globalThis.fetch = async (input) => { calls.push(String(input)); return new Response(token, { status: 401 }); };
  const ui = harness();
  try {
    await settle(); ui.render();
    assert.deepEqual(calls, ["https://example.test/api/auth/me"]);
    assert.match(ui.text(), /凭据失效/);
    assert.ok(!ui.text().includes(token));
    assert.equal(ui.connected.length, 0);
  } finally {
    ui.dispose(); globalThis.fetch = previousFetch; globalThis.indexedDB = previousFactory; IndexedDbCredentialStore.prototype.load = previousLoad;
  }
});

for (const boundary of ["markets", "join", "create"] as const) {
  test(`返回后旧 ${boundary} 响应不连接新页面`, { timeout: 10000 }, async () => {
    const previous = globalThis.fetch;
    const member = { account_id: "1", admission_funding: { external_cash: "1000000000000" } };
    const joined = { ...context, member, needs_rejoin: false };
    let resolve: (response: Response) => void = () => { throw new Error("延迟请求未开始"); };
    globalThis.fetch = async (input) => {
      const url = String(input);
      if (url.endsWith("guest")) return Response.json({ subject, token }, { status: 201 });
      if (boundary === "markets" || url.endsWith("/join") || url.endsWith("/new")) return new Promise((finish) => { resolve = finish; });
      return Response.json({ markets: boundary === "create" ? [] : [joined] });
    };
    const ui = harness();
    try {
      await settle(); ui.render();
      ui.button("明确创建匿名").props.onClick!();
      await settle(); ui.render();
      if (boundary === "join") ui.button("进入已有账户").props.onClick!();
      if (boundary === "create") { ui.check("确认创建共享市场"); ui.render(); ui.button("创建共享市场").props.onClick!(); }
      ui.button("返回启动选择").props.onClick!();
      resolve(Response.json(boundary === "markets" ? { markets: [joined] } : joined));
      await settle(); ui.render();
      assert.equal(ui.connected.length, 0);
    } finally { ui.dispose(); globalThis.fetch = previous; }
  });
}

test("远程新市场直接使用展示 seed 与配置，不再抽熵；非法 seed 不发创建请求", { timeout: 10000 }, async () => {
  const previousFetch = globalThis.fetch;
  const previousEntropy = globalThis.crypto.getRandomValues;
  const creations: { seed: string; setup: unknown }[] = [];
  globalThis.crypto.getRandomValues = () => { throw new Error("新市场不能在创建时抽第二个 seed"); };
  globalThis.fetch = async (input, init) => {
    const url = String(input);
    if (url.endsWith("guest")) return Response.json({ subject, token }, { status: 201 });
    if (url.endsWith("/new")) {
      const request = JSON.parse(String(init!.body)); creations.push(request);
      return Response.json({ ...context, setup: request.setup, seed: request.seed, member: { account_id: "1", admission_funding: { external_cash: "1000000000000" } }, needs_rejoin: false });
    }
    return Response.json({ markets: [] });
  };
  try {
    for (const seed of ["18446744073709551615", "01"]) {
      const ui = harness(seed);
      try {
        await settle(); ui.render(); ui.button("明确创建匿名").props.onClick!();
        await settle(); ui.render(); ui.check("确认创建共享市场"); ui.render(); ui.button("创建共享市场").props.onClick!();
        await settle(); ui.render();
        if (seed === "01") { assert.equal(ui.connected.length, 0); assert.match(ui.text(), /seed 必须/); }
        else assert.equal(ui.connected.length, 1, ui.text());
      } finally { ui.dispose(); }
    }
    assert.deepEqual(creations, [{ setup: DEFAULT_SETUP, seed: "18446744073709551615" }]);
  } finally { globalThis.fetch = previousFetch; globalThis.crypto.getRandomValues = previousEntropy; }
});
