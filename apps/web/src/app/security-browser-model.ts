export type SecurityListView = "all" | "watchlist" | "holdings";

interface SecurityFilter {
  codes: readonly string[];
  names: Readonly<Record<string, string>>;
  favorites: readonly string[];
  heldCodes: ReadonlySet<string>;
  query: string;
  view: SecurityListView;
}

export function filterSecurityCodes({ codes, names, favorites, heldCodes, query, view }: SecurityFilter): string[] {
  const watchlist = new Set(favorites);
  const search = query.trim().toLocaleLowerCase("zh-CN");
  return codes.filter(code =>
    (view === "all" || (view === "watchlist" ? watchlist.has(code) : heldCodes.has(code))) &&
    (search.length === 0 || code.toLocaleLowerCase("zh-CN").includes(search) || names[code]?.toLocaleLowerCase("zh-CN").includes(search)));
}

export function toggleWatchlistCode(favorites: readonly string[], code: string): string[] {
  return favorites.includes(code) ? favorites.filter(value => value !== code) : [...favorites, code];
}

export function adjacentSecurityCode(codes: readonly string[], current: string, direction: -1 | 1): string | null {
  const index = codes.indexOf(current);
  if (index < 0 || codes.length < 2) return null;
  return codes[(index + direction + codes.length) % codes.length];
}

export function securityListKeyboardTarget(codes: readonly string[], current: string, key: string): string | null {
  if (codes.length === 0) return null;
  if (key === "Home") return codes[0];
  if (key === "End") return codes[codes.length - 1];
  if (key === "ArrowUp" || key === "ArrowDown") return adjacentSecurityCode(codes, current, key === "ArrowUp" ? -1 : 1);
  return null;
}

export function securityListEmptyMessage(view: SecurityListView, query: string, ready: boolean): string {
  if (view === "watchlist" && !ready) return "自选尚未读取，请查看读取提示。";
  if (query.trim().length > 0) return "没有匹配股票，可清空搜索或切换范围。";
  if (view === "watchlist") return "暂无自选股票，可在个股报价区加入自选。";
  if (view === "holdings") return "暂无持仓，可从全部行情选择股票。";
  return "当前游戏没有可显示的股票。";
}
