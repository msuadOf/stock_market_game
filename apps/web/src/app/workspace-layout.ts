import type { Layout, LayoutItem } from "react-grid-layout";

export const WORKSPACE_PANEL_IDS = [
  "section-market", "section-trade", "section-company", "section-order", "section-positions", "section-trades",
] as const;

export type WorkspacePanelId = typeof WORKSPACE_PANEL_IDS[number];
export type WorkspaceBreakpoint = "wide" | "medium" | "narrow";
export type WorkspaceLayouts = Record<WorkspaceBreakpoint, Layout>;

export const WORKSPACE_BREAKPOINTS = { wide: 1200, medium: 900, narrow: 0 };
export const WORKSPACE_COLUMNS = { wide: 12, medium: 8, narrow: 4 };
export const WORKSPACE_ROW_HEIGHT = 24;
export const WORKSPACE_MARGIN = [10, 10] as const;

export const WORKSPACE_PANEL_TITLES: Record<WorkspacePanelId, string> = {
  "section-market": "行情",
  "section-trade": "走势图与盘口",
  "section-company": "公司信息",
  "section-order": "委托下单",
  "section-positions": "持仓",
  "section-trades": "分时成交",
};

const PANEL_MIN_HEIGHTS: Record<WorkspacePanelId, number> = {
  "section-market": 8,
  "section-trade": 12,
  "section-company": 10,
  "section-order": 12,
  "section-positions": 6,
  "section-trades": 6,
};

const WIDE_GEOMETRY = [
  [0, 0, 3, 20], [3, 0, 5, 12], [3, 12, 5, 16], [8, 0, 4, 14], [8, 22, 4, 8], [8, 14, 4, 8],
] as const;
const MEDIUM_GEOMETRY = [
  [0, 0, 4, 12], [4, 0, 4, 12], [0, 12, 4, 16], [4, 12, 4, 16], [0, 28, 4, 8], [4, 28, 4, 8],
] as const;

export function createWorkspaceLayouts(panelIds: readonly WorkspacePanelId[] = WORKSPACE_PANEL_IDS): WorkspaceLayouts {
  const seen = new Set<string>();
  for (const id of panelIds) {
    if (!WORKSPACE_PANEL_IDS.includes(id)) throw new Error(`工作台布局：未知面板 ${String(id)}`);
    if (seen.has(id)) throw new Error(`工作台布局：重复面板 ${id}`);
    seen.add(id);
  }
  let narrowRow = 0;
  const layouts: WorkspaceLayouts = { wide: [], medium: [], narrow: [] };
  for (const breakpoint of Object.keys(layouts) as WorkspaceBreakpoint[]) {
    const items: LayoutItem[] = WORKSPACE_PANEL_IDS.map((id, index) => {
      const minH = PANEL_MIN_HEIGHTS[id];
      if (breakpoint === "narrow") {
        const item = { i: id, x: 0, y: narrowRow, w: 4, h: minH + 2, minW: 4, minH };
        narrowRow += item.h;
        return item;
      }
      const [x, y, w, h] = breakpoint === "wide" ? WIDE_GEOMETRY[index]! : MEDIUM_GEOMETRY[index]!;
      const minW = breakpoint === "wide" && (id === "section-trade" || id === "section-company") ? 4 : 3;
      return { i: id, x, y, w, h, minW, minH };
    });
    layouts[breakpoint] = panelIds.map((id) => {
      const item = items.find((candidate) => candidate.i === id);
      if (!item) throw new Error(`工作台布局：缺少面板 ${id} 的 ${breakpoint} 布局`);
      return item;
    });
  }
  return layouts;
}
