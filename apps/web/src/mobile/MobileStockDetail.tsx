import { useState, type CSSProperties, type KeyboardEvent } from "react";
import type { KlinePoint, PricePoint } from "../components/PriceChart";
import type { MarketSnap, TradeEvent } from "../types/engine";
import { MobileSpeedSelect } from "./MobileSpeedSelect";
import { MobileGameClock } from "./MobileGameClock";
import { MobileRunToggle } from "./MobileRunToggle";
import { aggregateCandles, AUCTION_VOLUME_LINES_PER_MINUTE, buildFiveLevelBook, calculateKdj, candleBodyPrices, candleWickPrices, chartSlotGeometry, formatGameClock, formatTradeLots, formatTradingMinute, intradayChartX, intradayVolumeScale, klineWindow, MOBILE_KLINE_SLOT_CAPACITY, MOBILE_KLINE_ZOOM_LEVELS, orderBookDepthPercent, priceChangePercent, reduceKlineViewport, symmetricIntradayScale, type AuctionPoint, type KlineViewportAction } from "./market-model";
import type { MobileChartPeriod, MobileInfoTab } from "./mobile-ui-state";
import { formatYuanAmount } from "../utils/format";
import "./MobileStockDetail.css";

const chartPeriods: MobileChartPeriod[] = ["分时", "日K", "周K", "月K", "五日"];
const enabledChartPeriods: MobileChartPeriod[] = ["分时", "日K", "周K", "月K"];
const infoTabs: MobileInfoTab[] = ["看点", "资讯", "盘口", "资金", "社区", "简况"];

interface Props {
  code: string;
  name: string;
  market: MarketSnap;
  minutePoints: PricePoint[];
  auctionPoints: AuctionPoint[];
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
  const sellMaximum = Math.max(1, ...book.sells.flatMap(({ level }) => level ? [level[1]] : []));
  const buyMaximum = Math.max(1, ...book.buys.flatMap(({ level }) => level ? [level[1]] : []));
  return <aside className="msd-order-book" aria-label="五档盘口，数量单位为手">
    <div className="msd-book-head"><b className={tone(market.last_price - market.last_close)}>大单 <small>量/手</small></b><span>{yuan(market.last_price)}</span></div>
    {book.sells.map(({ label, level }) => <div className="msd-book-row" key={label}><span>{label}</span><b className={level ? tone(level[0] - market.last_close) : "flat"}>{level ? yuan(level[0]) : "--"}</b><span className="msd-book-depth sell" style={{ "--depth": `${level ? orderBookDepthPercent(level[1], sellMaximum) : 0}%` } as CSSProperties}><span>{level ? formatTradeLots(level[1]) : "--"}</span></span></div>)}
    <div className="msd-book-divider" />
    {book.buys.map(({ label, level }) => <div className="msd-book-row" key={label}><span>{label}</span><b className={level ? tone(level[0] - market.last_close) : "flat"}>{level ? yuan(level[0]) : "--"}</b><span className="msd-book-depth buy" style={{ "--depth": `${level ? orderBookDepthPercent(level[1], buyMaximum) : 0}%` } as CSSProperties}><span>{level ? formatTradeLots(level[1]) : "--"}</span></span></div>)}
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
  const latestCandle = allCandles.at(-1);
  const klineSignature = latestCandle
    ? `${latestCandle.time}:${latestCandle.open}:${latestCandle.high}:${latestCandle.low}:${latestCandle.close}:${latestCandle.volume ?? 0}`
    : "empty";
  return <section
    className="msd-kline"
    aria-label={`${period}图`}
    data-kline-count={allCandles.length}
    data-kline-signature={klineSignature}
  >
    <div className="msd-kline-meta"><button type="button" title="均线设置尚未开放" disabled>均线⌄</button><b>{period}</b><span>M5:{ma5.at(-1)?.toFixed(2)}</span><span>M10:{ma10.at(-1)?.toFixed(2)}</span><span>M20:{ma20.at(-1)?.toFixed(2)}</span></div>
    <svg className="msd-candle-chart" viewBox="0 0 390 190" preserveAspectRatio="none">{candles.map((c, index) => { const slot=slotFor(index); const rise=c.close>=c.open; const body=candleBodyPrices(c); const wick=candleWickPrices(c); const bodyTop=y(body.top); const bodyBottom=y(body.bottom); return <g key={`${c.time}-${index}`} className={rise?"rise":"fall"}><line className="upper-wick" x1={slot.center} x2={slot.center} y1={y(wick.upper.start)} y2={y(wick.upper.end)}/><rect x={slot.center-slot.markWidth/2} y={bodyTop} width={slot.markWidth} height={Math.max(1,bodyBottom-bodyTop)}/><line className="lower-wick" x1={slot.center} x2={slot.center} y1={y(wick.lower.start)} y2={y(wick.lower.end)}/></g>; })}<polyline className="ma5" points={line(ma5)}/><polyline className="ma10" points={line(ma10)}/><polyline className="ma20" points={line(ma20)}/></svg>
    <div className="msd-chart-tools" aria-label="K线窗口控制"><button type="button" aria-label="跳到最早历史" title="跳到最早历史" onClick={() => act("earliest")} disabled={window.offsetFromEnd >= window.maxOffset}>«</button><button type="button" aria-label="放大K线" title="放大K线" onClick={() => act("zoom-in")} disabled={atSmallestZoom}>＋</button><button type="button" aria-label="缩小K线" title="缩小K线" onClick={() => act("zoom-out")} disabled={atLargestZoom}>−</button><button type="button" aria-label="窗口左移" title="查看更早历史" onClick={() => act("pan-left")} disabled={window.offsetFromEnd >= window.maxOffset}>‹</button><button type="button" aria-label="窗口右移" title="查看更新历史" onClick={() => act("pan-right")} disabled={window.offsetFromEnd === 0}>›</button><button type="button" aria-label="复位K线窗口" title="回到最新并复位缩放" onClick={() => act("reset")} disabled={atLargestZoom && window.offsetFromEnd === 0}>⌗</button></div>
    <div className="msd-volume-title">成交量（手）　<span>量:{formatTradeLots(volumes.at(-1) ?? 0)}手</span></div><svg className="msd-k-volume" viewBox="0 0 390 75" preserveAspectRatio="none" aria-label={`${period}成交量，单位为手`}>{volumes.map((volume,index)=>{const slot=slotFor(index); const height=Math.max(1,volume/maxVolume*66); return <rect key={index} className={candles[index].close>=candles[index].open?"rise":"fall"} x={slot.center-slot.markWidth/2} y={75-height} width={slot.markWidth} height={height}/>;})}</svg>
    <div className="msd-kdj-title">KDJ(9,3,3)　 K:{kdj.k.at(-1)?.toFixed(2)}　<span>D:{kdj.d.at(-1)?.toFixed(2)}</span>　<em>J:{kdj.j.at(-1)?.toFixed(2)}</em></div><svg className="msd-kdj" viewBox="0 0 390 72" preserveAspectRatio="none"><polyline points={indicatorLine(kdj.k)}/><polyline className="orange" points={indicatorLine(kdj.d)}/><polyline className="pink" points={indicatorLine(kdj.j)}/></svg>
  </section>;
}

function IntradayPanel({ market, minutePoints, auctionPoints, trades, elapsedMinutes, totalMinutes, gameDay, gameTick }: Pick<Props, "market" | "minutePoints" | "auctionPoints" | "trades" | "elapsedMinutes" | "totalMinutes" | "gameDay" | "gameTick">) {
  const visiblePoints = minutePoints.slice(-totalMinutes);
  const visibleAuctionPoints = auctionPoints.slice(-15 * AUCTION_VOLUME_LINES_PER_MINUTE);
  const visibleAuctionPricePoints = visibleAuctionPoints.filter(
    (point): point is AuctionPoint & { value: number } => point.value !== null,
  );
  const lastClose = market.last_close / 100;
  const scale = symmetricIntradayScale([...visibleAuctionPricePoints, ...visiblePoints].map((point) => point.value), lastClose);
  const y = (value: number) => 8 + ((scale.top - value) / (scale.top - scale.bottom)) * 84;
  const auctionLinePoints = visibleAuctionPricePoints
    .map((point) => `${intradayChartX({ phase: "auction", minute: point.time })},${y(point.value)}`)
    .join(" ");
  const linePoints = visiblePoints
    .map((point) => `${intradayChartX({ phase: "continuous", minute: point.time })},${y(point.value)}`)
    .join(" ");
  const averagePoints = visiblePoints
    .map((point, index) => {
      const average = visiblePoints.slice(0, index + 1).reduce((sum, point) => sum + point.value, 0) / (index + 1);
      return `${intradayChartX({ phase: "continuous", minute: point.time })},${y(average)}`;
    })
    .join(" ");
  const progress = Math.min(1, (visibleAuctionPoints.length + elapsedMinutes) / (15 + totalMinutes));
  const averageSource = visiblePoints.length > 0 ? visiblePoints : visibleAuctionPricePoints;
  const displayedAverage = averageSource.length > 0
    ? averageSource.reduce((sum, point) => sum + point.value, 0) / averageSource.length
    : lastClose;
  const allVolumePoints = [
    ...visibleAuctionPoints.map((point) => ({ ...point, phase: "auction" as const, x: intradayChartX({ phase: "auction", minute: point.time }) })),
    ...visiblePoints.map((point) => ({ ...point, phase: "continuous" as const, x: intradayChartX({ phase: "continuous", minute: point.time }) })),
  ];
  const volumeScale = intradayVolumeScale(
    visibleAuctionPoints.map((point) => point.volume ?? 0),
    visiblePoints.map((point) => point.volume ?? 0),
  );
  const recentTrades = trades.slice(-7).reverse();
  const latestPoint = visiblePoints.at(-1);
  const latestAuctionPoint = visibleAuctionPoints.at(-1);
  const intradaySignature = latestPoint
    ? `${gameDay}:continuous:${latestPoint.time}:${latestPoint.value}:${latestPoint.volume ?? 0}`
    : latestAuctionPoint
      ? `${gameDay}:auction:${latestAuctionPoint.time}:${latestAuctionPoint.value}:${latestAuctionPoint.volume ?? 0}`
      : `${gameDay}:empty`;

  return (
    <section
      className="msd-market-composite"
      aria-label="分时、盘口、分时量和逐笔成交"
      data-intraday-count={visiblePoints.length}
      data-intraday-latest-minute={latestPoint?.time ?? -1}
      data-auction-count={visibleAuctionPoints.length}
      data-intraday-signature={intradaySignature}
    >
      <div className="msd-intraday-main">
        <div className="msd-chart-meta">
          <span>集合竞价</span><b className="average">均价:{displayedAverage.toFixed(2)}</b>
          <span>最新:{yuan(market.last_price)}</span>
        </div>
        <div className="msd-intraday-chart">
          <span className="msd-scale msd-scale-top-price">{scale.top.toFixed(2)}</span>
          <span className="msd-scale msd-scale-top-percent">+{scale.topPercent.toFixed(2)}%</span>
          <span className="msd-scale msd-scale-mid">0.00%</span>
          <span className="msd-scale msd-scale-bottom-price">{scale.bottom.toFixed(2)}</span>
          <span className="msd-scale msd-scale-bottom-percent">{scale.bottomPercent.toFixed(2)}%</span>
          <svg viewBox="0 0 100 100" preserveAspectRatio="none" aria-label={`日内分时线已完成 ${Math.round(progress * 100)}%`}>
            <rect className="msd-auction-band" x="0" y="0" width="16" height="100" />
            <line className="msd-session-line" x1="16" x2="16" y1="0" y2="100" />
            <line className="msd-session-line" x1="37" x2="37" y1="0" y2="100" />
            <line className="msd-session-line" x1="58" x2="58" y1="0" y2="100" />
            <line className="msd-session-line" x1="79" x2="79" y1="0" y2="100" />
            <polyline className="msd-auction-line" points={auctionLinePoints} />
            {visibleAuctionPricePoints.length === 1 && <circle className="msd-auction-dot" cx={intradayChartX({ phase: "auction", minute: visibleAuctionPricePoints[0].time })} cy={y(visibleAuctionPricePoints[0].value)} r="0.8" />}
            <polyline className="msd-average-line" points={averagePoints} />
            <polyline className="msd-price-line" points={linePoints} />
          </svg>
          <div className="msd-time-axis" aria-hidden="true"><span style={{ left: "0%" }}>09:15</span><span className="after-auction" style={{ left: "16%" }}>09:30</span><span style={{ left: "37%" }}>10:30</span><span className="lunch-turn" style={{ left: "58%" }}>11:30/13:00</span><span style={{ left: "79%" }}>14:00</span><span className="market-close">15:00</span></div>
        </div>
      </div>
      <FiveLevelBook market={market} />
      <div className="msd-minute-volume">
        <div className="msd-volume-meta"><b>分时量（手）⌄</b><span>量:{formatTradeLots(allVolumePoints.at(-1)?.volume ?? 0)}手</span><small>{formatGameClock(gameTick).slice(0, 5)}</small></div>
        <div
          className="msd-minute-bars"
          aria-label="集合竞价累计量细线及连续竞价一分钟成交量细线"
          data-auction-volume-line-count={visibleAuctionPoints.length}
        >
          <span className="msd-volume-guide" style={{ left: "16%" }} />
          <span className="msd-volume-guide" style={{ left: "37%" }} />
          <span className="msd-volume-guide" style={{ left: "58%" }} />
          <span className="msd-volume-guide" style={{ left: "79%" }} />
          {allVolumePoints.map((point, index) => {
            const phaseMaximum = point.phase === "auction" ? volumeScale.auctionMax : volumeScale.continuousMax;
            const height = Math.max(1, ((point.volume ?? 0) / phaseMaximum) * 100);
            return <i key={`${point.x}-${index}`} className={point.buy ? "rise" : "fall"} style={{ left: `${point.x}%`, height: `${height}%` }} />;
          })}
        </div>
      </div>
      <div className="msd-ticks" aria-label="逐笔成交">
        <div className="msd-ticks-head">明细⌃</div>
        {recentTrades.length === 0 ? <p>等待成交…</p> : recentTrades.map((trade) => (
          <div className="msd-tick-row" key={trade.seq}><span>{formatTradingMinute(Math.max(0, elapsedMinutes - 1))}</span><b className={tone(trade.price - market.last_close)}>{yuan(trade.price)}</b><span>{formatTradeLots(trade.qty)}</span></div>
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
          <div><span>成交额</span><b>{formatYuanAmount(turnoverYuan)}元</b></div>
          <div><span>成交量（手）</span><b>{formatTradeLots(tradedShares)}</b></div>
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
        <div className="msd-stock-stats"><span>昨收 <b>{yuan(market.last_close)}</b></span><span>成交量 <b>{formatTradeLots(props.trades.reduce((sum, trade) => sum + trade.qty, 0))}手</b></span><span>买一 <b className="rise">{market.best_bid ? yuan(market.best_bid) : "--"}</b></span><span>卖一 <b className="fall">{market.best_ask ? yuan(market.best_ask) : "--"}</b></span></div>
      </section>
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
