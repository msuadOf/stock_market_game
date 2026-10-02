import assert from "node:assert/strict";
import { test } from "node:test";
import { collides, getBreakpointFromWidth } from "react-grid-layout/core";
import {
  createWorkspaceLayouts,
  WORKSPACE_BREAKPOINTS,
  WORKSPACE_COLUMNS,
  WORKSPACE_PANEL_IDS,
  WORKSPACE_ROW_HEIGHT,
  WORKSPACE_MARGIN,
  type WorkspaceBreakpoint,
  type WorkspacePanelId,
} from "./workspace-layout.ts";

test("三个容器宽度布局都保留六个命名面板、不重叠、不越界", () => {
  const layouts = createWorkspaceLayouts();
  for (const breakpoint of Object.keys(layouts) as WorkspaceBreakpoint[]) {
    const layout = layouts[breakpoint];
    assert.deepEqual(layout.map((item) => item.i), WORKSPACE_PANEL_IDS);
    for (const [index, item] of layout.entries()) {
      assert.ok(item.x >= 0 && item.y >= 0);
      assert.ok(item.x + item.w <= WORKSPACE_COLUMNS[breakpoint]);
      assert.ok(item.minW !== undefined && item.w >= item.minW);
      assert.ok(item.minH !== undefined && item.h >= item.minH);
      assert.ok(item.minH * (WORKSPACE_ROW_HEIGHT + WORKSPACE_MARGIN[1]) - WORKSPACE_MARGIN[1] >= 190);
      for (const other of layout.slice(index + 1)) assert.equal(collides(item, other), false);
    }
  }
});

test("桌面默认行情在左、图在右上、委托在右侧；窄横屏仍用可缩放单列工作台", () => {
  const layouts = createWorkspaceLayouts();
  const [market, chart, company, order] = layouts.wide;
  assert.ok(market && chart && company && order);
  assert.equal(market.x, 0);
  assert.equal(chart.y, 0);
  assert.ok(chart.x >= market.x + market.w);
  assert.ok(company.y >= chart.y + chart.h);
  assert.ok(order.x >= chart.x + chart.w);
  assert.ok(chart.minH !== undefined && chart.minH >= 12);
  assert.ok(order.minH !== undefined && order.minH >= 12);
  for (const item of layouts.narrow) {
    assert.equal(item.x, 0);
    assert.equal(item.w, WORKSPACE_COLUMNS.narrow);
    assert.equal(item.minW, WORKSPACE_COLUMNS.narrow);
  }
  assert.equal(getBreakpointFromWidth(WORKSPACE_BREAKPOINTS, 1400), "wide");
  assert.equal(getBreakpointFromWidth(WORKSPACE_BREAKPOINTS, 1000), "medium");
  assert.equal(getBreakpointFromWidth(WORKSPACE_BREAKPOINTS, 700), "narrow");
});

test("可组合子集只产生对应面板，每次生成不共享可变布局条目", () => {
  const subset = ["section-trade", "section-order"] as const;
  const layouts = createWorkspaceLayouts(subset);
  assert.deepEqual(layouts.wide.map((item) => item.i), subset);
  assert.equal(layouts.wide.length, 2);
  layouts.wide[0]!.x = 0;
  assert.notEqual(createWorkspaceLayouts(subset).wide[0]!.x, 0);
  assert.notStrictEqual(layouts.medium[0], layouts.narrow[0]);
});

test("未知或重复面板明确报错，不偷偷丢弃或替换", () => {
  assert.throws(() => createWorkspaceLayouts(["section-unknown" as WorkspacePanelId]), /工作台.*未知.*section-unknown/);
  assert.throws(() => createWorkspaceLayouts(["section-market", "section-market"]), /工作台.*重复.*section-market/);
});
