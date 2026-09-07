import assert from "node:assert/strict";
import test from "node:test";
import { MOBILE_SPEED_OPTIONS, initialMobileUiState, mobilePrimaryTitle, mobileSpeedLabel, reduceMobileUi } from "./mobile-ui-state.ts";

test("详情页的图表周期与信息标签互不重置", () => {
  const detail = reduceMobileUi(initialMobileUiState, { type: "open-detail", code: "600460" });
  const daily = reduceMobileUi(detail, { type: "select-period", period: "日K" });
  const news = reduceMobileUi(daily, { type: "select-info", tab: "资讯" });

  assert.equal(news.detailCode, "600460");
  assert.equal(news.chartPeriod, "日K");
  assert.equal(news.infoTab, "资讯");
});

test("交易底页关闭后回到原详情状态，切主导航则清空临时层", () => {
  const detail = reduceMobileUi(initialMobileUiState, { type: "open-detail", code: "600460" });
  const withTrade = reduceMobileUi(detail, { type: "open-trade" });
  const closed = reduceMobileUi(withTrade, { type: "close-top-layer" });

  assert.equal(withTrade.tradeSheetOpen, true);
  assert.equal(closed.tradeSheetOpen, false);
  assert.equal(closed.detailCode, "600460");

  const positions = reduceMobileUi(withTrade, { type: "switch-primary", tab: "positions" });
  assert.deepEqual(positions, {
    ...initialMobileUiState,
    primaryTab: "positions",
  });
});

test("返回键先关闭交易底页，再退出详情页", () => {
  const detail = reduceMobileUi(initialMobileUiState, { type: "open-detail", code: "600460" });
  const withTrade = reduceMobileUi(detail, { type: "open-trade" });
  const afterFirstBack = reduceMobileUi(withTrade, { type: "back" });
  const afterSecondBack = reduceMobileUi(afterFirstBack, { type: "back" });

  assert.equal(afterFirstBack.tradeSheetOpen, false);
  assert.equal(afterFirstBack.detailCode, "600460");
  assert.equal(afterSecondBack.detailCode, null);
});

test("主导航页面拥有稳定标题，持仓和我的能从详情直接进入", () => {
  assert.equal(mobilePrimaryTitle("market"), "模拟自选");
  assert.equal(mobilePrimaryTitle("positions"), "持仓");
  assert.equal(mobilePrimaryTitle("user"), "我的");

  const detail = reduceMobileUi(initialMobileUiState, { type: "open-detail", code: "600101" });
  const user = reduceMobileUi(detail, { type: "switch-primary", tab: "user" });
  assert.equal(user.primaryTab, "user");
  assert.equal(user.detailCode, null);
});

test("移动端顶栏提供完整的常用倍速", () => {
  assert.deepEqual(MOBILE_SPEED_OPTIONS, [1, 1.5, 2, 3, 6, 30, 60, 180, 360, 720, Infinity]);
  assert.equal(mobileSpeedLabel(Infinity), "最快");
});
