import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { configureStore } from "@reduxjs/toolkit";
import { Provider, ReactReduxContext } from "react-redux";
import { createServer, type ViteDevServer } from "vite";
import { DEFAULT_MOVING_AVERAGES, MOVING_AVERAGE_STORAGE_KEY } from "../config/moving-average-settings.ts";
import { chartSettingsReducer, setChartAverageSettings } from "../store/chart-settings-slice.ts";

let vite: ViteDevServer;
let Settings: typeof import("./MovingAverageSettings.tsx").MovingAverageSettings;
let useSettings: typeof import("./useMovingAverageSettings.ts").useMovingAverageSettings;
let ReduxContext: typeof ReactReduxContext;
before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  const settingsModule = await loadSsrModule<{ MovingAverageSettings: typeof import("./MovingAverageSettings.tsx").MovingAverageSettings }>("/src/components/MovingAverageSettings.tsx");
  const hookModule = await loadSsrModule<{ useMovingAverageSettings: typeof import("./useMovingAverageSettings.ts").useMovingAverageSettings }>("/src/components/useMovingAverageSettings.ts");
  const reduxModule = await loadSsrModule<{ ReactReduxContext: typeof ReactReduxContext }>("react-redux");
  Settings = settingsModule.MovingAverageSettings;
  useSettings = hookModule.useMovingAverageSettings;
  ReduxContext = reduxModule.ReactReduxContext;
});
async function loadSsrModule<T extends Record<string, unknown>>(url: string): Promise<T> {
  return await vite.ssrLoadModule(url) as T;
}
after(async () => { await vite.close(); });
test("均线设置实际表单默认五条可编辑与显示开关，没有旧禁用按钮", { timeout: 10000 }, () => {
  const html = renderToStaticMarkup(React.createElement(Settings, { settings: DEFAULT_MOVING_AVERAGES, save: () => true, error: null }));
  for (const period of [5, 10, 20, 30, 60]) assert.match(html, new RegExp(`value="${period}"`));
  assert.equal((html.match(/type="checkbox"/g) ?? []).length, 5);
  assert.match(html, /新增均线/);
  assert.match(html, /保存均线设置/);
  assert.doesNotMatch(html, /disabled/);
});

type HookResult = ReturnType<typeof useSettings>;
interface HookHarness<T> { render(): T; dispose(): void }
function mountHook(contextValue: React.ContextType<typeof ReactReduxContext>): HookHarness<HookResult>;
function mountHook<T>(renderValue: () => T): HookHarness<T>;
function mountHook<T>(contextOrRenderValue: React.ContextType<typeof ReactReduxContext> | (() => T)): HookHarness<T | HookResult> {
  const slots: { value: unknown; deps?: readonly unknown[] }[] = [];
  const effects: (() => void)[] = [];
  const cleanups: (() => void)[] = [];
  let cursor = 0;
  const internal = (React as unknown as { __SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED: { ReactCurrentDispatcher: { current: unknown } } }).__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED;
  const dispatcher = {
    useState(initial: unknown) { const index = cursor++; if (slots[index] === undefined) slots[index] = { value: typeof initial === "function" ? initial() : initial }; return [slots[index].value, (value: unknown) => { slots[index].value = value; }]; },
    useContext(context: unknown) {
      cursor++;
      if (context !== ReduxContext || typeof contextOrRenderValue === "function") throw new Error("测试只允许读取实际Provider交付的ReactReduxContext");
      return contextOrRenderValue;
    },
    useRef(initial: unknown) { const index = cursor++; if (slots[index] === undefined) slots[index] = { value: { current: initial } }; return slots[index].value; },
    useMemo(factory: () => unknown, deps: readonly unknown[]) { const index = cursor++; const previous = slots[index]; if (previous === undefined || deps.some((value, position) => !Object.is(value, previous.deps?.[position]))) slots[index] = { value: factory(), deps }; return slots[index].value; },
    useCallback(callback: (...args: never[]) => unknown, deps: readonly unknown[]) { return dispatcher.useMemo(() => callback, deps); },
    useSyncExternalStore(_subscribe: unknown, getSnapshot: () => unknown) { cursor++; return getSnapshot(); },
    useDebugValue() { cursor++; },
    useEffect(effect: () => void | (() => void), deps: readonly unknown[]) { const index = cursor++; const previous = slots[index]?.deps; if (previous === undefined || deps.some((value, position) => !Object.is(value, previous[position]))) { slots[index] = { value: undefined, deps }; effects.push(() => { const cleanup = effect(); if (cleanup) cleanups.push(cleanup); }); } },
  };
  const invoke = typeof contextOrRenderValue === "function" ? contextOrRenderValue : () => useSettings();
  return {
    render() { const previous = internal.ReactCurrentDispatcher.current; cursor = 0; internal.ReactCurrentDispatcher.current = dispatcher; let result: T | HookResult; try { result = invoke(); } finally { internal.ReactCurrentDispatcher.current = previous; } while (effects.length) effects.shift()!(); return result; },
    dispose() { cleanups.forEach((cleanup) => cleanup()); },
  };
}
function createStore() { return configureStore({ reducer: { chartSettings: chartSettingsReducer } }); }
function provideStore(store: ReturnType<typeof createStore>) {
  const provider = mountHook(() => Provider({ store, children: null }));
  const element = provider.render() as React.ReactElement<{ value: React.ContextType<typeof ReactReduxContext> }>;
  assert.equal(element.type, ReduxContext.Provider);
  assert.ok(element.props.value);
  assert.equal(element.props.value.store, store);
  return { contextValue: element.props.value, dispose() { provider.dispose(); } };
}
test("Desktop/Mobile两个消费者同步同一偏好，保存失败保留双方旧设置且显错", { timeout: 10000 }, () => {
  const previousWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  const previousStorageEvent = Object.getOwnPropertyDescriptor(globalThis, "StorageEvent");
  let stored: string | null = null, failing = false;
  const windowPort = Object.assign(new EventTarget(), { localStorage: { getItem: (key: string) => { assert.equal(key, MOVING_AVERAGE_STORAGE_KEY); return stored; }, setItem: (_key: string, value: string) => { if (failing) throw new Error("空间不足"); stored = value; } } });
  Object.defineProperty(globalThis, "window", { configurable: true, value: windowPort });
  Object.defineProperty(globalThis, "StorageEvent", { configurable: true, value: class extends Event {} });
  const store = createStore();
  const root = provideStore(store);
  const desktop = mountHook(root.contextValue), mobile = mountHook(root.contextValue);
  try {
    desktop.render(); mobile.render();
    const next = [{ period: 7, visible: false }, { period: 30, visible: true }];
    assert.equal(desktop.render().save(next), true);
    assert.deepEqual(mobile.render().settings, next);
    assert.deepEqual(desktop.render().settings, next);
    failing = true;
    assert.equal(mobile.render().save([{ period: 99, visible: true }]), false);
    assert.deepEqual(desktop.render().settings, next);
    assert.deepEqual(mobile.render().settings, next);
    assert.match(mobile.render().error!, /空间不足.*原设置保持不变/);
  } finally {
    desktop.dispose(); mobile.dispose(); root.dispose();
    if (previousWindow) Object.defineProperty(globalThis, "window", previousWindow); else Reflect.deleteProperty(globalThis, "window");
    if (previousStorageEvent) Object.defineProperty(globalThis, "StorageEvent", previousStorageEvent); else Reflect.deleteProperty(globalThis, "StorageEvent");
  }
});

test("读取坏JSON显式报错并保留Redux偏好，合法保存后恢复可用", { timeout: 10000 }, () => {
  const previousWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  const previousStorageEvent = Object.getOwnPropertyDescriptor(globalThis, "StorageEvent");
  const retained = [{ period: 13, visible: false }, { period: 26, visible: true }];
  const store = createStore();
  store.dispatch(setChartAverageSettings(retained));
  let stored: string | null = "{";
  const windowPort = Object.assign(new EventTarget(), { localStorage: {
    getItem: (key: string) => { assert.equal(key, MOVING_AVERAGE_STORAGE_KEY); return stored; },
    setItem: (_key: string, value: string) => { stored = value; },
  } });
  Object.defineProperty(globalThis, "window", { configurable: true, value: windowPort });
  Object.defineProperty(globalThis, "StorageEvent", { configurable: true, value: class extends Event {} });
  const root = provideStore(store);
  const hook = mountHook(root.contextValue);
  try {
    hook.render();
    const failed = hook.render();
    assert.deepEqual(failed.settings, retained);
    assert.equal(failed.available, false);
    assert.match(failed.error!, /读取MA显示设置失败.*未安装默认值或损坏内容/);
    const valid = [{ period: 8, visible: true }];
    assert.equal(failed.save(valid), true);
    const recovered = hook.render();
    assert.deepEqual(recovered.settings, valid);
    assert.equal(recovered.available, true);
    assert.equal(recovered.error, null);
    assert.deepEqual(JSON.parse(stored!), valid);
  } finally {
    hook.dispose(); root.dispose();
    if (previousWindow) Object.defineProperty(globalThis, "window", previousWindow); else Reflect.deleteProperty(globalThis, "window");
    if (previousStorageEvent) Object.defineProperty(globalThis, "StorageEvent", previousStorageEvent); else Reflect.deleteProperty(globalThis, "StorageEvent");
  }
});

test("读取存储异常显式报错且不安装默认Redux偏好", { timeout: 10000 }, () => {
  const previousWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  const previousStorageEvent = Object.getOwnPropertyDescriptor(globalThis, "StorageEvent");
  const retained = [{ period: 9, visible: false }];
  const store = createStore();
  store.dispatch(setChartAverageSettings(retained));
  const windowPort = Object.assign(new EventTarget(), { localStorage: {
    getItem: () => { throw new Error("存储被拒绝"); },
    setItem: () => { throw new Error("存储被拒绝"); },
  } });
  Object.defineProperty(globalThis, "window", { configurable: true, value: windowPort });
  Object.defineProperty(globalThis, "StorageEvent", { configurable: true, value: class extends Event {} });
  const root = provideStore(store);
  const hook = mountHook(root.contextValue);
  try {
    hook.render();
    const result = hook.render();
    assert.deepEqual(result.settings, retained);
    assert.equal(result.available, false);
    assert.match(result.error!, /读取MA显示设置失败.*存储被拒绝/);
  } finally {
    hook.dispose(); root.dispose();
    if (previousWindow) Object.defineProperty(globalThis, "window", previousWindow); else Reflect.deleteProperty(globalThis, "window");
    if (previousStorageEvent) Object.defineProperty(globalThis, "StorageEvent", previousStorageEvent); else Reflect.deleteProperty(globalThis, "StorageEvent");
  }
});
