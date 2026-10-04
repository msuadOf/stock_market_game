import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

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
  assert.match(source, /aria-describedby=\{fieldErrors\.quantity \? "trade-quantity-error"/);
  assert.match(source, /openDetail\(code\)/);
  assert.match(controller, /detailFocusRef\.current\.apply\(mobileDetail/);
  assert.doesNotMatch(controller, /scrollIntoView/);
  assert.match(source, /aria-current=\{mobileTab === tab \? "page"/);
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
