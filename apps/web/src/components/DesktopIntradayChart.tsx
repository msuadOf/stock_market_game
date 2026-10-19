import { auctionContinuousJoin, auctionDisplayPoints } from "./intraday-auction-display.ts";
import { centsToYuanText, moneyToChartNumber } from "../utils/money.ts";
import type { Cents } from "../types/engine.ts";
import { Fragment } from "react";
import { intradayChartX, symmetricIntradayScale, type MobileIntradayProjection } from "../mobile/market-model.ts";
import { formatSharesAsLots } from "../utils/format.ts";
import { parseIndicatorResults, type IndicatorResultState } from "./indicator-results.ts";
import type { PriceChartIndicator } from "./price-chart-runtime.ts";
import "./desktop-intraday.css";

interface Props {
  projection: MobileIntradayProjection;
  lastClose: Cents;
  dayRange?: Readonly<{ high: number; low: number; volume?: number }>;
  indicator: PriceChartIndicator;
  result: IndicatorResultState;
  averagePoints?: readonly { time: number; value: number }[];
}
const times = [[0, "09:15"], [16, "09:30"], [37, "10:30"], [58, "11:30/13:00"], [79, "14:00"], [100, "15:00"]] as const;
function Grid() {
  return <><rect x="0" y="0" width="16" height="100" className="intraday-auction-zone" />{[16, 37, 58, 79].map(x => <line key={x} x1={x} x2={x} y1="0" y2="100" className="intraday-gridline" />)}</>;
}
function TimeAxis() {
  return <div className="intraday-time-axis">{times.map(([x, text]) => <span key={x} style={{ left: `${x}%` }}>{text}</span>)}</div>;
}

/** 与移动端共享行情投影；价格、量能和指标使用同一全天槽位，不拉伸已发生行情。 */
export function DesktopIntradayChart({ projection: p, lastClose, dayRange, indicator, result, averagePoints = [] }: Props) {
  // 分时0%轴固定居中；复用手机的对称价域，按已出现的最大偏离展开。
  const close = moneyToChartNumber(lastClose) / 100;
  const auction = auctionDisplayPoints(p.visibleAuctionPoints, close);
  const prices = [...auction, ...p.visiblePoints].map(point => point.value);
  for (const point of averagePoints) {
    intradayChartX({ phase: "continuous", minute: point.time });
    if (!Number.isFinite(point.value) || point.value <= 0) throw new Error("分时均价绘图必须使用真实有限正价");
    prices.push(point.value);
  }
  if (dayRange && dayRange.volume !== undefined && dayRange.volume > 0) prices.push(dayRange.high, dayRange.low);
  const { top: high, bottom: low } = symmetricIntradayScale(prices, close, 0);
  const priceY = (value: number) => value === close ? 50 : (high - value) / (high - low) * 100;
  const rows = [0, 1, 2, 3, 4].map(index => ({ y: index * 25, value: index === 2 ? close : high - (high - low) * index / 4 }));
  const priceClass = (value: number) => value > close ? "up" : value < close ? "down" : "";
  const empty = p.visiblePoints.length === 0 && p.visibleAuctionPricePoints.length === 0;
  const points = p.visiblePoints;
  const sessionJoin = auctionContinuousJoin(auction, points);
  const ready = (indicator === "macd" || indicator === "kdj") && result.kind === "ready"
    ? parseIndicatorResults(result.value, points.length, 0) : null;
  const lines = ready ? (indicator === "macd"
    ? [{ name: "DIF", values: ready.macd.dif, color: "#c35670" }, { name: "DEA", values: ready.macd.dea, color: "#467db9" }]
    : [{ name: "K", values: ready.priceKdj.k, color: "#ac7b00" }, { name: "D", values: ready.priceKdj.d, color: "#9b58b5" }, { name: "J", values: ready.priceKdj.j, color: "#328450" }]) : [];
  const values = [...lines.flatMap(line => line.values), ...(ready && indicator === "macd" ? ready.macd.histogram : [])];
  const min = Math.min(0, ...values);
  const max = Math.max(0, ...values);
  const indicatorY = (value: number) => max === min ? 50 : 95 - (value - min) / (max - min) * 90;
  return <div className="desktop-intraday" data-signature={p.signature}>
    <div className="intraday-caption"><span>分时走势 · 元</span><span>昨收 {centsToYuanText(lastClose)}</span></div>
    <div className="intraday-price-area">
      {rows.map(({ y, value }, index) => <Fragment key={index}><span className={`intraday-price-label ${priceClass(value)}`} style={{ top: `${y}%` }}>{value.toFixed(2)}</span><span className={`intraday-percent-label ${priceClass(value)}`} style={{ top: `${y}%` }}>{((value / close - 1) * 100).toFixed(2)}%</span></Fragment>)}
      <svg className="intraday-price-plot" viewBox="0 0 100 100" preserveAspectRatio="none" role="img" aria-label="全天分时走势，左侧价格，右侧相对昨收涨跌幅">
        <Grid />
        {rows.map(({ y }) => <line key={y} x1="0" x2="100" y1={y} y2={y} className="intraday-gridline" />)}
        <line x1="0" x2="100" y1="50" y2="50" className="intraday-baseline" />
        {auction.length > 0 && <polyline className="intraday-auction-line" points={auction.map(point => `${intradayChartX({ phase: "auction", minute: point.time })},${priceY(point.value)}`).join(" ")}><title>无指示价时沿昨收0%参考轴显示；粗点表示竞价指示更新，并非已成交</title></polyline>}
        {auction.filter(point => point.updated).map(point => <circle key={point.time} className="intraday-auction-point" cx={intradayChartX({ phase: "auction", minute: point.time })} cy={priceY(point.value)} r="0.15" stroke="currentColor" strokeWidth="3" vectorEffect="non-scaling-stroke"><title>竞价指示价或可匹配量更新</title></circle>)}
        {sessionJoin.length > 0 && <polyline className="intraday-session-join" points={sessionJoin.map(point => `${point.x},${priceY(point.value)}`).join(" ")} />}
        {points.length > 0 && <polyline className="intraday-price-line" points={points.map(point => `${intradayChartX({ phase: "continuous", minute: point.time })},${priceY(point.value)}`).join(" ")} />}
        {indicator === "intradayAverage" && averagePoints.length > 0 && <polyline className="intraday-average-line" stroke="#ad7900" fill="none" strokeWidth="1" vectorEffect="non-scaling-stroke" points={averagePoints.map(point => `${intradayChartX({ phase: "continuous", minute: point.time })},${priceY(point.value)}`).join(" ")} />}
      </svg>
      {empty && <div className="intraday-empty" role="status">等待行情<br /><small>暂无有效指示价，按昨收0%参考轴显示</small></div>}
    </div>
    <TimeAxis />
    {indicator !== "none" && indicator !== "intradayAverage" && <div className="intraday-secondary">
      <div className="intraday-volume-caption">{indicator === "volume" ? <><span>竞价累计量 · 上限 {formatSharesAsLots(p.volumeScale.auctionMax)}手</span><span>分钟成交量 · 上限 {formatSharesAsLots(p.volumeScale.continuousMax)}手</span></> : <span>{indicator.toUpperCase()} {lines.map(line => line.name).join(" / ")}</span>}</div>
      <div className="intraday-secondary-area"><svg className="intraday-volume-plot" viewBox="0 0 100 100" preserveAspectRatio="none" role="img" aria-label={indicator === "volume" ? "全天量能，与价格时间轴对齐；竞价累计量与分钟成交量分别缩放" : `${indicator.toUpperCase()} 全天指标`}>
        <Grid />
        {indicator === "volume" && p.volumeMarks().map(mark => <line key={`${mark.phase}:${mark.time}`} x1={mark.x} x2={mark.x} y1="100" y2={100 - mark.height} stroke={mark.buy ? "var(--up)" : "var(--down)"} className="intraday-volume-mark"><title>{`${mark.phase === "auction" ? "竞价累计量" : "分钟成交量"} ${formatSharesAsLots(mark.volume ?? 0)}手`}</title></line>)}
        {ready && indicator === "macd" && ready.macd.histogram.map((value, index) => <line key={index} x1={intradayChartX({ phase: "continuous", minute: points[index].time })} x2={intradayChartX({ phase: "continuous", minute: points[index].time })} y1={indicatorY(0)} y2={indicatorY(value)} stroke={value >= 0 ? "var(--up)" : "var(--down)"} className="intraday-volume-mark" />)}
        {lines.map(line => <polyline key={line.name} className="intraday-indicator-line" stroke={line.color} points={line.values.map((value, index) => `${intradayChartX({ phase: "continuous", minute: points[index].time })},${indicatorY(value)}`).join(" ")} />)}
      </svg></div>
    </div>}
  </div>;
}
