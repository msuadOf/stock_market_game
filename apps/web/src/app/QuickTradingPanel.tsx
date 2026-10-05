import { useId, useRef, useState } from "react";
import { useSelector } from "react-redux";
import type { RootState } from "../store/store.ts";
import type { SessionSetup } from "../types/engine.ts";
import { STOCK_NAMES } from "../config/defaults.ts";
import { centsToYuanText, moneyToBigInt, moneyFromBigInt } from "../utils/money.ts";
import { formatCentsAmount } from "../utils/format.ts";
import { QuickTrading, fractionLots, type TradeSide, type PriceMode } from "./quick-trading.ts";
import "./quick-trading.css";

import { useTradeRevision } from "./QuickTradingContext.ts";
function TradingTicket({ trading, code, side, setup, notice }: { trading: QuickTrading; code: string; side: TradeSide; setup: SessionSetup; notice(message: string): void }) {
  useTradeRevision(trading);
  const market = useSelector((state: RootState) => state.snapshot.snapshot?.markets[code]);
  const account = useSelector((state: RootState) => state.snapshot.snapshot?.accounts[0]);
  const id = useId(), draft = trading.draft(code, side), name = side === "Buy" ? "买入" : "卖出";
  const [touched, setTouched] = useState(false);
  const priceRef = useRef<HTMLInputElement>(null), quantityRef = useRef<HTMLInputElement>(null);
  const job = trading.jobs().find(item => item.code === code && item.side === side);
  const validation = trading.validation(code, side), error = validation?.message;
  let priceText = draft.priceText, capacity: bigint | null = null, capacityError = "", estimatePrice = "";
  const symbolic = draft.orderKind === "market" || draft.priceMode === "highest" || draft.priceMode === "lowest";
  try { estimatePrice = centsToYuanText(trading.price(code, side)); if (draft.priceMode !== "fixed") priceText = estimatePrice; capacity = trading.capacity(code, side); } catch (failure) { capacityError = failure instanceof Error ? failure.message : String(failure); }
  if (symbolic) priceText = draft.orderKind === "market" ? "市价委托无需价格" : "按受理时规则确定";
  function editPrice(delta: bigint) {
    try { const spec = setup.stocks.find(stock => stock.code === code); if (!spec) throw new Error("当前股票缺少最小价位配置"); const price = moneyToBigInt(trading.price(code, side)) + moneyToBigInt(spec.tick) * delta; trading.edit(code, side, { priceMode: "fixed", priceText: centsToYuanText(moneyFromBigInt(price > 0n ? price : moneyToBigInt(spec.tick))) }); } catch (failure) { notice(failure instanceof Error ? failure.message : String(failure)); }
  }
  function changeQuantity(action: "minus" | "plus" | "divide" | "multiply") {
    if (!/^\d+$/.test(draft.quantityText)) { notice("数量加减需要先输入非负整数手；零股可直接输入小数手"); return; }
    const current = BigInt(draft.quantityText);
    const next = action === "minus" ? (current > 0n ? current - 1n : 0n) : action === "plus" ? current + 1n : action === "divide" ? current / 10n : current * 10n;
    trading.edit(code, side, { quantityText: String(next) }); setTouched(true);
  }
  const prices: [PriceMode, string][] = [["lowest", "最小"], ["bid", "B1"], ["ask", "S1"], ["highest", "最大"]];
  return <section className="trade-ticket" data-side={side} aria-label={`${name}委托`}>
    <div className="trade-ticket-head"><strong className={side === "Buy" ? "up" : "down"}>{name}</strong><span>{STOCK_NAMES[code] ?? code}</span><span className="mono">{code}</span></div>
    <div className="trade-ticket-row"><label htmlFor={`${id}-kind`}>类型</label><select id={`${id}-kind`} aria-label="委托类型" value={draft.orderKind} onChange={event => trading.edit(code, side, { orderKind: event.target.value as "limit" | "market" })}><option value="limit">限价委托</option><option value="market">市价委托</option></select></div>
    <div className="trade-ticket-row"><label htmlFor={`${id}-price`}>价格（元）</label><input ref={priceRef} id={`${id}-price`} inputMode="decimal" value={priceText} aria-invalid={Boolean(touched && validation?.field === "price")} aria-describedby={touched && validation?.field === "price" ? `${id}-error` : undefined} disabled={draft.orderKind === "market" || draft.priceMode !== "fixed"} placeholder={draft.orderKind === "market" ? "市价委托无需价格" : "委托价"} onChange={event => { trading.edit(code, side, { priceMode: "fixed", priceText: event.target.value }); setTouched(true); }} /><button type="button" aria-label={`${name}价格减少`} disabled={draft.orderKind === "market"} onClick={() => editPrice(-1n)}>−</button><button type="button" aria-label={`${name}价格增加`} disabled={draft.orderKind === "market"} onClick={() => editPrice(1n)}>＋</button></div>
    <div className="trade-price-options" role="group" aria-label={`${name}选价`}>{prices.map(([mode, label]) => <button type="button" key={mode} aria-pressed={draft.priceMode === mode} disabled={draft.orderKind === "market" || ((mode === "bid" ? market?.best_bid : mode === "ask" ? market?.best_ask : true) === null)} onClick={() => trading.edit(code, side, { priceMode: mode })}>{label}</button>)}<button type="button" aria-pressed={draft.priceMode === "fixed"} disabled={draft.orderKind === "market"} onClick={() => trading.edit(code, side, { priceMode: "fixed", priceText: symbolic ? estimatePrice : priceText })}>指定</button></div>
    <div className="trade-ticket-row"><label htmlFor={`${id}-quantity`}>数量（手）</label><input ref={quantityRef} id={`${id}-quantity`} inputMode="decimal" value={draft.quantityText} placeholder="买入按手；零股一次卖完" aria-invalid={Boolean(touched && validation?.field === "quantity")} aria-describedby={touched && error && validation?.field === "quantity" ? `${id}-error` : `${id}-capacity`} onChange={event => { trading.edit(code, side, { quantityText: event.target.value }); setTouched(true); }} /><button type="button" aria-label={`${name}数量减少`} onClick={() => changeQuantity("minus")}>−</button><button type="button" aria-label={`${name}数量增加`} onClick={() => changeQuantity("plus")}>＋</button></div>
    <div className="trade-quantity-options" role="group" aria-label={`${name}数量比例`}>{[5, 4, 3, 2, 1].map(divisor => <button type="button" key={divisor} disabled={capacity === null} onClick={() => { if (capacity !== null) trading.edit(code, side, { quantityText: String(fractionLots(capacity, divisor)) }); setTouched(true); }}>{divisor === 1 ? "全部" : `1/${divisor}`}</button>)}<button type="button" onClick={() => changeQuantity("divide")}>÷10</button><button type="button" onClick={() => changeQuantity("multiply")}>×10</button></div>
    <div className="trade-capacity" id={`${id}-capacity`}>{capacity === null ? capacityError : `可${side === "Buy" ? "买" : "卖"} ${capacity} 手`}{side === "Buy" && account ? ` · 可用 ${formatCentsAmount(moneyFromBigInt(moneyToBigInt(account.cash) - moneyToBigInt(account.reserved_cash)))}元` : ""}{symbolic && estimatePrice && <div>预留估算边界 {estimatePrice}元；非确定成交价</div>}</div>
    <div className="trade-submit-row"><button type="button" className={`trade-submit ${side === "Buy" ? "up" : "down"}`} disabled={trading.busy(code, side)} onClick={async () => { setTouched(true); const failure = trading.validation(code, side); if (failure?.field === "price") priceRef.current?.focus(); else if (failure?.field === "quantity") quantityRef.current?.focus(); const success = await trading.submit(code, side); notice(success ? `已提交${name}委托：${code} ${draft.quantityText}手，等待引擎受理` : trading.error(code, side) ?? trading.status(code, side)); }}>{trading.busy(code, side) ? "提交中…" : name}</button><label>间隔<select aria-label={`${name}自动间隔`} value={draft.interval} onChange={event => trading.edit(code, side, { interval: Number(event.target.value) })}>{[1, 1.5, 2, 3, 5, 10].map(value => <option key={value} value={value}>{value}s</option>)}</select></label><button type="button" aria-pressed={job?.enabled === true} onClick={() => { try { trading.toggleAuto(code, side); } catch (failure) { notice(String(failure)); } }}>自动</button></div>
    <div className="trade-ticket-feedback" role="status">{touched && error ? <span id={`${id}-error`} className="field-error">{error}</span> : trading.status(code, side)}</div>
  </section>;
}
export function QuickTradingPanel({ trading, code, setup, side, onSideChange, onSelect, notice }: { trading: QuickTrading; code: string; setup: SessionSetup; side: TradeSide; onSideChange(side: TradeSide): void; onSelect(code: string): void; notice(message: string): void }) {
  useTradeRevision(trading);
  return <div className="quick-trading" data-active-side={side}>
    <label className="quick-trading-stock">股票<select aria-label="股票" value={code} onChange={event => onSelect(event.target.value)}>{setup.stocks.map(stock => <option key={stock.code} value={stock.code}>{STOCK_NAMES[stock.code] ?? stock.code} {stock.code}</option>)}</select></label>
    <div className="trade-side-tabs" role="group" aria-label="买卖方向">{(["Buy", "Sell"] as const).map(value => <button type="button" key={value} aria-pressed={side === value} onClick={() => onSideChange(value)}>{value === "Buy" ? "买入" : "卖出"}</button>)}</div>
    <div className="trade-tickets">{(["Sell", "Buy"] as const).map(value => <TradingTicket key={`${code}:${value}`} trading={trading} code={code} side={value} setup={setup} notice={notice} />)}</div>
    <p className="trade-timing-note">间隔按游戏时间计，到期后随游戏推进执行；暂停不下单。1手＝100股，零股可输入小数手。</p>
    <div className="repeating-trades" aria-label="自动下单任务">{trading.jobs().filter(job => job.enabled).map(job => <div key={`${job.code}:${job.side}`}><span>{STOCK_NAMES[job.code] ?? job.code} {job.code} · 自动{job.side === "Buy" ? "买入" : "卖出"}</span><span role="status">{job.status}</span><button type="button" onClick={() => trading.toggleAuto(job.code, job.side)}>停止自动</button></div>)}</div>
  </div>;
}
