import { useEffect, useRef, useState } from "react";
import type { MarketHistoryRequest, MarketHistoryPage } from "../host/market-history.ts";
import { yuan, formatDecimalCentsAsYuan } from "../utils/format.ts";
import "./RetainedHistoryPanel.css";

export interface RetainedHistoryPanelProps {
  readonly query: (request: MarketHistoryRequest) => Promise<MarketHistoryPage>;
  readonly scopeKey: string;
  readonly code: string;
  readonly currentDate: string | null;
}

const statuses = { Traded: "已日结，有真实成交", NoTrades: "开市但零成交", Closed: "本证券休市", BeforeStart: "开局前", NotEnded: "尚未完成日结：未归档" };
const PAGE_SIZE = 31;

function HistoryDay({ entry }: { entry: MarketHistoryPage["entries"][number] }) {
  const candle = entry.daily_candle;
  const stats = candle?.trade_stats;
  return <section className="retained-history-day" aria-label={`${entry.date}日K`}>
    <h4>{entry.date}　{statuses[entry.availability]}</h4>
    {entry.availability === "BeforeStart" && candle !== null && <p>此日为生成的虚拟前史日K，不是本局真实成交。</p>}
    {candle !== null && <div className="retained-history-table"><table>
      <caption>{entry.date}日K</caption><thead><tr><th scope="col">日K开/高/低/收（元）</th><th scope="col">成交股数</th><th scope="col">成交额（元）</th><th scope="col">成交笔数</th></tr></thead>
      <tbody><tr><td>{[candle.open, candle.high, candle.low, candle.close].map(yuan).join("／")}</td><td>{candle.volume}</td><td>{stats == null ? "无真实统计" : formatDecimalCentsAsYuan(stats.turnover_cents)}</td><td>{stats == null ? "无真实统计" : stats.trade_count}</td></tr></tbody>
    </table></div>}
  </section>;
}

export function RetainedHistoryPanel({ query, scopeKey, code, currentDate }: RetainedHistoryPanelProps) {
  const [from, setFrom] = useState(currentDate === null ? "" : currentDate);
  const [to, setTo] = useState(currentDate === null ? "" : currentDate);
  const [pages, setPages] = useState<readonly MarketHistoryPage[]>([]);
  const [index, setIndex] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const owner = useRef({ alive: false, sequence: 0 });
  useEffect(() => {
    const requestOwner = owner.current;
    requestOwner.alive = true;
    requestOwner.sequence += 1;
    setPages([]); setIndex(0); setLoading(false); setError(null);
    return () => { requestOwner.alive = false; requestOwner.sequence += 1; };
  }, [scopeKey, code, query]);
  const invalidate = () => { owner.current.sequence += 1; setPages([]); setIndex(0); setLoading(false); setError(null); };
  const load = async (request: MarketHistoryRequest, previous: readonly MarketHistoryPage[]) => {
    const sequence = ++owner.current.sequence;
    setLoading(true); setError(null);
    try {
      const page = await query(request);
      if (!owner.current.alive || owner.current.sequence !== sequence) return;
      setPages([...previous, page]); setIndex(previous.length);
    } catch (failure) {
      if (owner.current.alive && owner.current.sequence === sequence) setError(`历史日K查询失败：${failure instanceof Error ? failure.message : String(failure)}；请核对日期后重试并反馈错误详情。`);
    } finally { if (owner.current.alive && owner.current.sequence === sequence) setLoading(false); }
  };
  const refresh = () => {
    invalidate();
    if (!from || !to || from > to) {
      setError("请选择完整的日期范围，起始日期不能晚于结束日期。");
      return;
    }
    void load({ code, date_from: from, date_to: to, after: null, page_size: PAGE_SIZE }, []);
  };
  const page = pages[index];
  return <section className="retained-history" aria-label="按日期分页查询历史日K">
    <h3>历史日K</h3>
    <p>日结后只保留日K。休市与零成交分别标记；当前交易日请查看分时。</p>
    <form onSubmit={(event) => { event.preventDefault(); refresh(); }}><label>起始日期<input aria-label="历史日K起始日期" type="date" value={from} onChange={(event) => { setFrom(event.currentTarget.value); invalidate(); }} required /></label><label>结束日期<input aria-label="历史日K结束日期" type="date" value={to} onChange={(event) => { setTo(event.currentTarget.value); invalidate(); }} required /></label><button type="submit" disabled={loading}>查询范围</button></form>
    <div><button type="button" disabled={loading || index === 0} onClick={() => setIndex(index - 1)}>上一页</button><span>第{index + 1}页，每页{PAGE_SIZE}个自然日</span><button type="button" disabled={loading || page === undefined || page.next_cursor === null} onClick={() => {
      if (page === undefined || page.next_cursor === null) throw new Error("历史日K没有下一页游标");
      if (index + 1 < pages.length) { setIndex(index + 1); return; }
      void load({ code, date_from: from, date_to: to, after: page.next_cursor, page_size: PAGE_SIZE }, pages.slice(0, index + 1));
    }}>下一页</button></div>
    {loading && <p role="status">正在按日期查询日K…</p>}{error !== null && <p role="alert">{error}</p>}
    {page !== undefined && <><p>日终历史已结束至：{page.settled_through === null ? "尚未首次日结" : page.settled_through}。</p>{page.entries.map((entry) => <HistoryDay key={entry.date} entry={entry} />)}</>}
  </section>;
}
