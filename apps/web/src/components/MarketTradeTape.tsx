import { useId, useState } from "react";
import type { MarketSnap, TradeEvent } from "../types/engine.ts";
import { colorClass, formatSharesAsLots, yuan } from "../utils/format.ts";
import { compareMoney } from "../utils/money.ts";
import { formatTradeTime } from "../mobile/market-model.ts";
import "./market-quote.css";

interface Props {
  code: string;
  market: MarketSnap;
  trades: readonly TradeEvent[];
}

/** 两端共用证券筛选、时间、精确价格和手数；缓存不冒充完整逐笔历史。 */
export function MarketTradeTape({ code, market, trades }: Props) {
  const [expanded, setExpanded] = useState(false);
  const id = useId();
  const selected = trades.filter(trade => trade.code === code);
  const rows = expanded ? selected : selected.slice(0, 7);
  return <section className="market-trade-tape" data-expanded={expanded}>
    <div className="market-tape-heading"><strong><span className="market-tape-prefix">成交</span>明细</strong><span className="market-tape-cache-label">最近成交缓存</span>{selected.length > 7 && <button type="button" className="market-tape-expand" aria-label={expanded ? "收起明细" : `显示缓存明细（${selected.length}笔）`} title={expanded ? "收起明细" : `显示缓存明细（${selected.length}笔）`} aria-expanded={expanded} aria-controls={id} onClick={() => setExpanded(value => !value)}>{expanded ? "收起⌃" : "展开⌄"}</button>}</div>
    <div className="market-tape-scroll" id={id}>
      <table aria-label={`${code} 最近逐笔成交，数量单位为手`}>
        <thead><tr><th scope="col">时间</th><th scope="col" aria-label="价格（元）">价格<span className="market-tape-unit">（元）</span></th><th scope="col" aria-label="量（手）">量<span className="market-tape-unit">（手）</span></th></tr></thead>
        <tbody>{rows.length === 0
          ? <tr><td colSpan={3} className="market-tape-empty">暂无该股票的成交缓存</td></tr>
          : rows.map(trade => <tr key={trade.seq} data-trade-seq={trade.seq}>
            <td data-time-missing={trade.tick === undefined || undefined}>{formatTradeTime(trade.tick)}</td>
            <td className={colorClass(compareMoney(trade.price, market.last_close))}>{yuan(trade.price)}</td>
            <td>{formatSharesAsLots(trade.qty)}</td>
          </tr>)}</tbody>
      </table>
    </div>
    {expanded && <p className="market-tape-note">仅展示当前缓存，非完整逐笔历史。</p>}
  </section>;
}
