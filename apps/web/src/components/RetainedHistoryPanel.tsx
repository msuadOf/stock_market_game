import { useEffect, useRef, useState } from "react";
import type { MarketHistoryRequest, MarketHistoryPage } from "../host/market-history.ts";
import { latestFiveTradingDays } from "./retained-history-model.ts";
import { yuan, formatDecimalCentsAsYuan } from "../utils/format.ts";
import "./RetainedHistoryPanel.css";

export interface RetainedHistoryPanelProps {
  readonly query: (request: MarketHistoryRequest) => Promise<MarketHistoryPage>;
  readonly scopeKey: string;
  readonly code: string;
  readonly currentDate: string | null;
  readonly fiveDay?: boolean;
}

const statuses = { Traded: "有真实成交", NoTrades: "开市但零成交", Closed: "本证券休市", BeforeStart: "开局前：没有本局分钟成交", NotEnded: "尚未完成日结：未归档" };
const phases = { OpenAuction: "开盘集合竞价", Continuous: "连续竞价", ClosingAuction: "收盘集合竞价" };

function HistoryDay({ entry }: { entry: MarketHistoryPage["entries"][number] }) {
  const prices = entry.bars.map((bar) => Number(bar.close) / 100);
  const minimum = Math.min(...prices);
  const maximum = Math.max(...prices);
  const span = Math.max(maximum - minimum, 0.01);
  const priceY = (price: number) => 90 - (price - minimum) / span * 80;
  const minuteX = (minute: number) => 10 + (minute - 565) / 335 * 480;
  return <section className="retained-history-day" aria-label={`${entry.date}分钟历史`}>
    <h4>{entry.date}　{statuses[entry.availability]}</h4>
    {entry.bars.length > 0 && <>
      <svg viewBox="0 0 500 100" role="img" aria-label={`${entry.date}真实分钟收盘价，空白分钟不填充`}>
        {entry.bars.map((bar, index) => <circle key={`${bar.minute_of_day}:${bar.phase}`} cx={minuteX(bar.minute_of_day)} cy={priceY(prices[index])} r="2" fill="var(--text-secondary, #315a8c)"><title>{`${String(Math.floor(bar.minute_of_day / 60)).padStart(2, "0")}:${String(bar.minute_of_day % 60).padStart(2, "0")} ${phases[bar.phase]} 收${yuan(bar.close)}元 成交${bar.volume_shares}股`}</title></circle>)}
      </svg>
      <div className="retained-history-table"><table><caption>真实稀疏分钟OHLCV及成交额，不含竞价指示量</caption><thead><tr><th>时刻／阶段</th><th>开／高／低／收（元）</th><th>成交股数</th><th>成交额（元）</th><th>笔数</th></tr></thead><tbody>{entry.bars.map((bar) => <tr key={`${bar.minute_of_day}:${bar.phase}`}><td>{String(Math.floor(bar.minute_of_day / 60)).padStart(2, "0")}:{String(bar.minute_of_day % 60).padStart(2, "0")} {phases[bar.phase]}</td><td>{[bar.open, bar.high, bar.low, bar.close].map(yuan).join("／")}</td><td>{bar.volume_shares}</td><td>{formatDecimalCentsAsYuan(bar.turnover_cents)}</td><td>{bar.trade_count}</td></tr>)}</tbody></table></div>
    </>}
    {entry.availability === "BeforeStart" && entry.daily_candle !== null && <p>此日存在生成的虚拟前史日K，不据此推算分钟成交。</p>}
  </section>;
}

export function RetainedHistoryPanel({ query, scopeKey, code, currentDate, fiveDay = false }: RetainedHistoryPanelProps) {
  const [from, setFrom] = useState(currentDate === null ? "" : currentDate);
  const [to, setTo] = useState(currentDate === null ? "" : currentDate);
  const [pages, setPages] = useState<readonly MarketHistoryPage[]>([]);
  const [index, setIndex] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const owner = useRef({ alive: false, sequence: 0 });
  useEffect(() => {
    owner.current.alive = true;
    owner.current.sequence += 1;
    setPages([]); setIndex(0); setLoading(false); setError(null);
    return () => { owner.current.alive = false; owner.current.sequence += 1; };
  }, [scopeKey, code, query, fiveDay]);
  const invalidate = () => { owner.current.sequence += 1; setPages([]); setIndex(0); setLoading(false); setError(null); };
  const load = async (request: MarketHistoryRequest | null, previous: readonly MarketHistoryPage[]) => {
    const sequence = ++owner.current.sequence;
    setLoading(true); setError(null);
    const scopedQuery = async (input: MarketHistoryRequest) => {
      if (!owner.current.alive || owner.current.sequence !== sequence) throw new Error("历史查询范围已切换");
      const page = await query(input);
      if (!owner.current.alive || owner.current.sequence !== sequence) throw new Error("历史响应属于已切换范围");
      return page;
    };
    try {
      if (request === null && currentDate === null) throw new Error("当前自然日尚未同步，不能选择五日窗口");
      const page = request === null ? await latestFiveTradingDays(scopedQuery, code, currentDate!) : await scopedQuery(request);
      if (!owner.current.alive || owner.current.sequence !== sequence) return;
      setPages([...previous, page]); setIndex(previous.length);
    } catch (failure) {
      if (owner.current.alive && owner.current.sequence === sequence) setError(`分钟历史查询失败：${failure instanceof Error ? failure.message : String(failure)}；请核对日期后重试并反馈错误详情。`);
    } finally { if (owner.current.alive && owner.current.sequence === sequence) setLoading(false); }
  };
  const refresh = () => { invalidate(); void load(fiveDay ? null : { code, date_from: from, date_to: to, after: null, page_size: 5 }, []); };
  const page = pages[index];
  return <section className="retained-history" aria-label={fiveDay ? "五日分时历史" : "按日期分页分钟历史"}>
    <h3>{fiveDay ? "五日分时（最近五个已结束开市日）" : "长期分钟历史"}</h3>
    <p>历史全部保留，五日仅为展示窗口；只展示日结后真实成交，零成交与休市分开，不生成虚拟分钟。当前日请查看分时。</p>
    {!fiveDay && <form onSubmit={(event) => { event.preventDefault(); refresh(); }}><label>起始日期<input aria-label="分钟历史起始日期" type="date" value={from} onChange={(event) => { setFrom(event.currentTarget.value); invalidate(); }} required /></label><label>结束日期<input aria-label="分钟历史结束日期" type="date" value={to} onChange={(event) => { setTo(event.currentTarget.value); invalidate(); }} required /></label><button type="submit" disabled={loading}>查询范围</button></form>}
    {fiveDay && <button type="button" disabled={loading || currentDate === null} onClick={refresh}>读取／刷新五日</button>}
    {!fiveDay && <div><button type="button" disabled={loading || index === 0} onClick={() => setIndex(index - 1)}>上一页</button><span>第{index + 1}页，每页5个自然日</span><button type="button" disabled={loading || page === undefined || page.next_cursor === null} onClick={() => {
      if (page === undefined || page.next_cursor === null) throw new Error("分钟历史没有下一页游标");
      if (index + 1 < pages.length) { setIndex(index + 1); return; }
      void load({ code, date_from: from, date_to: to, after: page.next_cursor, page_size: 5 }, pages.slice(0, index + 1));
    }}>下一页</button></div>}
    {loading && <p role="status">正在查询已结束日期…</p>}{error !== null && <p role="alert">{error}</p>}
    {page !== undefined && <><p>日终历史已结束至：{page.settled_through === null ? "尚未首次日结" : page.settled_through}。{fiveDay && page.entries.length < 5 ? `本局仅有${page.entries.length}个已结束开市日，不补造不足五日的数据。` : ""}</p>{page.entries.map((entry) => <HistoryDay key={entry.date} entry={entry} />)}</>}
  </section>;
}
