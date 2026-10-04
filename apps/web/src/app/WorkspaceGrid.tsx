import { Children, isValidElement, useState, type HTMLAttributes, type ReactElement, type ReactNode } from "react";
import { ResponsiveGridLayout, type Layout, type ResponsiveLayouts } from "react-grid-layout";
import type { Orientation } from "../hooks/useOrientation.ts";
import {
  WORKSPACE_BREAKPOINTS,
  WORKSPACE_COLUMNS,
  WORKSPACE_MARGIN,
  WORKSPACE_PANEL_IDS,
  WORKSPACE_PANEL_TITLES,
  WORKSPACE_ROW_HEIGHT,
  type WorkspaceBreakpoint,
  type WorkspacePanelId,
} from "./workspace-layout.ts";
import { DesktopTerminal, type DesktopView } from "./DesktopTerminal.tsx";
import "react-grid-layout/css/styles.css";
import "react-resizable/css/styles.css";
import "./workspace-grid.css";

export interface WorkspaceGridProps extends HTMLAttributes<HTMLDivElement> {
  orientation: Orientation;
  desktopView?: DesktopView;
  onDesktopViewChange?: (view: DesktopView) => void;
  stockList?: ReactNode;
  onTradeCurrent?: () => void;
  "data-mobile-tab"?: string;
  "data-mobile-detail"?: string;
}

export interface WorkspaceDesktopLayoutProps {
  width: number;
  layouts: ResponsiveLayouts<WorkspaceBreakpoint>;
  children?: ReactNode;
  onLayoutChange: (layout: Layout, layouts: ResponsiveLayouts<WorkspaceBreakpoint>) => void;
}

function namedPanels(children: ReactNode): ReactElement<{ id: WorkspacePanelId | "section-user" }>[] {
  const seen = new Set<string>();
  return Children.toArray(children).map((child) => {
    if (!isValidElement<{ id?: string }>(child) || typeof child.props.id !== "string") {
      throw new Error("工作台组件：每个直接子面板必须有现有 section 标识");
    }
    const id = child.props.id;
    if (id !== "section-user" && !WORKSPACE_PANEL_IDS.includes(id as WorkspacePanelId)) {
      throw new Error(`工作台组件：未知面板 ${id}`);
    }
    if (seen.has(id)) throw new Error(`工作台组件：重复面板 ${id}`);
    seen.add(id);
    return child as ReactElement<{ id: WorkspacePanelId | "section-user" }>;
  });
}

export function WorkspaceDesktopLayout({ width, layouts, children, onLayoutChange }: WorkspaceDesktopLayoutProps) {
  if (!Number.isFinite(width) || width <= 0) throw new Error(`工作台组件：容器宽度必须为有限正数，收到 ${width}`);
  const panels = namedPanels(children).filter((panel) => panel.props.id !== "section-user");
  const panelIds = new Set(panels.map((panel) => panel.props.id));
  const visibleLayouts = Object.fromEntries(Object.entries(layouts).map(([breakpoint, layout]) => [
    breakpoint, layout.filter((item) => panelIds.has(item.i as WorkspacePanelId)),
  ]));
  return <ResponsiveGridLayout<WorkspaceBreakpoint>
    className="workspace-grid"
    style={{ width }}
    width={width}
    layouts={visibleLayouts}
    breakpoints={WORKSPACE_BREAKPOINTS}
    cols={WORKSPACE_COLUMNS}
    rowHeight={WORKSPACE_ROW_HEIGHT}
    margin={WORKSPACE_MARGIN}
    containerPadding={[0, 0]}
    dragConfig={{ enabled: true, handle: ".workspace-drag-handle", cancel: ".workspace-panel-body, input, textarea, select, button, a, [contenteditable], canvas", threshold: 4 }}
    resizeConfig={{ enabled: true, handles: ["se"] }}
    onLayoutChange={onLayoutChange}
  >
    {panels.map((panel) => {
      const id = panel.props.id as WorkspacePanelId;
      const title = WORKSPACE_PANEL_TITLES[id];
      return <div key={id} className="workspace-panel">
        <div className="workspace-panel-heading">
          <span className="workspace-drag-handle" title={`拖动${title}面板`}>
            <span aria-hidden="true">⠿</span>{title}
          </span>
        </div>
        <div className="workspace-panel-body">{panel}</div>
      </div>;
    })}
  </ResponsiveGridLayout>;
}

export function WorkspaceGrid({ orientation, children, className = "", desktopView = "quotes", onDesktopViewChange, stockList, onTradeCurrent, ...attributes }: WorkspaceGridProps) {
  const [localView, setLocalView] = useState<DesktopView>("quotes");
  const panels = namedPanels(children);
  if (orientation === "portrait") return <div className={`app-grid ${className}`.trim()} {...attributes}>{children}</div>;
  return <div className={`workspace-desktop ${className}`.trim()} {...attributes}>
    <DesktopTerminal view={onDesktopViewChange ? desktopView : localView} onViewChange={onDesktopViewChange ?? setLocalView} stockList={stockList} panels={panels} onTradeCurrent={onTradeCurrent} />
  </div>;
}
