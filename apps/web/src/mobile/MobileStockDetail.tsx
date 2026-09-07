import { useState, type KeyboardEvent } from "react";
import type { KlinePoint, PricePoint } from "../components/PriceChart";
import type { MarketSnap, TradeEvent } from "../types/engine";
import { MobileSpeedSelect } from "./MobileSpeedSelect";
import { MobileGameClock } from "./MobileGameClock";
import { MobileRunToggle } from "./MobileRunToggle";
import { aggregateCandles, buildFiveLevelBook, calculateKdj, candleBodyPrices, candleWickPrices, chartSlotGeometry, formatTradingMinute, klineWindow, MOBILE_KLINE_SLOT_CAPACITY, MOBILE_KLINE_ZOOM_LEVELS, priceChangePercent, reduceKlineViewport, tradingDayProgress, type KlineViewportAction } from "./market-model";
import type { MobileChartPeriod, MobileInfoTab } from "./mobile-ui-state";
import "./MobileStockDetail.css";

const chartPeriods: MobileChartPeriod[] = ["分时", "日K", "周K", "月K", "五日"];
const enabledChartPeriods: MobileChartPeriod[] = ["分时", "日K", "周K", "月K"];
const infoTabs: MobileInfoTab[] = ["看点", "资讯", "盘口", "资金", "社区", "简况"];

interface Props {
  code: string;
  name: string;
  market: MarketSnap;
  minutePoints: PricePoint[];
  dailyCandles: KlinePoint[];
  activeDailyCandle?: KlinePoint;
  trades: TradeEvent[];
  elapsedMinutes: number;
  totalMinutes: number;
  klineDays: number;
  period: MobileChartPeriod;
  infoTab: MobileInfoTab;
  speed: number;
  running: boolean;
  gameDay: number;
  gameTick: number;
  onKlineDaysChange: (days: number) => void;
  onPeriodChange: (period: MobileChartPeriod) => void;
  onInfoTabChange: (tab: MobileInfoTab) => void;
  onSpeedChange: (speed: number) => void;
  onPauseToggle: () => void;
  onBack: () => void;
  onPrevious: () => void;
  onNext: () => void;
}

function yuan(cents: number): string {
  return (cents / 100).toFixed(2);
}

function tone(diff: number): "rise" | "fall" | "flat" {
  return diff > 0 ? "rise" : diff < 0 ? "fall" : "flat";
}

function FiveLevelBook({ market }: { market: MarketSnap }) {
  const book = buildFiveLevelBook(market.bids, market.asks);
  return <aside className="msd-order-book" aria-label="五档盘口">
    <div className="msd-book-head"><b className={tone(market.last_price - market.last_close)}>大单</b><span>{yuan(market.last_price)}</span></div>
    {book.sells.map(({ label, level }) => <div className="msd-book-row" key={label}><span>{label}</span><b className={level ? tone(level[0] - market.last_close) : "flat"}>{level ? yuan(level[0]) : "--"}</b><span>{level ? level[1] : "--"}</span></div>)}
    <div className="msd-book-divider" />
    {book.buys.map(({ label, level }) => <div className="msd-book-row" key={label}><span>{label}</span><b className={level ? tone(level[0] - market.last_close) : "flat"}>{level ? yuan(level[0]) : "--"}</b><span>{level ? level[1] : "--"}</span></div>)}
  </aside>;
}

function KlinePanel({ dailyCandles, period }: Pick<Props, "dailyCandles" | "period">) {
  const candlePeriod = period === "周K" || period === "月K" ? period : "日K";
  const allCandles = aggregateCandles(dailyCandles, candlePeriod);
  const [viewport, setViewport] = useState({ capacity: MOBILE_KLINE_SLOT_CAPACITY, offsetFromEnd: 0 });
  if (allCandles.length === 0) return <section className="msd-kline msd-chart-empty" aria-label={`${period}图`}><b>{period}</b><p>等待游戏生成首个交易日 K 线…</p></section>;
  const window = klineWindow(allCandles.length, viewport.capacity, viewport.offsetFromEnd);
  const candles = allCandles.slice(window.start, window.end);
  const values = candles.flatMap((candle) => [candle.high, candle.low]);
  const max = Math.max(...values); const min = Math.min(...values); const range = Math.max(.01, max - min);
  const y = (value: number) => 8 + (max - value) / range * 166;
  const slotFor = (index: number) => chartSlotGeometry(index, candles.length, 390, viewport.capacity);
  const movingAverage = (days: number) => allCandles.map((_, index) => allCandles.slice(Math.max(0, index - days + 1), index + 1).reduce((sum, c) => sum + c.close, 0) / Math.min(days, index + 1)).slice(window.start, window.end);
  const ma5 = movingAverage(5); const ma10 = movingAverage(10); const ma20 = movingAverage(20);
  const line = (series: number[]) => series.map((value, index) => `${slotFor(index).center},${y(value)}`).join(" ");
  const completeKdj = calculateKdj(allCandles);
  const kdj = { k: completeKdj.k.slice(window.start, window.end), d: completeKdj.d.slice(window.start, window.end), j: completeKdj.j.slice(window.start, window.end) };
  const indicatorMin = Math.min(0, ...kdj.j); const indicatorMax = Math.max(100, ...kdj.j); const indicatorRange = Math.max(1, indicatorMax - indicatorMin);
  const indicatorLine = (series: number[]) => series.map((value, index) => `${slotFor(index).center},${68 - (value - indicatorMin) / indicatorRange * 64}`).join(" ");
  const volumes = candles.map(candle => candle.volume ?? 0); const maxVolume = Math.max(1, ...volumes);
  const act = (action: KlineViewportAction) => setViewport((current) => reduceKlineViewport(current, allCandles.length, action));
  const atSmallestZoom = viewport.capacity === MOBILE_KLINE_ZOOM_LEVELS.at(-1);
  const atLargestZoom = viewport.capacity === MOBILE_KLINE_ZOOM_LEVELS[0];
  return <section className="msd-kline" aria-label={`${period}图`}>
    <div className="msd-kline-meta"><button type="button" title="均线设置尚未开放" disabled>均线⌄</button><b>{period}</b><span>M5:{ma5.at(-1)?.toFixed(2)}</span><span>M10:{ma10.at(-1)?.toFixed(2)}</span><span>M20:{ma20.at(-1)?.toFixed(2)}</span></div>
    <svg className="msd-candle-chart" viewBox="0 0 390 190" preserveAspectRatio="none">{candles.map((c, index) => { const slot=slotFor(index); const rise=c.close>=c.open; const body=candleBodyPrices(c); const wick=candleWickPrices(c); const bodyTop=y(body.top); const bodyBottom=y(body.bottom); return <g key={`${c.time}-${index}`} className={rise?"rise":"fall"}><line className="upper-wick" x1={slot.center} x2={slot.center} y1={y(wick.upper.start)} y2={y(wick.upper.end)}/><rect x={slot.center-slot.markWidth/2} y={bodyTop} width={slot.markWidth} height={Math.max(1,bodyBottom-bodyTop)}/><line className="lower-wick" x1={slot.center} x2={slot.center} y1={y(wick.lower.start)} y2={y(wick.lower.end)}/></g>; })}<polyline className="ma5" points={line(ma5)}/><polyline className="ma10" points={line(ma10)}/><polyline className="ma20" points={line(ma20)}/></svg>
    <div className="msd-chart-tools" aria-label="K线窗口控制"><button type="button" aria-label="跳到最早历史" title="跳到最早历史" onClick={() => act("earliest")} disabled={window.offsetFromEnd >= window.maxOffset}>«</button><button type="button" aria-label="放大K线" title="放大K线" onClick={() => act("zoom-in")} disabled={atSmallestZoom}>＋</button><button type="button" aria-label="缩小K线" title="缩小K线" onClick={() => act("zoom-out")} disabled={atLargestZoom}>−</button><button type="button" aria-label="窗口左移" title="查看更早历史" onClick={() => act("pan-left")} disabled={window.offsetFromEnd >= window.maxOffset}>‹</button><button type="button" aria-label="窗口右移" title="查看更新历史" onClick={() => act("pan-right")} disabled={window.offsetFromEnd === 0}>›</button><button type="button" aria-label="复位K线窗口" title="回到最新并复位缩放" onClick={() => act("reset")} disabled={atLargestZoom && window.offsetFromEnd === 0}>⌗</button></div>
    <div className="msd-volume-title">成交量　<span>量:{volumes.at(-1) ?? 0}</span></div><svg className="msd-k-volume" viewBox="0 0 390 75" preserveAspectRatio="none" aria-label={`${period}成交量`}>{volumes.map((volume,index)=>{const slot=slotFor(index); const height=Math.max(1,volume/maxVolume*66); return <rect key={index} className={candles[index].close>=candles[index].open?"rise":"fall"} x={slot.center-slot.markWidth/2} y={75-height} width={slot.markWidth} height={height}/>;})}</svg>
    <div className="msd-kdj-title">KDJ(9,3,3)　 K:{kdj.k.at(-1)?.toFixed(2)}　<span>D:{kdj.d.at(-1)?.toFixed(2)}</span>　<em>J:{kdj.j.at(-1)?.toFixed(2)}</em></div><svg className="msd-kdj" viewBox="0 0 390 72" preserveAspectRatio="none"><polyline points={indicatorLine(kdj.k)}/><polyline className="orange" points={indicatorLine(kdj.d)}/><polyline className="pink" points={indicatorLine(kdj.j)}/></svg>
  </section>;
}

function IntradayPanel({ market, minutePoints, trades, elapsedMinutes, totalMinutes }: Pick<Props, "market" | "minutePoints" | "trades" | "elapsedMinutes" | "totalMinutes">) {
  const visiblePoints = minutePoints.slice(-totalMinutes);
  const pointsByMinute = new Map(visiblePoints.map((point) => [point.time, point]));
  const values = visiblePoints.map((point) => point.value);
  const lastClose = market.last_close / 100;
  const highest = Math.max(lastClose, ...values);
  const lowest = Math.min(lastClose, ...values);
  const distance = Math.max(highest - lastClose, lastClose - lowest, lastClose * 0.006, 0.01);
  const top = lastClose + distance * 1.12;
  const bottom = lastClose - distance * 1.12;
  const y = (value: number) => 8 + ((top - value) / (top - bottom)) * 84;
  const linePoints = visiblePoints
    .map((point) => `${(point.time / Math.max(totalMinutes - 1, 1)) * 100},${y(point.value)}`)
    .join(" ");
  const averagePoints = visiblePoints
    .map((point, index) => {
      const average = visiblePoints.slice(0, index + 1).reduce((sum, point) => sum + point.value, 0) / (index + 1);
      return `${(point.time / Math.max(totalMinutes - 1, 1)) * 100},${y(average)}`;
    })
    .join(" ");
  const progress = tradingDayProgress(elapsedMinutes, totalMinutes);
  const maxVolume = Math.max(1, ...visiblePoints.map((point) => point.volume ?? 0));
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
      <FiveLevelBook market={market} />
      <div className="msd-minute-volume">
        <div className="msd-volume-meta"><b>分时量⌄</b><span>量:{visiblePoints.at(-1)?.volume ?? 0}</span><small>每分钟一根 · {formatTradingMinute(Math.max(0, elapsedMinutes - 1))}</small></div>
        <div className="msd-minute-bars" aria-label="240 根一分钟成交量柱">
          {Array.from({ length: totalMinutes }, (_, index) => {
            const point = pointsByMinute.get(index);
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

function FundsPanel({ trades }: Pick<Props, "trades">) {
  const turnoverYuan = trades.reduce((sum, trade) => sum + trade.price * trade.qty / 100, 0);
  const tradedShares = trades.reduce((sum, trade) => sum + trade.qty, 0);
  return (
    <section className="msd-funds" aria-labelledby="fund-flow-title">
      <div className="msd-fund-title"><h2 id="fund-flow-title">实时成交统计</h2><span>来自游戏撮合数据</span></div>
      <div className="msd-fund-grid">
        <div className="msd-fund-summary">
          <div><span>成交额</span><b>{turnoverYuan.toFixed(2)} 元</b></div>
          <div><span>成交股数</span><b>{tradedShares}</b></div>
          <div><span>成交笔数</span><b>{trades.length}</b></div>
        </div>
        <div className="msd-fund-bars" aria-label="资金方向暂无数据"><p>引擎暂未提供主动买卖方向，故不推算或伪造“大单流入/流出”。</p></div>
      </div>
    </section>
  );
}

export function MobileStockDetail(props: Props) {
  const { market } = props;
  const diff = market.last_price - market.last_close;
  const percent = priceChangePercent(market.last_price, market.last_close);
  const open = props.activeDailyCandle ? props.activeDailyCandle.open * 100 : market.last_close;
  const high = props.activeDailyCandle ? props.activeDailyCandle.high * 100 : market.last_price;
  const low = props.activeDailyCandle ? props.activeDailyCandle.low * 100 : market.last_price;
  const chartType = props.period === "分时" ? "分时" : "日K";

  function moveTabFocus(event: KeyboardEvent<HTMLButtonElement>, items: readonly string[]) {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const buttons = Array.from(event.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>('[role="tab"]') ?? []);
    const current = buttons.indexOf(event.currentTarget);
    const direction = event.key === "ArrowRight" ? 1 : -1;
    buttons[(current + direction + items.length) % items.length]?.focus();
  }

  return (
    <main className="mobile-stock-detail">
      <header className="msd-header">
        <button type="button" className="msd-back" aria-label="返回自选列表" onClick={props.onBack}>‹</button>
        <MobileGameClock day={props.gameDay} tick={props.gameTick} variant="detail" />
        <button type="button" className="msd-stock-switch msd-previous" aria-label="上一只" onClick={props.onPrevious}>◀</button>
        <div className="msd-security-title"><strong>{props.name}</strong><small>{props.code}</small></div>
        <button type="button" className="msd-stock-switch msd-next" aria-label="下一只" onClick={props.onNext}>▶</button>
        <MobileRunToggle running={props.running} onToggle={props.onPauseToggle} variant="detail" />
        <MobileSpeedSelect speed={props.speed} onChange={props.onSpeedChange} />
      </header>
      <section className="msd-quote" aria-label="股票报价摘要">
        <div className={`msd-last ${tone(diff)}`}><strong>{yuan(market.last_price)}</strong><span>{diff >= 0 ? "+" : ""}{yuan(diff)}　{percent >= 0 ? "+" : ""}{percent.toFixed(2)}%</span></div>
        <div className="msd-day-prices"><span>高 <b className={tone(high - market.last_close)}>{yuan(high)}</b></span><span>低 <b className={tone(low - market.last_close)}>{yuan(low)}</b></span><span>开 <b className={tone(open - market.last_close)}>{yuan(open)}</b></span></div>
        <div className="msd-stock-stats"><span>昨收 <b>{yuan(market.last_close)}</b></span><span>估值 <b>{yuan(market.fundamental_value)}</b></span><span>成交量 <b>{props.trades.reduce((sum, trade) => sum + trade.qty, 0)}</b></span><span>买一 <b className="rise">{market.best_bid ? yuan(market.best_bid) : "--"}</b></span><span>卖一 <b className="fall">{market.best_ask ? yuan(market.best_ask) : "--"}</b></span></div>
      </section>
      <div className="msd-after-hours"><b>盘中交易</b><span className={tone(diff)}>{yuan(market.last_price)}</span><span>量 {props.trades.reduce((sum, trade) => sum + trade.qty, 0)}</span><span className="rise">买一 {market.best_bid ? yuan(market.best_bid) : "--"}</span><span className="fall">卖一 {market.best_ask ? yuan(market.best_ask) : "--"}</span></div>
      <div className="msd-period-tabs" role="tablist" aria-label="图表周期">
        {chartPeriods.map((item) => {
          const disabled = item === "五日";
          return <button type="button" role="tab" id={`period-${item}`} aria-controls="mobile-chart-panel" aria-selected={props.period === item} aria-disabled={disabled} disabled={disabled} title={disabled ? "等待引擎提供跨日分钟数据" : undefined} tabIndex={props.period === item ? 0 : -1} key={item} onKeyDown={(event) => moveTabFocus(event, enabledChartPeriods)} onClick={() => props.onPeriodChange(item)}>{item}</button>;
        })}
        <button type="button" className="msd-more" aria-label="更多周期（即将开放）" title="更多周期（即将开放）" disabled>更多⌄</button>
      </div>
      <div id="mobile-chart-panel" role="tabpanel" aria-labelledby={`period-${props.period}`}>
      {chartType === "分时" ? <IntradayPanel {...props} /> : <KlinePanel key={`${props.code}-${props.period}`} dailyCandles={props.dailyCandles} period={props.period} />}
      </div>
      <div className="msd-info-tabs" role="tablist" aria-label="股票详情信息">
        {infoTabs.map((item) => <button type="button" role="tab" id={`info-${item}`} aria-controls="mobile-info-panel" aria-selected={props.infoTab === item} tabIndex={props.infoTab === item ? 0 : -1} key={item} onKeyDown={(event) => moveTabFocus(event, infoTabs)} onClick={() => props.onInfoTabChange(item)}>{item}</button>)}
      </div>
      <div id="mobile-info-panel" role="tabpanel" aria-labelledby={`info-${props.infoTab}`}>
        {props.infoTab === "资金" ? <FundsPanel trades={props.trades} /> : props.infoTab === "盘口" ? <section className="msd-info-book"><FiveLevelBook market={market} /></section> : <section className="msd-placeholder"><b>{props.infoTab}</b><p>该内容区独立于上方图表周期，切换分时或日 K 时保持不变。</p></section>}
      </div>
    </main>
  );
}
