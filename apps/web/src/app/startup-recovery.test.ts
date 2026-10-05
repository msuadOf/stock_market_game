import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import * as React from "react";
import type { ReactElement } from "react";
import { createServer, type ViteDevServer } from "vite";
import { DayEndPersistence } from "../save/day-end-persistence.ts";
import type { SessionSetup } from "../types/engine.ts";

let vite: ViteDevServer;
let App: typeof import("../App.tsx").default;
let previewEntropyReads = 0;

before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", mode: "production", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true }, define: { "import.meta.env.DEV": "false" } });
  const originalEntropy = globalThis.crypto.getRandomValues;
  globalThis.crypto.getRandomValues = function<ArrayType extends Exclude<BufferSource, ArrayBuffer>>(array: ArrayType): ArrayType {
    if (array instanceof Uint32Array && array.length === 2) { previewEntropyReads++; array.set([0x12345678, 0x90abcdef]); return array; }
    originalEntropy.call(globalThis.crypto, array);
    return array;
  };
  try { App = (await vite.ssrLoadModule("/src/App.tsx") as typeof import("../App.tsx")).default; }
  finally { globalThis.crypto.getRandomValues = originalEntropy; }
});

after(async () => { if (vite) await vite.close(); });

function deferred<Value>() {
  let resolve!: (value: Value) => void;
  const promise = new Promise<Value>((done) => { resolve = done; });
  return { promise, resolve };
}

function memoryApp() {
  const slots: { value: unknown }[] = [];
  let cursor = 0;
  const internals = (React as unknown as { __SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED: { ReactCurrentDispatcher: { current: unknown } } }).__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED;
  const dispatcher = {
    useState<Value>(initial: Value | (() => Value)) {
      const index = cursor++;
      if (slots[index] === undefined) slots[index] = { value: typeof initial === "function" ? (initial as () => Value)() : initial };
      return [slots[index].value, (next: Value | ((previous: Value) => Value)) => {
        slots[index].value = typeof next === "function" ? (next as (previous: Value) => Value)(slots[index].value as Value) : next;
      }];
    },
    useRef<Value>(initial: Value) {
      const index = cursor++;
      if (slots[index] === undefined) slots[index] = { value: { current: initial } };
      return slots[index].value;
    },
    useCallback(callback: unknown) { cursor++; return callback; },
  };
  function render(): ReactElement {
    const previous = internals.ReactCurrentDispatcher.current;
    internals.ReactCurrentDispatcher.current = dispatcher;
    cursor = 0;
    try { return App(); } finally { internals.ReactCurrentDispatcher.current = previous; }
  }
  function startRemote() {
    let screen = render();
    screen.props.onModeChange("remote");
    screen.props.onAddressChange("https://runtime.example/game");
    screen = render();
    const original = Object.getOwnPropertyDescriptor(globalThis, "window");
    Object.defineProperty(globalThis, "window", { configurable: true, value: {} });
    try { screen.props.onStart(); } finally {
      if (original === undefined) Reflect.deleteProperty(globalThis, "window");
      else Object.defineProperty(globalThis, "window", original);
    }
    const login = render();
    login.props.onConnected("a".repeat(64), { session_id: "market", setup: login.props.setup, seed: login.props.seed, resumed: false, generation: "1", member: null, can_control: true, needs_rejoin: false });
    return render().props.children;
  }
  return { render, startRemote };
}

test("重选等待已提交的原子 rename/close，下一局不能越过旧写入屏障", async () => {
  const app = memoryApp();
  const shell = app.startRemote();
  assert.ok(shell.props.dayEndPersistenceRef, "日终队列必须由外层 App 共享");
  const queue = shell.props.dayEndPersistenceRef.current as DayEndPersistence;
  const committed = deferred<void>();
  const entered = deferred<void>();
  let target = "previous archive";
  queue.install("1");
  const oldWrite = queue.completed("1", Promise.resolve("old archive"), async (_slot, current) => {
    assert.equal(current(), true);
    entered.resolve();
    await committed.promise;
    target = "old archive";
  });
  await entered.promise;
  let stopped = false;
  const selecting = shell.props.onSelectHost(() => { stopped = true; });
  assert.equal(stopped, true);
  await Promise.resolve();
  assert.ok(app.render().props.children, "旧提交尚未完成时不能卸载到选择页");
  assert.equal(app.render().props.children.props.returningToStartup, true);
  await shell.props.onSelectHost(() => { assert.fail("重选进行中不能再次停止或开启另一转换"); });
  committed.resolve();
  await selecting;
  assert.equal(await oldWrite, false);
  assert.equal(target, "old archive");
  assert.equal(app.render().props.mode, "remote");
  const nextShell = app.startRemote();
  assert.equal(nextShell.props.dayEndPersistenceRef.current, queue);
  queue.install("1");
  await queue.completed("1", Promise.resolve("new archive"), async () => { target = "new archive"; });
  assert.equal(target, "new archive");
});

test("停止失败明确留在原启动，不开始下一局，并允许再次等待共享屏障", async () => {
  const app = memoryApp();
  const shell = app.startRemote();
  await shell.props.onSelectHost(() => { throw new Error("原宿主停止失败"); });
  const current = app.render().props.children;
  assert.match(current.props.startupReturnError, /返回启动选择失败.*原宿主停止失败/);
  assert.equal(current.props.returningToStartup, false);
  assert.equal(current.props.startupTarget.kind, "remote");
  await current.props.onSelectHost(() => {});
  assert.equal(app.render().props.mode, "remote");
});

test("自定义新局日期和价格笼子在取消或失败重挂后保留，不重新读快速槽", async () => {
  const app = memoryApp();
  let shell = app.startRemote();
  assert.equal(typeof shell.props.setSessionCreation, "function", "待启动 setup 与 seed 必须由外层 App 原子持有");
  const custom: SessionSetup = {
    ...shell.props.sessionSetup,
    start_date: "2031-02-03",
    config: { ...shell.props.sessionSetup.config, price_cage_enabled: false },
  };
  shell.props.initialSaveSourceRef.current.reset();
  shell.props.setSessionCreation(custom, "18446744073709551615");
  let reads = 0;
  for (const reason of ["取消", "初始化失败"]) {
    shell = app.render().props.children;
    assert.equal(shell.props.sessionSetup, custom, reason);
    assert.equal(shell.props.chosenSessionSeed, "18446744073709551615", reason);
    await shell.props.onSelectHost(() => {});
    shell = app.startRemote();
    assert.equal(shell.props.sessionSetup.start_date, "2031-02-03", reason);
    assert.equal(shell.props.sessionSetup.config.price_cage_enabled, false, reason);
    assert.equal(await shell.props.initialSaveSourceRef.current.read(async () => { reads++; return null; }), null);
  }
  assert.equal(reads, 0);
});

test("真实 App 创建使用预览 seed，StrictMode 重渲染不再抽；用户编辑配置不被 seed 改动覆盖", { timeout: 10000 }, () => {
  const app = memoryApp();
  const initial = app.render().props.creationSettings;
  assert.equal(previewEntropyReads, 1);
  assert.equal(initial.props.seed, "1311768467294899695");
  const firstConfig = initial.props.value;
  app.render(); app.render();
  assert.equal(previewEntropyReads, 1);
  initial.props.onChange("本人未完成的 JSON");
  const edited = app.render().props.creationSettings;
  edited.props.onSeedChange("18446744073709551615");
  const changed = app.render().props.creationSettings;
  assert.equal(changed.props.origin, "custom");
  assert.equal(changed.props.value, "本人未完成的 JSON");
  assert.equal(changed.props.seed, "18446744073709551615");
  assert.notEqual(firstConfig, changed.props.value);
  changed.props.onChange(firstConfig);
  const shell = app.startRemote();
  assert.equal(shell.props.chosenSessionSeed, "18446744073709551615");
});

test("明确重生成失败保留预览与 seed，显式展示熵错误", { timeout: 10000 }, () => {
  const app = memoryApp();
  const initial = app.render().props.creationSettings;
  const originalEntropy = globalThis.crypto.getRandomValues;
  globalThis.crypto.getRandomValues = () => { throw new Error("crypto 熵获取被拒绝"); };
  try { initial.props.onRegenerate(); } finally { globalThis.crypto.getRandomValues = originalEntropy; }
  const failed = app.render();
  assert.match(failed.props.error, /crypto 熵获取被拒绝/);
  assert.equal(failed.props.creationSettings.props.value, initial.props.value);
  assert.equal(failed.props.creationSettings.props.seed, initial.props.seed);
});

test("App 周期预览保持展示 seed；本人编辑后切换周期显错而不覆盖", { timeout: 10000 }, () => {
  const app = memoryApp();
  const initial = app.render().props.creationSettings;
  initial.props.onSettlementCycleChange("Annual");
  const annual = app.render().props.creationSettings;
  assert.equal(annual.props.seed, initial.props.seed);
  assert.equal(JSON.parse(annual.props.value).config.settlement_cycle, "Annual");
  assert.notEqual(JSON.parse(annual.props.value).config.companies[0].generation.initial_revenue, JSON.parse(initial.props.value).config.companies[0].generation.initial_revenue);
  annual.props.onChange(annual.props.value);
  const custom = app.render().props.creationSettings;
  custom.props.onSettlementCycleChange("Monthly");
  const rejected = app.render();
  assert.match(rejected.props.error, /本人编辑.*同长度/);
  assert.equal(rejected.props.creationSettings.props.value, annual.props.value);
  assert.equal(rejected.props.creationSettings.props.seed, initial.props.seed);
});
