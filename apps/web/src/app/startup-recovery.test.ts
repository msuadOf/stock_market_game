import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import * as React from "react";
import type { ReactElement } from "react";
import { createServer, type ViteDevServer } from "vite";
import { DayEndPersistence } from "../save/day-end-persistence.ts";
import type { SessionSetup } from "../types/engine.ts";

let vite: ViteDevServer;
let AppRoot: typeof import("./AppRoot.tsx").AppRoot;

before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", mode: "production", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true }, define: { "import.meta.env.DEV": "false" } });
  AppRoot = (await vite.ssrLoadModule("/src/app/AppRoot.tsx") as typeof import("./AppRoot.tsx")).AppRoot;
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
  let root: ReactElement;
  function render(): ReactElement {
    const previous = internals.ReactCurrentDispatcher.current;
    internals.ReactCurrentDispatcher.current = dispatcher;
    cursor = 0;
    try {
      root = AppRoot({ renderShell: (props) => React.createElement("div", props) });
      return Array.isArray(root.props.failures) ? root.props.children : root;
    } finally { internals.ReactCurrentDispatcher.current = previous; }
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
    return render();
  }
  return { render, startRemote, report: () => root };
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
  assert.equal(app.render().props.startupTarget.kind, "remote", "旧提交尚未完成时不能卸载到选择页");
  assert.equal(app.render().props.returningToStartup, true);
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
  const current = app.render();
  assert.match(current.props.startupReturnError, /返回启动选择失败.*原宿主停止失败/);
  assert.equal(current.props.returningToStartup, false);
  assert.equal(current.props.startupTarget.kind, "remote");
  await current.props.onSelectHost(() => {});
  assert.equal(app.render().props.mode, "remote");
});

test("自定义新局日期和价格笼子在取消或失败重挂后保留，不重新读快速槽", async () => {
  const app = memoryApp();
  let shell = app.startRemote();
  assert.equal(typeof shell.props.setSessionSetup, "function", "待启动 setup 必须由外层 App 持有");
  const custom: SessionSetup = {
    ...shell.props.sessionSetup,
    start_date: "2031-02-03",
    config: { ...shell.props.sessionSetup.config, price_cage_enabled: false },
  };
  shell.props.initialSaveSourceRef.current.reset();
  shell.props.setSessionSetup(custom);
  let reads = 0;
  for (const reason of ["取消", "初始化失败"]) {
    shell = app.render();
    assert.equal(shell.props.sessionSetup, custom, reason);
    await shell.props.onSelectHost(() => {});
    shell = app.startRemote();
    assert.equal(shell.props.sessionSetup.start_date, "2031-02-03", reason);
    assert.equal(shell.props.sessionSetup.config.price_cage_enabled, false, reason);
    assert.equal(await shell.props.initialSaveSourceRef.current.read(async () => { reads++; return null; }), null);
  }
  assert.equal(reads, 0);
});


test("App 同一 paint 前连续 spawn failure 全部保留，重选新宿主不清报告，用户关闭才清空", { timeout: 10000 }, async () => {
  const app = memoryApp();
  const shell = app.startRemote();
  assert.equal(typeof shell.props.onSpawnFailure, "function", "App 必须从构造开始提供非 fatal sink");
  const first = { code: "WORKER_SPAWN_FAILED", where: "wasm.spawn", message: "第一次", cause: { message: "原cause" }, context: { requestId: 1 } };
  const second = { ...first, message: "第二次", context: { requestId: 2 } };
  shell.props.onSpawnFailure(first);
  shell.props.onSpawnFailure(second);
  app.render();
  assert.deepEqual(app.report().props.failures, [first, second]);
  assert.equal(app.report().props.failures[0], first);
  assert.equal(app.report().props.failures[0].cause, first.cause);
  await shell.props.onSelectHost(async () => {});
  app.render();
  assert.deepEqual(app.report().props.failures, [first, second]);
  const nextShell = app.startRemote();
  assert.deepEqual(app.report().props.failures, [first, second]);
  for (let index = 0; index < 40; index++) nextShell.props.onSpawnFailure(first);
  app.render();
  assert.equal(app.report().props.failures.length, 42, "未确认报告不能静默截断");
  app.report().props.onDismiss();
  app.render();
  assert.deepEqual(app.report().props.failures, []);
});
