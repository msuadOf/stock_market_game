import { moneyToChartNumber } from "../utils/money.ts";
import { auctionContinuousJoin, auctionDisplayPoints } from "../components/intraday-auction-display.ts";
import { compareMoney, subtractMoney } from "../utils/money.ts";
import { type KeyboardEvent, type ReactNode } from "react";
import type { KlinePoint, PricePoint } from "../components/PriceChart";
import type { IndicatorCalculator } from "../components/indicator-results.ts";
import { MarketKlinePanel } from "../components/MarketKlinePanel.tsx";
import { ChartPeriodTabs } from "../components/ChartPeriodTabs.tsx";
import { FiveLevelBook } from "../components/FiveLevelBook.tsx";
import { useTradingTimeline } from "../components/TradingTimelineContext.tsx";
import { MarketTradeTape } from "../components/MarketTradeTape.tsx";
import { marketQuoteFacts } from "../components/market-quote-facts.ts";
import type { MarketSnap, TradeEvent } from "../types/engine";
import { MobileSpeedSelect } from "./MobileSpeedSelect";
import { MobileGameClock } from "./MobileGameClock";
import { MobileRunToggle } from "./MobileRunToggle";
import { formatTradeLots, intradayChartX, MobileIntradayProjection, priceChangePercent, type AuctionPoint } from "./market-model";
import { MOBILE_INFO_TABS, type MobileChartPeriod, type MobileInfoTab } from "./mobile-ui-state";
import { formatDecimalCentsAsYuan, yuan } from "../utils/format";
import "./MobileStockDetail.css";

const infoTabs = MOBILE_INFO_TABS;

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
  watchlistControl?: ReactNode;
  canSwitchStock?: boolean;
}

function tone(diff: number): "rise" | "fall" | "flat" {
  return diff > 0 ? "rise" : diff < 0 ? "fall" : "flat";
}


function IntradayPanel({ code, market, minutePoints, auctionPoints, trades, elapsedMinutes, totalMinutes, gameDay, gameTick }: Pick<Props, "code" | "market" | "minutePoints" | "auctionPoints" | "trades" | "elapsedMinutes" | "totalMinutes" | "gameDay" | "gameTick">) {
  const timeline = useTradingTimeline();
  const projection = MobileIntradayProjection.fromInputs({ timeline, market, minutePoints, auctionPoints, trades, elapsedMinutes, totalMinutes, gameDay, gameTick });
  const { visiblePoints, visibleAuctionPoints, scale, displayedAverage, progress } = projection;
  const latestPoint = visiblePoints.at(-1);
  const volumeMarks = projection.volumeMarks();
  const auction = auctionDisplayPoints(visibleAuctionPoints, moneyToChartNumber(market.last_close) / 100);
  const sessionJoin = auctionContinuousJoin(auction, visiblePoints);

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
          <span>集合竞价</span><b className="average">均价:{displayedAverage.toFixed(2)}</b>
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
            {auction.length > 0 && <polyline className="msd-auction-line" points={auction.map(point => `${intradayChartX({ phase: "auction", minute: point.time })},${projection.priceY(point.value)}`).join(" ")}><title>无指示价时沿昨收0%参考轴显示；粗点表示竞价指示更新，并非已成交</title></polyline>}
            {auction.filter(point => point.updated).map(point => <circle key={point.time} className="msd-auction-dot" cx={intradayChartX({ phase: "auction", minute: point.time })} cy={projection.priceY(point.value)} r="0.15" stroke="currentColor" strokeWidth="3" vectorEffect="non-scaling-stroke"><title>竞价指示价或可匹配量更新</title></circle>)}
            {sessionJoin.length > 0 && <polyline className="msd-session-join" points={sessionJoin.map(point => `${point.x},${projection.priceY(point.value)}`).join(" ")} />}
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
        <MarketTradeTape key={code} code={code} market={market} trades={trades} />
      </div>
    </section>
  );
}

function FundsPanel({ activeDailyCandle }: Pick<Props, "activeDailyCandle">) {
  const facts = marketQuoteFacts(activeDailyCandle);
  const { statsUnavailable } = facts;
  return (
    <section className="msd-funds" aria-labelledby="fund-flow-title">
      <div className="msd-fund-title"><h2 id="fund-flow-title">当日累计成交</h2><span>来自引擎权威撮合统计</span></div>
      <div className="msd-fund-grid">
        <div className="msd-fund-summary">
          <div><span>成交额</span><b>{facts.turnoverCents === null ? "--" : `${formatDecimalCentsAsYuan(facts.turnoverCents)}元`}</b></div>
          <div><span>成交量（手）</span><b>{formatTradeLots(facts.volume)}</b></div>
          <div><span>成交笔数</span><b>{facts.tradeCount === null ? "--" : facts.tradeCount}</b></div>
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
  const facts = marketQuoteFacts(props.activeDailyCandle);
  const { prices } = facts;
  const chartType = props.period === "分时" ? "分时" : "日K";

  function moveTabFocus(event: KeyboardEvent<HTMLButtonElement>, items: readonly string[]) {
    const buttons = Array.from(event.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>('[role="tab"]') ?? []);
    const current = buttons.indexOf(event.currentTarget);
    let next: number;
    switch (event.key) {
      case "ArrowLeft": next = (current + items.length - 1) % items.length; break;
      case "ArrowRight": next = (current + 1) % items.length; break;
      case "Home": next = 0; break;
      case "End": next = items.length - 1; break;
      default: return;
    }
    event.preventDefault();
    buttons[next]?.focus();
  }

  return (
    <main className="mobile-stock-detail">
      <header className="msd-header">
        <button type="button" className="msd-back" aria-label="返回股票列表" onClick={props.onBack}>‹</button>
        <MobileGameClock day={props.gameDay} tick={props.gameTick} variant="detail" />
        <button type="button" className="msd-stock-switch msd-previous" aria-label="上一只" disabled={props.canSwitchStock === false} onClick={props.onPrevious}>◀</button>
        <div className="msd-security-title"><strong>{props.name}</strong><small>{props.code}</small></div>
        <button type="button" className="msd-stock-switch msd-next" aria-label="下一只" disabled={props.canSwitchStock === false} onClick={props.onNext}>▶</button>
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
        <div className="msd-day-prices">{([["高", prices?.high], ["低", prices?.low], ["开", prices?.open]] as const).map(([label, price]) => <span key={label}>{label} <b className={price === undefined ? "flat" : tone(compareMoney(price, market.last_close))}>{price === undefined ? "--" : yuan(price)}</b></span>)}</div>
        <div className="msd-stock-stats"><span>昨收 <b>{yuan(market.last_close)}</b></span><span>当日成交量 <b>{formatTradeLots(facts.volume)}手</b></span><span>买一 <b className="rise">{market.best_bid ? yuan(market.best_bid) : "--"}</b></span><span>卖一 <b className="fall">{market.best_ask ? yuan(market.best_ask) : "--"}</b></span></div>
      </section>
      {props.watchlistControl && <div className="msd-watchlist">{props.watchlistControl}</div>}
      <ChartPeriodTabs period={props.period} onChange={props.onPeriodChange} panelId="mobile-chart-panel" variant="detail" />
      <div id="mobile-chart-panel" role="tabpanel" aria-label={`${props.period}图表`}>
        <div hidden={chartType !== "分时"}><IntradayPanel {...props} /></div>
        <div hidden={chartType === "分时"}><MarketKlinePanel code={props.code} key={props.code} dailyCandles={props.dailyCandles} period={props.period} indicatorCalculator={props.indicatorCalculator} /></div>
      </div>
      <div className="msd-info-tabs" role="tablist" aria-label="股票详情信息">
        {infoTabs.map((item) => <button type="button" role="tab" id={`info-${item}`} aria-controls="mobile-info-panel" aria-selected={props.infoTab === item} tabIndex={props.infoTab === item ? 0 : -1} key={item} onKeyDown={(event) => moveTabFocus(event, infoTabs)} onClick={() => props.onInfoTabChange(item)}>{item}</button>)}
      </div>
      <div id="mobile-info-panel" role="tabpanel" aria-labelledby={`info-${props.infoTab}`}>
        {props.infoTab === "资金" ? <FundsPanel activeDailyCandle={props.activeDailyCandle} /> : props.infoTab === "盘口" ? <section className="msd-info-book"><FiveLevelBook market={market} /></section> : props.companyContent}
      </div>
    </main>
  );
}
