import type { PersonalTradeHistoryRequest, PersonalTradeHistoryPage } from "../host/engine-host.ts";
import { useEffect, useRef, useState } from "react";
import { normalizePersonalTradeHistoryRequest, normalizePersonalTradeHistoryPage } from "../host/personal-trade-history.ts";
import { TradeConfirmationTable } from "./TradeConfirmationTable.tsx";

export interface PersonalTradeHistoryPanelProps {
  readonly query: (request: PersonalTradeHistoryRequest) => Promise<PersonalTradeHistoryPage>;
  readonly scopeKey: string;
  readonly initialDate?: string | null;
  readonly codes?: readonly string[];
}

export function PersonalTradeHistoryPanel({ query, scopeKey, initialDate = null, codes = [] }: PersonalTradeHistoryPanelProps) {
  const [from, setFrom] = useState(initialDate === null ? "" : initialDate);
  const [to, setTo] = useState(initialDate === null ? "" : initialDate);
  const [code, setCode] = useState<string>("");
  const [side, setSide] = useState<string>("");
  const [pages, setPages] = useState<readonly PersonalTradeHistoryPage[]>([]);
  const [pageIndex, setPageIndex] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const owner = useRef({ sequence: 0, alive: false });
  useEffect(() => {
    owner.current.alive = true;
    owner.current.sequence += 1;
    setPages([]);
    setPageIndex(0);
    setLoading(false);
    setError(null);
    return () => { owner.current.alive = false; owner.current.sequence += 1; };
  }, [scopeKey, query]);
  const invalidate = () => {
    owner.current.sequence += 1;
    setPages([]);
    setPageIndex(0);
    setError(null);
    setLoading(false);
  };
  const queryPage = async (request: PersonalTradeHistoryRequest, previous: readonly PersonalTradeHistoryPage[]) => {
    const sequence = ++owner.current.sequence;
    setLoading(true);
    setError(null);
    try {
      const normalized = normalizePersonalTradeHistoryRequest(request);
      const page = normalizePersonalTradeHistoryPage(await query(normalized), normalized);
      if (!owner.current.alive || sequence !== owner.current.sequence) return;
      setPages([...previous, page]);
      setPageIndex(previous.length);
    } catch (failure) {
      if (owner.current.alive && sequence === owner.current.sequence) setError(`本人日期交割历史查询失败：${failure instanceof Error ? failure.message : String(failure)}；请核对日期和筛选后重试，保留错误详情反馈。`);
    } finally {
      if (owner.current.alive && sequence === owner.current.sequence) setLoading(false);
    }
  };
  const refresh = () => { void queryPage({ date_from: from, date_to: to, code: code === "" ? null : code, side: side === "" ? null : side as "Buy" | "Sell", before_receipt: null, as_of_receipt: null, page_size: 100 }, []); };
  const page = pages[pageIndex];
  const next = () => {
    if (page === undefined || page.next_cursor === null) throw new Error("日期交割页没有下一receipt游标");
    if (pageIndex + 1 < pages.length) { setPageIndex(pageIndex + 1); return; }
    void queryPage({ ...page.request, before_receipt: page.next_cursor, as_of_receipt: page.as_of_receipt }, pages.slice(0, pageIndex + 1));
  };
  return <details>
    <summary>按日期复盘本人交割单</summary>
    <section aria-label="按日期复盘本人交割单">
      <p>读取本人真实Fill与实际费用，日期两端包含；不会记录行情读取经历，不计算或猜测盈亏归因。</p>
      <form onSubmit={(event) => { event.preventDefault(); refresh(); }}>
        <label>起始自然日<input aria-label="交割起始自然日" type="date" value={from} onChange={(event) => { setFrom(event.currentTarget.value); invalidate(); }} required /></label>
        <label>结束自然日<input aria-label="交割结束自然日" type="date" value={to} onChange={(event) => { setTo(event.currentTarget.value); invalidate(); }} required /></label>
        <label>证券<select aria-label="交割证券筛选" value={code} onChange={(event) => { setCode(event.currentTarget.value); invalidate(); }}><option value="">全部证券</option>{codes.map((stock) => <option key={stock} value={stock}>{stock}</option>)}</select></label>
        <label>方向<select aria-label="交割方向筛选" value={side} onChange={(event) => { setSide(event.currentTarget.value); invalidate(); }}><option value="">全部方向</option><option value="Buy">买入</option><option value="Sell">卖出</option></select></label>
        <button type="submit" disabled={loading}>查询／刷新范围</button>
      </form>
      <button type="button" disabled={loading || pageIndex === 0} onClick={() => setPageIndex(pageIndex - 1)}>上一页</button>
      <span>第{pageIndex + 1}页（每页最多100条；历史全部保留）</span>
      <button type="button" disabled={loading || page?.next_cursor == null} onClick={next}>下一页</button>
      {page !== undefined && <p>开局自然日{page.start_date}；当前自然日{page.current_date}；已结束至{page.settled_through === null ? "尚未完成首次日结" : page.settled_through}。{page.request.date_from < page.start_date ? "范围包含开局前，不存在本局成交事实。" : ""}{page.request.date_to > page.current_date ? "范围包含未发生日期。" : ""}{page.request.date_to >= page.current_date ? "当前日尚未日结，只含已成功提交的真实成交，未写入日终存档。" : ""}</p>}
      {page !== undefined && page.confirmations.length === 0 && !loading && error === null && <p>该日期及筛选范围无本人成交；不能据此推断市场休市或缺少量价历史。</p>}
      <TradeConfirmationTable rows={page?.confirmations ?? []} loading={loading} error={error} hasQueried={page !== undefined} onRefresh={refresh} />
    </section>
  </details>;
}
