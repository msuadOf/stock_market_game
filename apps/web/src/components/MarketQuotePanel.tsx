import { useId, useRef, useState, type KeyboardEvent } from "react";
import type { MarketSnap, TradeEvent } from "../types/engine.ts";
import type { KlinePoint } from "./PriceChart.tsx";
import { colorClass, formatDecimalCentsAsYuan, formatSharesAsLots, yuan } from "../utils/format.ts";
import { compareMoney } from "../utils/money.ts";
import { FiveLevelBook } from "./FiveLevelBook.tsx";
import { MarketTradeTape } from "./MarketTradeTape.tsx";
import { marketQuoteFacts } from "./market-quote-facts.ts";
import "./market-quote.css";

const tabs = ["盘口", "明细", "行情"] as const;
type QuoteTab = typeof tabs[number];
interface Props {
  code: string;
  market: MarketSnap;
  candle: KlinePoint | undefined;
  trades: readonly TradeEvent[];
}

function QuoteFacts({ market, candle }: Pick<Props, "market" | "candle">) {
  const facts = marketQuoteFacts(candle);
  const priceRows = [["昨收", market.last_close], ["今开", facts.prices?.open], ["最高", facts.prices?.high], ["最低", facts.prices?.low]] as const;
  return <section className="market-quote-facts" aria-label="当日行情摘要">
    <div className="market-tape-heading"><strong>当日行情</strong></div>
    <dl>{priceRows.map(([label, price]) => <div key={label}><dt>{label}</dt><dd className={price === undefined || label === "昨收" ? "" : colorClass(compareMoney(price, market.last_close))}>{price === undefined ? "--" : yuan(price)}</dd></div>)}
      <div><dt>总量（手）</dt><dd>{formatSharesAsLots(facts.volume)}</dd></div>
      <div><dt>成交额（元）</dt><dd>{facts.turnoverCents === null ? "--" : formatDecimalCentsAsYuan(facts.turnoverCents)}</dd></div>
      <div><dt>成交笔数</dt><dd>{facts.tradeCount === null ? "--" : facts.tradeCount}</dd></div>
    </dl>
    {facts.volume === 0 && <p>尚无成交，开盘价及高低价未形成。</p>}
    {facts.statsUnavailable && <p role="status">当前数据缺少可对账的成交额与笔数，进入下一交易日后恢复完整统计。</p>}
  </section>;
}

export function MarketQuotePanel(props: Props) {
  const [tab, setTab] = useState<QuoteTab>("盘口");
  const id = useId();
  const tablist = useRef<HTMLDivElement>(null);
  function onKeyDown(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    let next: number;
    switch (event.key) {
      case "ArrowRight": next = (index + 1) % tabs.length; break;
      case "ArrowLeft": next = (index + tabs.length - 1) % tabs.length; break;
      case "Home": next = 0; break;
      case "End": next = tabs.length - 1; break;
      default: return;
    }
    event.preventDefault();
    setTab(tabs[next]!);
    tablist.current?.querySelectorAll<HTMLButtonElement>('[role="tab"]')[next]?.focus();
  }
  return <>
    <div className="market-quote-tabs" role="tablist" aria-label="个股行情信息" ref={tablist}>
      {tabs.map((item, index) => <button key={item} type="button" role="tab" id={`${id}-tab-${index}`} aria-controls={`${id}-panel-${index}`} aria-selected={tab === item} tabIndex={tab === item ? 0 : -1} onKeyDown={event => onKeyDown(event, index)} onClick={() => setTab(item)}>{item}</button>)}
    </div>
    <div role="tabpanel" tabIndex={0} id={`${id}-panel-0`} aria-labelledby={`${id}-tab-0`} hidden={tab !== "盘口"}><div className="ob-title">五档盘口<span>价格（元） / 数量（手）</span></div><FiveLevelBook code={props.code} market={props.market} /></div>
    <div role="tabpanel" tabIndex={0} id={`${id}-panel-1`} aria-labelledby={`${id}-tab-1`} hidden={tab !== "明细"}><MarketTradeTape key={props.code} code={props.code} market={props.market} trades={props.trades} /></div>
    <div role="tabpanel" tabIndex={0} id={`${id}-panel-2`} aria-labelledby={`${id}-tab-2`} hidden={tab !== "行情"}><QuoteFacts market={props.market} candle={props.candle} /></div>
  </>;
}
