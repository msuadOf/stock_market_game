import { subtractMoney, compareMoney } from "../utils/money.ts";
import { yuan, formatCentsAmount } from "../utils/format.ts";
import { useKlineGestures } from "./useKlineGestures.ts";
import { klineTapIndex } from "./kline-gestures.ts";
import { ChartDisplayMenu } from "./ChartDisplayMenu.tsx";
import { CHART_INDICATORS } from "./chart-display-options.ts";
import { useDispatch, useSelector } from "react-redux";
import type { AppDispatch, RootState } from "../store/store.ts";
import { changeChartViewport, selectChartViewport, setChartIndicator, toggleChartAverage } from "../store/chart-settings-slice.ts";
import { useMemo, useState, type CSSProperties } from "react";
import type { KlinePoint } from "./PriceChart.tsx";
import type { IndicatorCalculator } from "./indicator-results.ts";
import { useIndicatorResults } from "./useIndicatorResults.ts";
import { KLINE_MOVING_AVERAGES, klineMovingAverage } from "./kline-moving-averages.ts";
import { KlineViewportControls } from "../mobile/KlineViewportControls.tsx";
import { aggregateCandles, klineWindow, formatTradeLots, priceChangePercent, MobileKlineProjection, type KlineViewportAction } from "../mobile/market-model.ts";
import type { MobileChartPeriod } from "../mobile/mobile-ui-state.ts";
import "../mobile/MobileStockDetail.css";
import { coordinateTicks, formatCoordinateValue, klineTimeTicks, klineTradingDayLabel, klineVolumeTicks } from "./kline-coordinates.ts";
import { KlineCoordinatePlot, KlineTimeAxis } from "./KlineCoordinatePlot.tsx";
interface Props { code: string; dailyCandles: readonly KlinePoint[]; period: MobileChartPeriod; indicatorCalculator: IndicatorCalculator | null; }
export function MarketKlinePanel({ code, dailyCandles, period, indicatorCalculator }: Props) {
  const candlePeriod = period === "周K" || period === "月K" ? period : "日K";
  const allCandles = useMemo(() => aggregateCandles(dailyCandles, candlePeriod), [candlePeriod, dailyCandles]);
  const indicatorInput = useMemo(() => ({
    prices: allCandles.map(candle => candle.close),
    candles: allCandles.map(({ high, low, close }) => ({ high, low, close })),
  }), [allCandles]);
  const indicatorResult = useIndicatorResults(indicatorCalculator, indicatorInput, allCandles.length > 0);
  const dispatch = useDispatch<AppDispatch>();
  const storedViewport = useSelector((state: RootState) => selectChartViewport(state.chartSettings, code));
  const viewport = klineWindow(allCandles.length, storedViewport.capacity, storedViewport.offsetFromEnd);
  const indicator = useSelector((state: RootState) => state.chartSettings.indicator);
  const selected = useSelector((state: RootState) => state.chartSettings.selectedAverages);
  const [selectedTime, setSelectedTime] = useState<number | null>(null);
  const toggleAverage = (days: number) => dispatch(toggleChartAverage(days));
  const setIndicator = (value: typeof indicator) => dispatch(setChartIndicator(value));
  const act = (action: KlineViewportAction) => dispatch(changeChartViewport({ code, total: allCandles.length, action }));
  const gestureRef = useKlineGestures(act, (x, width) => {
    const range = klineWindow(allCandles.length, viewport.capacity, viewport.offsetFromEnd);
    const index = klineTapIndex(x, width, range.capacity, range.end - range.start);
    if (index !== null) setSelectedTime(allCandles[range.start + index].time);
  }, selectedTime !== null, () => setSelectedTime(null));
  if (allCandles.length === 0) return <section ref={gestureRef} className="msd-kline msd-chart-empty" aria-label={`${period}图`}><b>{period}</b><p>等待游戏生成首个交易日 K 线…</p></section>;
  const window = klineWindow(allCandles.length, viewport.capacity, viewport.offsetFromEnd);
  const averages = KLINE_MOVING_AVERAGES.map(item => ({ ...item, points: klineMovingAverage(allCandles.slice(0, window.end), item.days, window.end - window.start) }));
  const projection = MobileKlineProjection.fromInputs(allCandles, viewport, indicatorResult, averages.filter(item => selected.includes(item.days)).flatMap(item => item.points.map(point => point.value)));
  const candles = projection.visibleCandles;
  const slots = new Map(candles.map((candle, index) => [candle.time, index]));
  const kdj = projection.kdj;
  const volumes = projection.volumes;
  const selectedIndex = candles.findIndex(candle => candle.time === selectedTime);
  const displayIndex = selectedIndex < 0 ? candles.length - 1 : selectedIndex;
  const displayTime = candles[displayIndex]?.time;
  const maValue = (points: { time: number; value: number }[]) => points.find(point => point.time === displayTime)?.value;
  const crosshairX = selectedIndex < 0 ? null : projection.slotFor(selectedIndex).center;
  const times = klineTimeTicks(projection);
  const priceTicks = coordinateTicks(projection.priceScale.min, projection.priceScale.max, value => projection.priceY(value));
  const priceStep = (projection.priceScale.max - projection.priceScale.min) / 4;
  const volumeTicks = klineVolumeTicks(projection.volumeMax);
  const indicatorTicks = coordinateTicks(projection.indicatorScale.min, projection.indicatorScale.max, value => 68 - (value - projection.indicatorScale.min) / (projection.indicatorScale.max - projection.indicatorScale.min) * 64, 3);
  const priceFormat = (value: number) => formatCoordinateValue(value, priceStep);
  const indicatorFormat = (value: number) => formatCoordinateValue(value, (projection.indicatorScale.max - projection.indicatorScale.min) / 2);
  const macd = indicator === "macd" && indicatorResult.kind === "ready" ? macdWindow(projection, indicatorResult.value.macd) : null;
  const axisLabels = [...priceTicks.map(tick => priceFormat(tick.value)), ...volumeTicks.map(tick => formatTradeLots(tick.value)), ...indicatorTicks.map(tick => indicatorFormat(tick.value)), ...(macd === null ? [] : macd.ticks.map(tick => formatCoordinateValue(tick.value, (macd.max - macd.min) / 2)))];
  const axisWidth = Math.max(54, ...axisLabels.map(label => label.length * 7 + 8));
  return <section
    ref={gestureRef}
    style={{ "--kline-axis-width": `${axisWidth}px` } as CSSProperties}
    data-capacity={viewport.capacity}
    data-offset={viewport.offsetFromEnd}
    className="msd-kline"
    aria-label={`${period}图`}
    data-kline-count={allCandles.length}
    data-kline-signature={projection.latestSignature}
  >
    <div className="msd-kline-meta" role="group" aria-label="均线（可多选）"><ChartDisplayMenu selected={selected} onToggleAverage={toggleAverage} indicator={indicator} onIndicator={setIndicator} />{averages.map(({ days, color, points }) => <button key={days} type="button" aria-pressed={selected.includes(days)} aria-label={`MA${days}：${maValue(points)?.toFixed(3) ?? "历史不足"}`} style={{ color, background: "transparent", textDecoration: selected.includes(days) ? "none" : "line-through" }} onClick={() => toggleAverage(days)}>MA{days}:{maValue(points)?.toFixed(3) ?? "—"}</button>)}</div>
    {candlePeriod !== "日K" && <div className="kline-period-note">{candlePeriod}：每{candlePeriod === "周K" ? 5 : 20}个游戏交易日合并（简化）</div>}
    <KlineCoordinatePlot className="kline-price-plot" height={190} label="价格坐标，单位为元" ticks={priceTicks} times={times} format={priceFormat}><svg className="msd-candle-chart" aria-label={`${period}价格，单位为元`} viewBox="0 0 390 190" preserveAspectRatio="none">{candles.map((c, index) => { const { slot, rise, body, wick } = projection.candleBodyAndWick(index); return <g key={`${c.time}-${index}`} className={rise?"rise":"fall"}><line className="upper-wick" x1={slot.center} x2={slot.center} y1={wick.upper.start} y2={wick.upper.end}/><rect x={slot.center-slot.markWidth/2} y={body.top} width={slot.markWidth} height={Math.max(1,body.bottom-body.top)}/><line className="lower-wick" x1={slot.center} x2={slot.center} y1={wick.lower.start} y2={wick.lower.end}/></g>; })}{averages.filter(item => selected.includes(item.days)).map(({ days, color, points }) => <polyline key={days} stroke={color} points={points.map(point => `${projection.slotFor(slots.get(point.time)!).center},${projection.priceY(point.value)}`).join(" ")} />)}<KlineCrosshair x={crosshairX} height={190} /></svg></KlineCoordinatePlot>
    <KlineTimeAxis times={times} selected={crosshairX === null ? null : { x: crosshairX, label: klineTradingDayLabel(candles[selectedIndex].time) }} />
    {selectedIndex >= 0 && <KlineDetails period={candlePeriod} previousRawClose={allCandles[window.start + selectedIndex - 1]?.rawPrices?.close} side={crosshairX !== null && crosshairX < 195 ? "right" : "left"} candle={candles[selectedIndex]} previousClose={allCandles[window.start + selectedIndex - 1]?.close} averages={averages.map(item => ({ days: item.days, color: item.color, value: maValue(item.points) }))} onClose={() => setSelectedTime(null)} />}
    {indicator !== "none" && <><div className="msd-volume-title">成交量（手）　<span>量:{formatTradeLots(volumes[displayIndex] ?? 0)}手</span></div><KlineCoordinatePlot className="kline-volume-plot" height={75} label="成交量坐标，单位为手" ticks={volumeTicks} times={times} format={formatTradeLots}><svg className="msd-k-volume" viewBox="0 0 390 75" preserveAspectRatio="none" aria-label={`${period}成交量，单位为手`}>{projection.volumeMarks().map(({ slot, height, rise }, index) => <rect key={index} className={rise?"rise":"fall"} x={slot.center-slot.markWidth/2} y={75-height} width={slot.markWidth} height={height}/>) }<KlineCrosshair x={crosshairX} height={75} /></svg></KlineCoordinatePlot></>}
    {indicator === "kdj" && (kdj === null ? <div className="msd-kdj-title" role={indicatorResult.kind === "error" || indicatorResult.kind === "unavailable" ? "alert" : "status"}>{indicatorResult.kind === "pending" ? "Rust 指标计算中…" : indicatorResult.kind === "error" ? `Rust 指标计算失败：${indicatorResult.message}` : indicatorResult.kind === "unavailable" ? "Rust 指标宿主尚未就绪" : "等待 Rust 指标…"}</div> : <><div className="msd-kdj-title">KDJ(9,3,3)　 K:{kdj.k[displayIndex]?.toFixed(2)}　<span>D:{kdj.d[displayIndex]?.toFixed(2)}</span>　<em>J:{kdj.j[displayIndex]?.toFixed(2)}</em></div><KlineCoordinatePlot className="kline-indicator-plot" height={72} label="KDJ坐标" ticks={indicatorTicks} times={times} format={indicatorFormat}><svg className="msd-kdj" viewBox="0 0 390 72" preserveAspectRatio="none"><polyline points={projection.indicatorLine(kdj.k)}/><polyline className="orange" points={projection.indicatorLine(kdj.d)}/><polyline className="pink" points={projection.indicatorLine(kdj.j)}/><KlineCrosshair x={crosshairX} height={72} /></svg></KlineCoordinatePlot></>)}
    {indicator === "macd" && <MacdPanel projection={projection} result={indicatorResult} crosshairX={crosshairX} displayIndex={displayIndex} />}
    <div className="kline-footer"><div className="shared-indicator-controls">{CHART_INDICATORS.map(([value, label]) => <button key={value} className="chart-indicator-button" aria-pressed={indicator === value} onClick={() => setIndicator(value)}>{label}</button>)}</div><KlineViewportControls total={allCandles.length} viewport={viewport} onAction={act} /></div>
  </section>;
}



/** MACD 图形与刻度共用可见窗口和纵轴，保留全零居中的原有规则。 */
function macdWindow(projection: MobileKlineProjection, macd: import("./indicator-results.ts").IndicatorResults["macd"]) {
  const { start, end } = projection.visibleWindow;
  const dif = macd.dif.slice(start, end), dea = macd.dea.slice(start, end), histogram = macd.histogram.slice(start, end);
  const min = Math.min(0, ...dif, ...dea, ...histogram), max = Math.max(0, ...dif, ...dea, ...histogram);
  const y = (value: number) => max === min ? 36 : 68 - (value - min) / (max - min) * 64;
  return { dif, dea, histogram, min, max, y, ticks: coordinateTicks(min, max, y, 3) };
}

export function MacdPanel({ projection, result, crosshairX = null, displayIndex = projection.visibleCandles.length - 1 }: { displayIndex?: number; crosshairX?: number | null; projection: MobileKlineProjection; result: import("./useIndicatorResults.ts").IndicatorResultState }) {
  if (result.kind !== "ready") return <div role={result.kind === "error" || result.kind === "unavailable" ? "alert" : "status"}>{result.kind === "error" ? `Rust 指标计算失败：${result.message}` : result.kind === "unavailable" ? "Rust 指标宿主尚未就绪" : "Rust 指标计算中…"}</div>;
  const { dif, dea, histogram, min, max, y, ticks } = macdWindow(projection, result.value.macd);
  const line = (values: readonly number[]) => values.map((value, index) => `${projection.slotFor(index).center},${y(value)}`).join(" ");
  return <><div className="msd-kdj-title">MACD(12,26,9)　DIF:{dif[displayIndex]?.toFixed(2)}　DEA:{dea[displayIndex]?.toFixed(2)}</div><KlineCoordinatePlot className="kline-indicator-plot" height={72} label="MACD坐标" ticks={ticks} times={klineTimeTicks(projection)} format={value => formatCoordinateValue(value, (max - min) / 2)}><svg className="msd-kdj" viewBox="0 0 390 72" preserveAspectRatio="none">{histogram.map((value, index) => <line key={index} x1={projection.slotFor(index).center} x2={projection.slotFor(index).center} y1={y(0)} y2={y(value)} stroke={value >= 0 ? "#ed3943" : "#15952d"} vectorEffect="non-scaling-stroke" />)}<polyline points={line(dif)} /><polyline className="orange" points={line(dea)} /><KlineCrosshair x={crosshairX} height={72} /></svg></KlineCoordinatePlot></>;
}

function KlineCrosshair({ x, height }: { x: number | null; height: number }) {
  return x === null ? null : <line className="kline-crosshair" x1={x} x2={x} y1={0} y2={height} stroke="#111" strokeWidth={1} vectorEffect="non-scaling-stroke" pointerEvents="none" />;
}

export function KlineDetails({ candle, previousClose, averages, onClose, side = "left", previousRawClose, period = "日K" }: { period?: string; previousRawClose?: string; side?: "left" | "right"; candle: KlinePoint; previousClose?: number; averages: { days: number; color: string; value?: number }[]; onClose: () => void }) {
  const delta = previousClose === undefined ? undefined : candle.close - previousClose;
  const percent = previousClose === undefined || previousClose === 0 ? undefined : (candle.close / previousClose - 1) * 100;
  const exactDelta = candle.rawPrices && previousRawClose !== undefined ? subtractMoney(candle.rawPrices.close, previousRawClose) : undefined;
  const exactPercent = candle.rawPrices && previousRawClose !== undefined && compareMoney(previousRawClose, "0") !== 0 ? priceChangePercent(candle.rawPrices.close, previousRawClose) : undefined;
  const shownPercent = exactPercent === undefined ? percent : exactPercent;
  return <aside className="kline-details" style={side === "right" ? { left: "auto", right: 10 } : undefined} aria-label="K线详细信息"><div className="kline-details-head"><strong>K线详情</strong><button type="button" aria-label="关闭K线详细信息" onClick={onClose}>×</button></div><dl>
    <dt>{period === "日K" ? "交易日" : `${period}起始日`}</dt><dd>{klineTradingDayLabel(candle.time, false)}</dd>
    {([ ["开盘", candle.open, candle.rawPrices?.open], ["最高", candle.high, candle.rawPrices?.high], ["最低", candle.low, candle.rawPrices?.low], ["收盘", candle.close, candle.rawPrices?.close] ] as const).map(([name, value, raw]) => <div className="kline-detail-row" key={name}><dt>{name}</dt><dd>{raw === undefined ? value.toFixed(2) : yuan(raw)}</dd></div>)}
    <dt>涨跌额</dt><dd>{exactDelta !== undefined ? `${compareMoney(exactDelta, "0") >= 0 ? "+" : ""}${yuan(exactDelta)}` : delta === undefined ? "—" : `${delta >= 0 ? "+" : ""}${delta.toFixed(2)}`}</dd><dt>涨跌幅</dt><dd>{shownPercent === undefined ? "—" : `${shownPercent >= 0 ? "+" : ""}${shownPercent.toFixed(2)}%`}</dd>
    <dt>成交量</dt><dd>{candle.volume === undefined ? "—" : `${formatTradeLots(candle.volume)}手`}</dd>
    <dt>成交额</dt><dd>{candle.tradeStats === undefined ? "—" : `${formatCentsAmount(candle.tradeStats.turnoverCents)}元`}</dd>
    {averages.map(item => <div className="kline-detail-row" key={item.days} style={{ color: item.color }}><dt>MA{item.days}</dt><dd>{item.value?.toFixed(3) ?? "—"}</dd></div>)}
  </dl></aside>;
}
