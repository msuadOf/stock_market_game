import { useId, useRef } from "react";
import type { SecurityBrowser } from "../app/useSecurityBrowser.ts";
import "./security-list-controls.css";

interface Props {
  browser: SecurityBrowser;
  codes: readonly string[];
  onOpen: (code: string) => void;
}

/** 列表范围、查询和恢复入口在两端共用，容器只改变排布。 */
export function SecurityListControls({ browser, codes, onOpen }: Props) {
  const inputId = useId();
  const searchRef = useRef<HTMLInputElement>(null);
  return <div className="security-list-controls">
    <nav className="security-list-views" aria-label="股票范围">
      {([ ["all", "全部"], ["watchlist", "自选"], ["holdings", "持仓"] ] as const).map(([view, label]) => <button key={view} type="button" aria-pressed={browser.view === view} onClick={() => browser.setView(view)}>{label}</button>)}
      <span className="security-list-count" role="status">{codes.length}只</span>
    </nav>
    <form noValidate className="security-search" role="search" aria-label="股票查询" onSubmit={event => { event.preventDefault(); if (codes.length > 0) onOpen(codes[0]); }}>
      <label className="sr-only" htmlFor={inputId}>搜索股票</label>
      <input ref={searchRef} id={inputId} type="search" placeholder="名称 / 代码" value={browser.query} onChange={event => browser.setQuery(event.currentTarget.value)} onKeyDown={event => { if (event.key === "Escape" && browser.query.length > 0) { event.preventDefault(); event.stopPropagation(); browser.setQuery(""); } }} />
      {browser.query.length > 0 && <button type="button" aria-label="清空搜索" onClick={() => { browser.setQuery(""); searchRef.current?.focus(); }}>×</button>}
    </form>
    {browser.error !== null && <div className="security-list-error" role="alert"><p>{browser.error}</p><button type="button" onClick={browser.reload}>重试读取自选</button></div>}
    {!browser.ready && browser.error === null && <p className="security-list-pending" role="status">正在读取自选…</p>}
  </div>;
}

export function WatchlistToggle({ browser, code }: { browser: SecurityBrowser; code: string }) {
  const selected = browser.favorites.includes(code);
  return <button type="button" className="watchlist-toggle" disabled={!browser.ready} aria-pressed={selected} onClick={() => browser.toggleFavorite(code)}><span aria-hidden="true">{selected ? "★" : "☆"}</span> {selected ? "移出自选" : "加入自选"}</button>;
}
