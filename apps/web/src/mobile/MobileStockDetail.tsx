import { compareMoney, subtractMoney } from "../utils/money.ts";
import { useMemo, useState, type CSSProperties, type KeyboardEvent, type ReactNode } from "react";
import type { KlinePoint, PricePoint } from "../components/PriceChart";
import type { IndicatorCalculator } from "../components/indicator-results.ts";
import { useIndicatorResults } from "../components/useIndicatorResults.ts";
import type { MarketSnap, TradeEvent } from "../types/engine";
import { MobileSpeedSelect } from "./MobileSpeedSelect";
import { MobileGameClock } from "./MobileGameClock";
import { MobileRunToggle } from "./MobileRunToggle";
import { KlineViewportControls } from "./KlineViewportControls.tsx";
import { aggregateCandles, buildFiveLevelBook, formatTradeLots, formatTradeTime, intradayChartX, MOBILE_KLINE_DEFAULT_CAPACITY, MobileIntradayProjection, MobileKlineProjection, orderBookDepthPercent, priceChangePercent, reduceKlineViewport, type AuctionPoint, type KlineViewportAction } from "./market-model";
import type { MobileChartPeriod, MobileInfoTab } from "./mobile-ui-state";
import { formatDecimalCentsAsYuan, yuan } from "../utils/format";
import "./MobileStockDetail.css";

const chartPeriods: MobileChartPeriod[] = ["分时", "日K", "周K", "月K", "五日"];
const enabledChartPeriods: MobileChartPeriod[] = ["分时", "日K", "周K", "月K"];
const infoTabs: MobileInfoTab[] = ["看点", "资讯", "财务", "盘口", "资金", "社区", "简况"];

interface Props {
  code: string;
  name: string;
  market: MarketSnap;
  minutePoints: readonly PricePoint[];
  auctionPoints: readonly AuctionPoint[];
  dailyCandles: readonly KlinePoint[];
  activeDailyCandle?: KlinePoint;
  indicatorCalculator: IndicatorCalculator | null;
  trades: readonly TradeEvent[];
  elapsedMinutes: number;
  totalMinutes: number;
  klineDays: number;
  period: MobileChartPeriod;
  infoTab: MobileInfoTab;
  speed: number;
  measuredSpeed?: string;
  measuredSpeedTitle?: string;
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
  companyContent: ReactNode;
}

function tone(diff: number): "rise" | "fall" | "flat" {
  return diff > 0 ? "rise" : diff < 0 ? "fall" : "flat";
}

function FiveLevelBook({ market }: { market: MarketSnap }) {
  const book = buildFiveLevelBook(market.bids, market.asks);
  const sellMaximum = Math.max(1, ...book.sells.flatMap(({ level }) => level ? [level[1]] : []));
  const buyMaximum = Math.max(1, ...book.buys.flatMap(({ level }) => level ? [level[1]] : []));
  return <aside className="msd-order-book" aria-label="五档盘口，数量单位为手">
    <div className="msd-book-head"><b className={tone(compareMoney(market.last_price, market.last_close))}>大单 <small>量/手</small></b><span>{yuan(market.last_price)}</span></div>
    {book.sells.map(({ label, level }) => <div className="msd-book-row" key={label}><span>{label}</span><b className={level ? tone(compareMoney(level[0], market.last_close)) : "flat"}>{level ? yuan(level[0]) : "--"}</b><span className="msd-book-depth sell" style={{ "--depth": `${level ? orderBookDepthPercent(level[1], sellMaximum) : 0}%` } as CSSProperties}><span>{level ? formatTradeLots(level[1]) : "--"}</span></span></div>)}
    <div className="msd-book-divider" />
    {book.buys.map(({ label, level }) => <div className="msd-book-row" key={label}><span>{label}</span><b className={level ? tone(compareMoney(level[0], market.last_close)) : "flat"}>{level ? yuan(level[0]) : "--"}</b><span className="msd-book-depth buy" style={{ "--depth": `${level ? orderBookDepthPercent(level[1], buyMaximum) : 0}%` } as CSSProperties}><span>{level ? formatTradeLots(level[1]) : "--"}</span></span></div>)}
  </aside>;
}

function KlinePanel({ dailyCandles, period, indicatorCalculator }: Pick<Props, "dailyCandles" | "period" | "indicatorCalculator">) {
  const candlePeriod = period === "周K" || period === "月K" ? period : "日K";
  const allCandles = useMemo(() => aggregateCandles(dailyCandles, candlePeriod), [candlePeriod, dailyCandles]);
  const indicatorInput = useMemo(() => ({
    prices: [],
    candles: allCandles.map(({ high, low, close }) => ({ high, low, close })),
  }), [allCandles]);
  const indicatorResult = useIndicatorResults(indicatorCalculator, indicatorInput, allCandles.length > 0);
  const [viewport, setViewport] = useState({ capacity: MOBILE_KLINE_DEFAULT_CAPACITY, offsetFromEnd: 0 });
  if (allCandles.length === 0) return <section className="msd-kline msd-chart-empty" aria-label={`${period}图`}><b>{period}</b><p>等待游戏生成首个交易日 K 线…</p></section>;
  const projection = MobileKlineProjection.fromInputs(allCandles, viewport, indicatorResult);
  const candles = projection.visibleCandles;
  const ma5 = projection.movingAverage(5); const ma10 = projection.movingAverage(10); const ma20 = projection.movingAverage(20);
  const kdj = projection.kdj;
  const volumes = projection.volumes;
  const act = (action: KlineViewportAction) => setViewport((current) => reduceKlineViewport(current, allCandles.length, action));
  return <section
    className="msd-kline"
    aria-label={`${period}图`}
    data-kline-count={allCandles.length}
    data-kline-signature={projection.latestSignature}
  >
    <div className="msd-kline-meta"><button type="button" title="均线设置尚未开放" disabled>均线⌄</button><b>{period}</b><span>M5:{ma5.at(-1)?.toFixed(2)}</span><span>M10:{ma10.at(-1)?.toFixed(2)}</span><span>M20:{ma20.at(-1)?.toFixed(2)}</span></div>
    <svg className="msd-candle-chart" viewBox="0 0 390 190" preserveAspectRatio="none">{candles.map((c, index) => { const { slot, rise, body, wick } = projection.candleBodyAndWick(index); return <g key={`${c.time}-${index}`} className={rise?"rise":"fall"}><line className="upper-wick" x1={slot.center} x2={slot.center} y1={wick.upper.start} y2={wick.upper.end}/><rect x={slot.center-slot.markWidth/2} y={body.top} width={slot.markWidth} height={Math.max(1,body.bottom-body.top)}/><line className="lower-wick" x1={slot.center} x2={slot.center} y1={wick.lower.start} y2={wick.lower.end}/></g>; })}<polyline className="ma5" points={projection.movingAverageLine(5)}/><polyline className="ma10" points={projection.movingAverageLine(10)}/><polyline className="ma20" points={projection.movingAverageLine(20)}/></svg>
    <KlineViewportControls total={allCandles.length} viewport={viewport} onAction={act} />
    <div className="msd-volume-title">成交量（手）　<span>量:{formatTradeLots(volumes.at(-1) ?? 0)}手</span></div><svg className="msd-k-volume" viewBox="0 0 390 75" preserveAspectRatio="none" aria-label={`${period}成交量，单位为手`}>{projection.volumeMarks().map(({ slot, height, rise }, index) => <rect key={index} className={rise?"rise":"fall"} x={slot.center-slot.markWidth/2} y={75-height} width={slot.markWidth} height={height}/>)}</svg>
    {kdj === null ? <div className="msd-kdj-title" role={indicatorResult.kind === "error" || indicatorResult.kind === "unavailable" ? "alert" : "status"}>{indicatorResult.kind === "pending" ? "Rust 指标计算中…" : indicatorResult.kind === "error" ? `Rust 指标计算失败：${indicatorResult.message}` : indicatorResult.kind === "unavailable" ? "Rust 指标宿主尚未就绪" : "等待 Rust 指标…"}</div> : <><div className="msd-kdj-title">KDJ(9,3,3)　 K:{kdj.k.at(-1)?.toFixed(2)}　<span>D:{kdj.d.at(-1)?.toFixed(2)}</span>　<em>J:{kdj.j.at(-1)?.toFixed(2)}</em></div><svg className="msd-kdj" viewBox="0 0 390 72" preserveAspectRatio="none"><polyline points={projection.indicatorLine(kdj.k)}/><polyline className="orange" points={projection.indicatorLine(kdj.d)}/><polyline className="pink" points={projection.indicatorLine(kdj.j)}/></svg></>}
  </section>;
}

function IntradayPanel({ market, minutePoints, auctionPoints, trades, activeDailyCandle, elapsedMinutes, totalMinutes, gameDay, gameTick }: Pick<Props, "market" | "minutePoints" | "auctionPoints" | "trades" | "activeDailyCandle" | "elapsedMinutes" | "totalMinutes" | "gameDay" | "gameTick">) {
  const projection = MobileIntradayProjection.fromInputs({ market, minutePoints, auctionPoints, trades, activeDailyCandle, elapsedMinutes, totalMinutes, gameDay, gameTick });
  const { visiblePoints, visibleAuctionPoints, scale, displayedAverage, progress, recentTrades } = projection;
  const latestPoint = visiblePoints.at(-1);
  const volumeMarks = projection.volumeMarks();
  const auctionSegments = projection.auctionSegments();

  return (
    <section
      className="msd-market-composite"
      aria-label="分时、盘口、分时量和逐笔成交"
      data-intraday-count={visiblePoints.length}
      data-intraday-latest-minute={latestPoint?.time ?? -1}
      data-auction-count={visibleAuctionPoints.length}
      data-intraday-signature={projection.signature}
    >
      <div className="msd-intraday-main">
        <div className="msd-chart-meta">
          <span>集合竞价</span><b className="average">均价:{displayedAverage === null ? projection.averageUnavailableReason : displayedAverage.toFixed(2)}</b>
          <span>最新:{yuan(market.last_price)}</span>
        </div>
        <div className="msd-intraday-chart">
          <div className="msd-price-plot">
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
            {auctionSegments.map((segment, index) => <g key={index}>
              <polyline className="msd-auction-line" points={segment.map(point => `${intradayChartX({ phase: "auction", minute: point.time })},${projection.priceY(point.value)}`).join(" ")} />
              {segment.length === 1 && <circle className="msd-auction-dot" cx={intradayChartX({ phase: "auction", minute: segment[0].time })} cy={projection.priceY(segment[0].value)} r="0.8" />}
            </g>)}
            <polyline className="msd-average-line" points={projection.averageLine()} />
            <polyline className="msd-price-line" points={projection.continuousLine()} />
          </svg>
          </div>
          <div className="msd-time-axis" aria-hidden="true"><span style={{ left: "0%" }}>09:15</span><span className="after-auction" style={{ left: "16%" }}>09:30</span><span style={{ left: "37%" }}>10:30</span><span className="lunch-turn" style={{ left: "58%" }}>11:30/13:00</span><span style={{ left: "79%" }}>14:00</span><span className="market-close">15:00</span></div>
        </div>
      </div>
      <FiveLevelBook market={market} />
      <div className="msd-minute-volume">
        <div className="msd-volume-meta"><b>分时量（手）⌄</b><span>量:{formatTradeLots(volumeMarks.at(-1)?.volume ?? 0)}手</span><small>{projection.clockTime}</small></div>
        <svg
          className="msd-minute-bars"
          aria-label="集合竞价累计量细线及连续竞价一分钟成交量细线"
          data-auction-volume-line-count={visibleAuctionPoints.length}
        >
          {[16, 37, 58, 79].map((position) => <line key={position} className="msd-volume-guide" x1={`${position}%`} x2={`${position}%`} y1="0" y2="100%" />)}
          {volumeMarks.map((point, index) => <rect key={`${point.x}-${index}`} className={`msd-minute-volume-mark ${point.buy ? "rise" : "fall"}`} x={`${point.x}%`} y={`${100 - point.height}%`} width={0.5} height={`${point.height}%`} fill={point.buy ? "none" : "var(--msd-fall)"} stroke={point.buy ? "var(--msd-rise)" : "none"} strokeWidth={0.125} vectorEffect="non-scaling-stroke" />)}
        </svg>
      </div>
      <div className="msd-ticks" aria-label="逐笔成交">
        <div className="msd-ticks-head">明细⌃</div>
        {recentTrades.length === 0 ? <p>等待成交…</p> : recentTrades.map((trade) => (
          <div className="msd-tick-row" key={trade.seq}><span>{formatTradeTime(trade.tick)}</span><b className={tone(compareMoney(trade.price, market.last_close))}>{yuan(trade.price)}</b><span>{formatTradeLots(trade.qty)}</span></div>
        ))}
      </div>
    </section>
  );
}

function FundsPanel({ activeDailyCandle }: Pick<Props, "activeDailyCandle">) {
  const tradedShares = activeDailyCandle?.volume ?? 0;
  const statsUnavailable = tradedShares > 0 && activeDailyCandle?.tradeStats === undefined;
  return (
    <section className="msd-funds" aria-labelledby="fund-flow-title">
      <div className="msd-fund-title"><h2 id="fund-flow-title">当日累计成交</h2><span>来自引擎权威撮合统计</span></div>
      <div className="msd-fund-grid">
        <div className="msd-fund-summary">
          <div><span>成交额</span><b>{statsUnavailable ? "--" : `${formatDecimalCentsAsYuan(activeDailyCandle?.tradeStats?.turnoverCents ?? "0")}元`}</b></div>
          <div><span>成交量（手）</span><b>{formatTradeLots(tradedShares)}</b></div>
          <div><span>成交笔数</span><b>{statsUnavailable ? "--" : (activeDailyCandle?.tradeStats?.tradeCount ?? 0)}</b></div>
        </div>
        <div className="msd-fund-bars" aria-label={statsUnavailable ? "旧存档缺少当日成交额和成交笔数" : "资金方向暂无数据"}><p>{statsUnavailable ? "当前旧存档只有成交量，没有可对账的成交额和笔数；进入下一交易日后会恢复完整统计。" : "引擎暂未提供主动买卖方向，故不推算或伪造“大单流入/流出”。"}</p></div>
      </div>
    </section>
  );
}

export function MobileStockDetail(props: Props) {
  const { market } = props;
  const diff = subtractMoney(market.last_price, market.last_close);
  const percent = priceChangePercent(market.last_price, market.last_close);
  const rawPrices = props.activeDailyCandle?.rawPrices;
  if (props.activeDailyCandle !== undefined && rawPrices === undefined) throw new Error("当日 K 线缺少精确分值，不能展示报价摘要");
  const open = rawPrices?.open ?? market.last_close;
  const high = rawPrices?.high ?? market.last_price;
  const low = rawPrices?.low ?? market.last_price;
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
        <MobileSpeedSelect
          speed={props.speed}
          measuredSpeed={props.measuredSpeed}
          measuredSpeedTitle={props.measuredSpeedTitle}
          onChange={props.onSpeedChange}
        />
      </header>
      <section className="msd-quote" aria-label="股票报价摘要">
        <div className={`msd-last ${tone(compareMoney(diff, "0"))}`}><strong>{yuan(market.last_price)}</strong><span>{compareMoney(diff, "0") >= 0 ? "+" : ""}{yuan(diff)}　{percent >= 0 ? "+" : ""}{percent.toFixed(2)}%</span></div>
        <div className="msd-day-prices"><span>高 <b className={tone(compareMoney(high, market.last_close))}>{yuan(high)}</b></span><span>低 <b className={tone(compareMoney(low, market.last_close))}>{yuan(low)}</b></span><span>开 <b className={tone(compareMoney(open, market.last_close))}>{yuan(open)}</b></span></div>
        <div className="msd-stock-stats"><span>昨收 <b>{yuan(market.last_close)}</b></span><span>当日成交量 <b>{formatTradeLots(props.activeDailyCandle?.volume ?? 0)}手</b></span><span>买一 <b className="rise">{market.best_bid ? yuan(market.best_bid) : "--"}</b></span><span>卖一 <b className="fall">{market.best_ask ? yuan(market.best_ask) : "--"}</b></span></div>
      </section>
      <div className="msd-period-tabs" role="tablist" aria-label="图表周期">
        {chartPeriods.map((item) => {
          const disabled = item === "五日";
          return <button type="button" role="tab" id={`period-${item}`} aria-controls="mobile-chart-panel" aria-selected={props.period === item} aria-disabled={disabled} disabled={disabled} title={disabled ? "等待引擎提供跨日分钟数据" : undefined} tabIndex={props.period === item ? 0 : -1} key={item} onKeyDown={(event) => moveTabFocus(event, enabledChartPeriods)} onClick={() => props.onPeriodChange(item)}>{item}</button>;
        })}
        <button type="button" className="msd-more" aria-label="更多周期（即将开放）" title="更多周期（即将开放）" disabled>更多⌄</button>
      </div>
      <div id="mobile-chart-panel" role="tabpanel" aria-labelledby={`period-${props.period}`}>
      {chartType === "分时" ? <IntradayPanel {...props} /> : <KlinePanel key={`${props.code}-${props.period}`} dailyCandles={props.dailyCandles} period={props.period} indicatorCalculator={props.indicatorCalculator} />}
      </div>
      <div className="msd-info-tabs" role="tablist" aria-label="股票详情信息">
        {infoTabs.map((item) => <button type="button" role="tab" id={`info-${item}`} aria-controls="mobile-info-panel" aria-selected={props.infoTab === item} tabIndex={props.infoTab === item ? 0 : -1} key={item} onKeyDown={(event) => moveTabFocus(event, infoTabs)} onClick={() => props.onInfoTabChange(item)}>{item}</button>)}
      </div>
      <div id="mobile-info-panel" role="tabpanel" aria-labelledby={`info-${props.infoTab}`}>
        {props.infoTab === "资金" ? <FundsPanel activeDailyCandle={props.activeDailyCandle} /> : props.infoTab === "盘口" ? <section className="msd-info-book"><FiveLevelBook market={market} /></section> : props.infoTab === "财务" ? props.companyContent : <section className="msd-placeholder"><b>{props.infoTab}</b><p>该内容区独立于上方图表周期，切换分时或日 K 时保持不变。</p></section>}
      </div>
    </main>
  );
}
