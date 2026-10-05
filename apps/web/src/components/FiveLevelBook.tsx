import { handleBookPick, useBookTrading } from "../app/QuickTradingContext.ts";
import type { CSSProperties } from "react";
import type { MarketSnap } from "../types/engine.ts";
import { compareMoney } from "../utils/money.ts";
import { yuan } from "../utils/format.ts";
import { buildFiveLevelBook, formatTradeLots, orderBookDepthPercent } from "../mobile/market-model.ts";
function tone(diff: number) { return diff > 0 ? "rise" : diff < 0 ? "fall" : "flat"; }
export function FiveLevelBook({ market, code }: { market: MarketSnap; code?: string }) {
  const trading = useBookTrading();
  const interactive = trading !== null && code !== undefined;
  function pick(side: "Buy" | "Sell", label: string, field: "price" | "quantity") { if (trading && code) handleBookPick(trading, code, side, Number(label.slice(1)) - 1, field); }
  const book = buildFiveLevelBook(market.bids, market.asks);
  const sellMaximum = Math.max(1, ...book.sells.flatMap(({ level }) => level ? [level[1]] : []));
  const buyMaximum = Math.max(1, ...book.buys.flatMap(({ level }) => level ? [level[1]] : []));
  return <aside className="msd-order-book" aria-label="五档盘口，数量单位为手">
    <div className="msd-book-head"><b className={tone(compareMoney(market.last_price, market.last_close))}>大单 <small>量/手</small></b><span>{yuan(market.last_price)}</span></div>
    {book.sells.map(({ label, level }) => <div className="msd-book-row" key={label}><span>{label}</span><b className={level ? tone(compareMoney(level[0], market.last_close)) : "flat"}>{interactive && level ? <button type="button" aria-label={`${label}价格 ${yuan(level[0])}`} onClick={() => pick(label.startsWith("卖") ? "Buy" : "Sell", label, "price")}>{yuan(level[0])}</button> : level ? yuan(level[0]) : "--"}</b><span className="msd-book-depth sell" style={{ "--depth": `${level ? orderBookDepthPercent(level[1], sellMaximum) : 0}%` } as CSSProperties}><span>{interactive && level ? <button type="button" aria-label={`${label}手数 ${formatTradeLots(level[1])}`} onClick={() => pick(label.startsWith("卖") ? "Buy" : "Sell", label, "quantity")}>{formatTradeLots(level[1])}</button> : level ? formatTradeLots(level[1]) : "--"}</span></span></div>)}
    <div className="msd-book-divider" />
    {book.buys.map(({ label, level }) => <div className="msd-book-row" key={label}><span>{label}</span><b className={level ? tone(compareMoney(level[0], market.last_close)) : "flat"}>{interactive && level ? <button type="button" aria-label={`${label}价格 ${yuan(level[0])}`} onClick={() => pick(label.startsWith("卖") ? "Buy" : "Sell", label, "price")}>{yuan(level[0])}</button> : level ? yuan(level[0]) : "--"}</b><span className="msd-book-depth buy" style={{ "--depth": `${level ? orderBookDepthPercent(level[1], buyMaximum) : 0}%` } as CSSProperties}><span>{interactive && level ? <button type="button" aria-label={`${label}手数 ${formatTradeLots(level[1])}`} onClick={() => pick(label.startsWith("卖") ? "Buy" : "Sell", label, "quantity")}>{formatTradeLots(level[1])}</button> : level ? formatTradeLots(level[1]) : "--"}</span></span></div>)}
  </aside>;
}
