import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import * as React from "react";
import { createServer } from "vite";
import type { MobileUiAction, MobileUiState } from "../mobile/mobile-ui-state.ts";

function mobileUiFixture(hook: (orientation: "portrait" | "landscape", chartCode: string) => { openDetail: (code: string) => void; dispatchMobileUi: (action: MobileUiAction) => void }) {
  type Effect = () => void | (() => void);
  const internals = (React as unknown as { __SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED: { ReactCurrentDispatcher: { current: unknown } } }).__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED;
  const refs: { current: unknown }[] = [];
  const states: unknown[] = [];
  const dependencies = new Map<number, readonly unknown[]>();
  const cleanups = new Map<number, () => void>();
  const pending = new Map<number, Effect>();
  let cursor = 0;
  const dispatcher = {
    useReducer(reducer: (state: MobileUiState, action: MobileUiAction) => MobileUiState, initialState: MobileUiState) {
      const index = cursor++;
      states[index] ??= initialState;
      return [states[index] as MobileUiState, (action: MobileUiAction) => { states[index] = reducer(states[index] as MobileUiState, action); }] as const;
    },
    useRef<Value>(initial: Value) {
      const index = cursor++;
      refs[index] ??= { current: initial };
      return refs[index] as { current: Value };
    },
    useLayoutEffect() { cursor += 1; },
    useEffect(effect: Effect, next: readonly unknown[]) {
      const index = cursor++;
      const previous = dependencies.get(index);
      if (previous === undefined || previous.length !== next.length || next.some((value, position) => !Object.is(value, previous[position]))) {
        dependencies.set(index, next);
        pending.set(index, effect);
      }
    },
  };
  return {
    render(chartCode = "600101", orientation: "portrait" | "landscape" = "portrait") {
      const previous = internals.ReactCurrentDispatcher.current;
      cursor = 0;
      internals.ReactCurrentDispatcher.current = dispatcher;
      try { return hook(orientation, chartCode); } finally { internals.ReactCurrentDispatcher.current = previous; }
    },
    commit() {
      for (const [index, effect] of pending) {
        cleanups.get(index)?.();
        const cleanup = effect();
        if (cleanup) cleanups.set(index, cleanup);
        else cleanups.delete(index);
      }
      pending.clear();
    },
  };
}

test("G48/G64：真实Grid消费中文locale与单元格键盘选择，不禁止焦点", { timeout: 10000 }, () => {
  const source = readFileSync(new URL("../components/MarketGrid.tsx", import.meta.url), "utf8");
  assert.match(source, /localeText=\{MARKET_GRID_LOCALE\}/);
  assert.match(source, /onCellKeyDown=\{onCellKeyDown\}/);
  assert.doesNotMatch(source, /suppressCellFocus=\{true\}/);
  assert.match(readFileSync(new URL("../../index.html", import.meta.url), "utf8"), /lang="zh-CN"/);
});

test("G22/G23/G24/G65：App消费字段错误与详情焦点owner，信息切换不请求滚动", { timeout: 10000 }, () => {
  const source = readFileSync(new URL("../App.tsx", import.meta.url), "utf8");
  const controller = readFileSync(new URL("./useMobileUiController.ts", import.meta.url), "utf8");
  const ticket = readFileSync(new URL("./QuickTradingPanel.tsx", import.meta.url), "utf8");
  assert.match(source, /QuickTradingPanel/);
  assert.match(ticket, /aria-describedby=\{touched && error/);
  assert.match(source, /openDetail\(code\)/);
  assert.match(controller, /detailFocusRef\.current\.apply\(mobileDetail/);
  assert.doesNotMatch(controller, /scrollIntoView/);
  assert.match(source, /aria-current=\{mobileTab === tab \? "page"/);
});

test("Q04：移动详情页标题使用股票名，首页和启动页使用应用名", { timeout: 10000 }, () => {
  const controller = readFileSync(new URL("./useMobileUiController.ts", import.meta.url), "utf8");
  const html = readFileSync(new URL("../../index.html", import.meta.url), "utf8");
  const detail = readFileSync(new URL("../mobile/MobileStockDetail.tsx", import.meta.url), "utf8");
  assert.match(controller, /document\.title\s*=\s*mobileDetail && orientation === "portrait"\s*\?\s*`\$\{STOCK_NAMES\[chartCode\] \?\? chartCode\} — 股票模拟游戏`\s*:\s*"股票模拟游戏"/);
  assert.match(html, /<title>股票模拟游戏<\/title>/);
  assert.match(detail, /className="msd-security-title"><strong>\{props\.name\}<\/strong>/);
});

test("Q04：标题 effect 随打开详情、切换股票、横竖屏和返回更新", { timeout: 10000 }, async () => {
  const vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  const previousDocument = Object.getOwnPropertyDescriptor(globalThis, "document");
  const previousElement = Object.getOwnPropertyDescriptor(globalThis, "HTMLElement");
  try {
    Object.defineProperty(globalThis, "document", { configurable: true, value: { title: "股票模拟游戏", activeElement: null } });
    Object.defineProperty(globalThis, "HTMLElement", { configurable: true, value: class HTMLElement {} });
    const controllerModule = await vite.ssrLoadModule("/src/app/useMobileUiController.ts") as typeof import("./useMobileUiController.ts");
    const fixture = mobileUiFixture(controllerModule.useMobileUiController);
    let controller = fixture.render();
    fixture.commit();
    assert.equal(globalThis.document.title, "股票模拟游戏");
    controller.openDetail("600101");
    controller = fixture.render();
    fixture.commit();
    assert.equal(globalThis.document.title, "稳健实业 — 股票模拟游戏");
    controller = fixture.render("002156");
    fixture.commit();
    assert.equal(globalThis.document.title, "芯片科技 — 股票模拟游戏");
    fixture.render("002156", "landscape");
    fixture.commit();
    assert.equal(globalThis.document.title, "股票模拟游戏");
    controller = fixture.render("002156", "portrait");
    fixture.commit();
    assert.equal(globalThis.document.title, "芯片科技 — 股票模拟游戏");
    controller.openDetail("002156");
    controller = fixture.render("002156");
    fixture.commit();
    assert.equal(globalThis.document.title, "芯片科技 — 股票模拟游戏");
    controller.dispatchMobileUi({ type: "back" });
    fixture.render();
    fixture.commit();
    assert.equal(globalThis.document.title, "股票模拟游戏");
  } finally {
    if (previousDocument) Object.defineProperty(globalThis, "document", previousDocument);
    else Reflect.deleteProperty(globalThis, "document");
    if (previousElement) Object.defineProperty(globalThis, "HTMLElement", previousElement);
    else Reflect.deleteProperty(globalThis, "HTMLElement");
    await vite.close();
  }
});

test("Q16：持仓视图直接显示派生净成本金额与浮盈", { timeout: 10000 }, () => {
  const views = readFileSync(new URL("./LocalRefreshViews.tsx", import.meta.url), "utf8");
  assert.match(views, /<td className="num">\{yuan\(position\.avgCost\)\}<\/td>/);
  assert.match(views, /formatCentsAmount\(position\.pnl\)/);
});

test("G40：移动详情倍速透传同一已确认控制owner而非直接改Redux", { timeout: 10000 }, () => {
  const app = readFileSync(new URL("../App.tsx", import.meta.url), "utf8");
  const views = readFileSync(new URL("./LocalRefreshViews.tsx", import.meta.url), "utf8");
  assert.match(app, /onSpeedChange=\{handleSpeedChange\}/);
  assert.match(views, /onSpeedChange=\{props\.onSpeedChange\}/);
  assert.doesNotMatch(views, /dispatch\(setSpeed/);
  assert.match(app, /onFailure\(failure\) \{\s*fatalHostErrorRef\.current\(failure\)/);
  assert.match(app, /onPauseAfterCloseChange=\{\(value\) => void pausePreferences\.changePreferences/);
});

test("G25/G30/G33/G34：最终CSS热区、focus token、容器字号与交易底页减少动效规则", { timeout: 10000 }, () => {
  const css = readFileSync(new URL("../mobile/MobileStockDetail.css", import.meta.url), "utf8");
  const appCss = readFileSync(new URL("../App.css", import.meta.url), "utf8");
  assert.match(css, /--msd-focus:\s*#126fd1/);
  assert.match(css, /\.msd-header \.msd-stock-switch \{[^}]*width:44px/);
  const lastQuoteRule = css.slice(css.lastIndexOf(".msd-last strong"));
  assert.match(lastQuoteRule, /font-size:clamp\([^)]*cqw/);
  assert.match(appCss, /@media \(prefers-reduced-motion: reduce\) \{\s*\.layout-mobile \.order-panel\.mobile-sheet,[\s\S]*?transition: none/);
});
