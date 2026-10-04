export const MARKET_GRID_LOCALE = {
  ariaSortableColumn: "按 Enter 排序",
  ariaSortableColumnWithCellSelection: "按 Alt + Enter 排序",
  ariaMenuColumn: "按 Alt + 下方向键打开列菜单",
  ariaFilterColumn: "按 Ctrl + Enter 打开筛选",
  ariaColumnFiltered: "此列已筛选",
  ariaLabelColumnMenu: "列菜单",
  ariaLabelColumnFilter: "列筛选",
  ariaRow: "行",
  ariaColumn: "列",
  ariaRowSelect: "按空格选择此行",
  ariaRowDeselect: "按空格取消选择此行",
  loadingOoo: "正在加载行情…",
  noRowsToShow: "暂无行情",
  noMatchingRows: "没有匹配的行情",
};

export function selectMarketByKeyboard(key: string, code: string | undefined, onSelect: (code: string) => void): boolean {
  if (code === undefined || (key !== "Enter" && key !== " ")) return false;
  onSelect(code);
  return true;
}
