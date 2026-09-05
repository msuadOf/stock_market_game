import { useMemo, useState } from "react";
import type { KlinePoint, PricePoint } from "../components/PriceChart";
import { PriceChart } from "../components/PriceChart";
import type { MarketSnap, TradeEvent } from "../types/engine";
import { formatTradingMinute, tradingDayProgress } from "./market-model";
import "./MobileStockDetail.css";

type ChartPeriod = "分时" | "日K" | "周K" | "月K" | "五日" | "更多";
type InfoTab = "看点" | "资讯" | "盘口" | "资金" | "社区" | "简况";

interface Props {
  code: string;
  name: string;
  market: MarketSnap;
  minutePoints: PricePoint[];
  dailyCandles: KlinePoint[];
  trades: TradeEvent[];
  elapsedMinutes: number;
  totalMinutes: number;
  klineDays: number;
  onKlineDaysChange: (days: number) => void;
  onBack: () => void;
}

function yuan(cents: number): string {
  return (cents / 100).toFixed(2);
}

function largeMoney(cents: number): string {
  const yuanValue = cents / 100;
  return yuanValue >= 100_000_000 ? `${(yuanValue / 100_000_000).toFixed(2)}亿` : `${(yuanValue / 10_000).toFixed(2)}万`;
}

function tone(diff: number): "rise" | "fall" | "flat" {
  return diff > 0 ? "rise" : diff < 0 ? "fall" : "flat";
}

function IntradayPanel({ market, minutePoints, trades, elapsedMinutes, totalMinutes }: Pick<Props, "market" | "minutePoints" | "trades" | "elapsedMinutes" | "totalMinutes">) {
  const visiblePoints = minutePoints.slice(-totalMinutes);
  const values = visiblePoints.map((point) => point.value);
  const lastClose = market.last_close / 100;
  const highest = Math.max(lastClose, ...values);
  const lowest = Math.min(lastClose, ...values);
  const distance = Math.max(highest - lastClose, lastClose - lowest, lastClose * 0.006, 0.01);
  const top = lastClose + distance * 1.12;
  const bottom = lastClose - distance * 1.12;
  const y = (value: number) => 8 + ((top - value) / (top - bottom)) * 84;
  const linePoints = visiblePoints
    .map((point, index) => `${(index / Math.max(totalMinutes - 1, 1)) * 100},${y(point.value)}`)
    .join(" ");
  const averagePoints = visiblePoints
    .map((_, index) => {
      const average = visiblePoints.slice(0, index + 1).reduce((sum, point) => sum + point.value, 0) / (index + 1);
      return `${(index / Math.max(totalMinutes - 1, 1)) * 100},${y(average)}`;
    })
    .join(" ");
  const progress = tradingDayProgress(elapsedMinutes, totalMinutes);
  const maxVolume = Math.max(1, ...visiblePoints.map((point) => point.volume ?? 0));
  const asks = market.asks.slice(0, 5).reverse();
  const bids = market.bids.slice(0, 5);
  const recentTrades = trades.slice(-7).reverse();

  return (
    <section className="msd-market-composite" aria-label="分时、盘口、分时量和逐笔成交">
      <div className="msd-intraday-main">
        <div className="msd-chart-meta">
          <span>集合竞价</span><b className="average">均价:{visiblePoints.length ? (visiblePoints.reduce((sum, point) => sum + point.value, 0) / visiblePoints.length).toFixed(2) : yuan(market.last_close)}</b>
          <span>最新:{yuan(market.last_price)}</span>
        </div>
        <div className="msd-intraday-chart">
          <span className="msd-scale msd-scale-top">{top.toFixed(2)}</span>
          <span className="msd-scale msd-scale-mid">0.00%</span>
          <span className="msd-scale msd-scale-bottom">{bottom.toFixed(2)}</span>
          <svg viewBox="0 0 100 100" preserveAspectRatio="none" aria-label={`日内分时线已完成 ${Math.round(progress * 100)}%`}>
            <polyline className="msd-average-line" points={averagePoints} />
            <polyline className="msd-price-line" points={linePoints} />
          </svg>
          <div className="msd-time-axis" aria-hidden="true"><span>09:30</span><span>11:30</span><span>13:00</span><span>15:00</span></div>
        </div>
      </div>
      <aside className="msd-order-book" aria-label="五档盘口">
        <div className="msd-book-head"><b className={tone(market.last_price - market.last_close)}>大单</b><span>{yuan(market.last_price)}</span></div>
        {asks.map(([price, qty], index) => <div className="msd-book-row" key={`ask-${price}-${index}`}><span>卖{5 - index}</span><b className={tone(price - market.last_close)}>{yuan(price)}</b><span>{qty}</span></div>)}
        <div className="msd-book-divider" />
        {bids.map(([price, qty], index) => <div className="msd-book-row" key={`bid-${price}-${index}`}><span>买{index + 1}</span><b className={tone(price - market.last_close)}>{yuan(price)}</b><span>{qty}</span></div>)}
      </aside>
      <div className="msd-minute-volume">
        <div className="msd-volume-meta"><b>分时量⌄</b><span>量:{visiblePoints.at(-1)?.volume ?? 0}</span><small>每分钟一根 · {formatTradingMinute(Math.max(0, elapsedMinutes - 1))}</small></div>
        <div className="msd-minute-bars" aria-label="240 根一分钟成交量柱">
          {Array.from({ length: totalMinutes }, (_, index) => {
            const point = visiblePoints[index];
            const height = point ? Math.max(1, ((point.volume ?? 0) / maxVolume) * 100) : 0;
            return <i key={index} className={point?.buy ? "rise" : "fall"} style={{ height: `${height}%` }} />;
          })}
        </div>
      </div>
      <div className="msd-ticks" aria-label="逐笔成交">
        <div className="msd-ticks-head">明细⌃</div>
        {recentTrades.length === 0 ? <p>等待成交…</p> : recentTrades.map((trade) => (
          <div className="msd-tick-row" key={trade.seq}><span>{formatTradingMinute(Math.max(0, elapsedMinutes - 1))}</span><b className={tone(trade.price - market.last_close)}>{yuan(trade.price)}</b><span>{Math.round(trade.qty / 100)}</span></div>
        ))}
      </div>
    </section>
  );
}

function FundsPanel({ market, trades }: Pick<Props, "market" | "trades">) {
  const amounts = useMemo(() => {
    let inflow = 0;
    let outflow = 0;
    for (const trade of trades) {
      const amount = (trade.price * trade.qty) / 10_000_000_000;
      if (trade.price >= market.last_close) inflow += amount;
      else outflow += amount;
    }
    return { inflow, outflow, net: inflow - outflow };
  }, [market.last_close, trades]);
  const bars = [amounts.net * 0.5, amounts.net, -amounts.net * 0.36, -amounts.net * 0.62];
  const max = Math.max(0.01, ...bars.map(Math.abs));
  return (
    <section className="msd-funds" aria-labelledby="fund-flow-title">
      <div className="msd-fund-title"><h2 id="fund-flow-title">大单流向(亿元)</h2><span>查看资金动向 ›</span></div>
      <div className="msd-fund-grid">
        <div className="msd-fund-summary">
          <div><span>大单流入</span><b className="rise">{amounts.inflow.toFixed(2)}</b></div>
          <div><span>大单流出</span><b className="fall">{amounts.outflow.toFixed(2)}</b></div>
          <div><span>大单净流入</span><b className={tone(amounts.net)}>{amounts.net.toFixed(2)}</b></div>
        </div>
        <div className="msd-fund-bars" aria-label="特大单、大单、中单、小单资金流向">
          {bars.map((value, index) => <div className="msd-fund-bar" key={index}><i className={value >= 0 ? "rise" : "fall"} style={{ height: `${Math.max(8, Math.abs(value) / max * 52)}%`, [value >= 0 ? "bottom" : "top"]: "50%" }} /><b className={value >= 0 ? "rise" : "fall"}>{value.toFixed(2)}</b><span>{["特大单", "大单", "中单", "小单"][index]}</span></div>)}
        </div>
      </div>
    </section>
  );
}

export function MobileStockDetail(props: Props) {
  const [period, setPeriod] = useState<ChartPeriod>("分时");
  const [infoTab, setInfoTab] = useState<InfoTab>("资金");
  const { market, minutePoints } = props;
  const diff = market.last_price - market.last_close;
  const percent = market.last_close === 0 ? 0 : diff / market.last_close * 100;
  const prices = minutePoints.map((point) => point.value * 100);
  const open = prices[0] ?? market.last_close;
  const high = Math.max(market.last_price, ...prices);
  const low = Math.min(market.last_price, ...prices);
  const chartType = period === "分时" || period === "五日" ? "分时" : "日K";

  return (
    <main className="mobile-stock-detail">
      <header className="msd-header">
        <button type="button" aria-label="返回自选列表" onClick={props.onBack}>‹</button>
        <div><strong>{props.name}</strong><small>{props.code}　L1　沪股通</small></div>
        <span aria-hidden="true">⌕</span>
      </header>
      <section className="msd-quote" aria-label="股票报价摘要">
        <div className={`msd-last ${tone(diff)}`}><strong>{yuan(market.last_price)}</strong><span>{diff >= 0 ? "+" : ""}{yuan(diff)}　{percent >= 0 ? "+" : ""}{percent.toFixed(2)}%</span></div>
        <div className="msd-day-prices"><span>高 <b className={tone(high - market.last_close)}>{yuan(high)}</b></span><span>低 <b className={tone(low - market.last_close)}>{yuan(low)}</b></span><span>开 <b className={tone(open - market.last_close)}>{yuan(open)}</b></span></div>
        <div className="msd-stock-stats"><span>市值 <b>{largeMoney(market.fundamental_value * 16_000_000)}</b></span><span>流通 <b>{largeMoney(market.fundamental_value * 12_000_000)}</b></span><span>换手 <b>{Math.min(99, props.trades.reduce((sum, trade) => sum + trade.qty, 0) / 100_000).toFixed(2)}%</b></span><span>买一 <b className="rise">{market.best_bid ? yuan(market.best_bid) : "--"}</b></span><span>卖一 <b className="fall">{market.best_ask ? yuan(market.best_ask) : "--"}</b></span></div>
      </section>
      <div className="msd-after-hours"><b>盘中交易</b><span className={tone(diff)}>{yuan(market.last_price)}</span><span>量 {props.trades.reduce((sum, trade) => sum + trade.qty, 0)}</span><span className="rise">买一 {market.best_bid ? yuan(market.best_bid) : "--"}</span><span className="fall">卖一 {market.best_ask ? yuan(market.best_ask) : "--"}</span></div>
      <div className="msd-period-tabs" role="tablist" aria-label="图表周期">
        {(["分时", "日K", "周K", "月K", "五日", "更多"] as ChartPeriod[]).map((item) => <button type="button" role="tab" aria-selected={period === item} key={item} onClick={() => setPeriod(item)}>{item}{item === "更多" ? "⌄" : ""}</button>)}
      </div>
      {chartType === "分时" ? <IntradayPanel {...props} /> : (
        <section className="msd-kline" aria-label={`${period}图`}>
          <div className="msd-kline-meta"><b>{period}</b><span>M5</span><span>M10</span><span>M20</span><span>M60</span></div>
          <PriceChart data={props.minutePoints} dailyCandles={props.dailyCandles} lastClose={market.last_close / 100} chartType="日K" klineDays={props.klineDays} />
          <div className="msd-kline-days">{[20, 60, 120, 240, 360].map((days) => <button type="button" className={props.klineDays === days ? "active" : ""} key={days} onClick={() => props.onKlineDaysChange(days)}>{days}日</button>)}</div>
        </section>
      )}
      <div className="msd-info-tabs" role="tablist" aria-label="股票详情信息">
        {(["看点", "资讯", "盘口", "资金", "社区", "简况"] as InfoTab[]).map((item) => <button type="button" role="tab" aria-selected={infoTab === item} key={item} onClick={() => setInfoTab(item)}>{item}</button>)}
      </div>
      {infoTab === "资金" ? <FundsPanel market={market} trades={props.trades} /> : <section className="msd-placeholder"><b>{infoTab}</b><p>该内容区独立于上方图表周期，切换分时或日 K 时保持不变。</p></section>}
    </main>
  );
}
