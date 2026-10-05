import { useCallback, useEffect, useState } from "react";
import { validateSecuritySort, type SecuritySort } from "./security-sort-model.ts";
import { WatchlistPreferences } from "../config/watchlist-preferences.ts";
import { toggleWatchlistCode, type SecurityListView } from "./security-browser-model.ts";

/** 名单与查询只有一个 owner，证券选中仍由 MarketRuntime 管理。 */
export function useSecurityBrowser(onNotice: (message: string) => void) {
  const [repository] = useState(() => new WatchlistPreferences(() => window.localStorage));
  const [favorites, setFavorites] = useState<readonly string[]>([]);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<SecurityListView>("all");
  const [query, setQuery] = useState("");
  const [sortRules, updateSortRules] = useState<readonly SecuritySort[]>([]);
  const setSortRules = useCallback((rules: readonly SecuritySort[]) => {
    validateSecuritySort(rules);
    updateSortRules(previous => previous.length === rules.length && previous.every((rule, index) => rule.field === rules[index].field && rule.direction === rules[index].direction)
      ? previous : rules.map(rule => ({ ...rule })));
  }, []);

  const load = useCallback(() => {
    try {
      setFavorites(repository.load());
      setReady(true);
      setError(null);
      return true;
    } catch (failure) {
      const message = `${failure instanceof Error ? failure.message : String(failure)}。请检查浏览器存储权限或反馈错误详情，再重试读取。`;
      setFavorites([]);
      setReady(false);
      setError(message);
      onNotice(message);
      return false;
    }
  }, [repository, onNotice]);
  useEffect(() => { load(); }, [load]);

  function reload() {
    if (load()) onNotice("自选已重新读取。");
  }

  function toggleFavorite(code: string) {
    if (!ready) return;
    const next = toggleWatchlistCode(favorites, code);
    try {
      repository.save(next);
      setFavorites(next);
      setError(null);
      if (error !== null) onNotice("自选已保存。");
    } catch (failure) {
      const message = `${failure instanceof Error ? failure.message : String(failure)}。已保留原自选名单；请检查浏览器空间和权限或反馈错误详情。`;
      setError(message);
      onNotice(message);
    }
  }

  return { favorites, ready, error, view, query, sortRules, setSortRules, setView, setQuery, toggleFavorite, reload };
}

export type SecurityBrowser = ReturnType<typeof useSecurityBrowser>;
