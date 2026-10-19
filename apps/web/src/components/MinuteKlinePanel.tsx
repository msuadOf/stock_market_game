import { useEffect, useMemo, useRef, useState } from "react";
import type { MarketHistoryPage, MarketHistoryRequest } from "../host/market-history.ts";
import type { CurrentMinuteHistoryResponse } from "../host/current-minute-history.ts";
import { activeMinuteKlineBucketMinute, aggregateMinuteBars, aggregateMinuteKlines, type MinuteKline, type MinuteKlinePeriod } from "./minute-kline.ts";
import { formatDecimalCentsAsYuan, yuan } from "../utils/format.ts";
import { candleBodyPrices, candleWickPrices } from "../mobile/market-model.ts";
import { KlineCoordinatePlot } from "./KlineCoordinatePlot.tsx";
import { coordinateTicks, formatCoordinateValue, type KlineTimeTick } from "./kline-coordinates.ts";
import "./MinuteKlinePanel.css";

export interface MinuteKlinePanelProps {
  readonly query: (request: MarketHistoryRequest) => Promise<MarketHistoryPage>;
  readonly scopeKey: string;
  readonly code: string;
  readonly currentDate: string | null;
  readonly period: MinuteKlinePeriod;
  readonly queryCurrentMinuteHistory?: (request: { readonly code: string }) => Promise<CurrentMinuteHistoryResponse>;
  readonly tickRefreshKey?: string | number;
}

const phases = { OpenAuction: "开盘集合竞价", Continuous: "连续竞价", ClosingAuction: "收盘集合竞价" };
const currentPhases = { CallAuction: "开盘集合竞价", PreOpen: "开盘前", Continuous: "连续竞价", ClosingAuction: "收盘集合竞价", AfterClose: "收盘后", Closed: "休市" };
const labels: Record<MinuteKlinePeriod, string> = { 120: "120分钟", 60: "60分钟", 30: "30分钟", 15: "15分钟", 5: "5分钟", 1: "1分钟" };
const MAX_VOLUME_BAR_HEIGHT = 62;

export function minuteVolumeBarHeight(volume: bigint, maximum: bigint): number {
  if (volume < 0n || maximum < volume || maximum <= 0n) throw new RangeError("分钟成交量坐标超出真实股数范围");
  if (volume === 0n) return 0;
  const scaled = Number(volume * BigInt(MAX_VOLUME_BAR_HEIGHT) / maximum);
  return Math.max(1, scaled);
}

function shiftDate(date: string, days: number): string {
  const parsed = new Date(`${date}T00:00:00Z`);
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date) || !Number.isFinite(parsed.getTime()) || parsed.toISOString().slice(0, 10) !== date) throw new Error(`自然日期无效：${date}`);
  parsed.setUTCDate(parsed.getUTCDate() + days);
  return parsed.toISOString().slice(0, 10);
}

export function MinuteKlineDay({ date, bars, period, activeMinute = null }: { date: string; bars: readonly MinuteKline[]; period: MinuteKlinePeriod; activeMinute?: number | null }) {
  const prices = bars.flatMap((bar) => [Number(BigInt(bar.low)) / 100, Number(BigInt(bar.high)) / 100]);
  const minimum = Math.min(...prices), maximum = Math.max(...prices);
  const priceSpan = maximum - minimum || Math.max(Math.abs(maximum) * 0.01, 0.01);
  const priceMin = minimum - priceSpan * 0.05, priceMax = maximum + priceSpan * 0.05;
  const y = (value: number) => 180 - (value - priceMin) / (priceMax - priceMin) * 170;
  const x = (minute: number) => 12 + (minute - 565) / 335 * 366;
  const times: KlineTimeTick[] = [570, 630, 690, 780, 840, 900].map((minute) => ({ label: `${String(Math.floor(minute / 60)).padStart(2, "0")}:${String(minute % 60).padStart(2, "0")}`, x: x(minute) }));
  const priceTicks = coordinateTicks(priceMin, priceMax, y);
  const priceStep = (priceMax - priceMin) / 4;
  const volumeMax = bars.reduce((max, bar) => BigInt(bar.volume_shares) > max ? BigInt(bar.volume_shares) : max, 0n);
  const volumeTicks = [{ value: 1, y: 8 }, { value: 0.5, y: 39 }, { value: 0, y: 70 }];
  const volumeLabel = (fraction: number) => (BigInt(Math.round(fraction * 1_000_000)) * volumeMax / 1_000_000n).toLocaleString("zh-CN");
  const volumeHeight = (value: bigint) => minuteVolumeBarHeight(value, volumeMax);
  const volumeTop = (value: bigint) => 70 - volumeHeight(value);
  const coordinates = { priceY: (value: number) => y(value) };
  return <section className="minute-kline-day" aria-label={`${date}分钟K线`}>
    <h4>{date}</h4>
    <KlineCoordinatePlot className="minute-kline-price-plot" height={190} label="分钟K价格坐标，单位为元" ticks={priceTicks} times={times} format={(value) => formatCoordinateValue(value, priceStep)}>
      <svg className="msd-candle-chart minute-kline-chart" viewBox="0 0 390 190" preserveAspectRatio="none" role="img" aria-label={`${date}真实成交分钟K线，空档不补柱`}>
        {bars.map((bar) => {
          const center = x(bar.minute_of_day), scalePrice = (value: string) => Number(BigInt(value)) / 100;
          const candle = { open: scalePrice(bar.open), high: scalePrice(bar.high), low: scalePrice(bar.low), close: scalePrice(bar.close) };
          const body = candleBodyPrices(candle), wick = candleWickPrices(candle);
          const rise = BigInt(bar.close) >= BigInt(bar.open);
          return <g key={`${bar.minute_of_day}:${bar.phase}`} className={rise ? "rise" : "fall"}>
            <title>{`${String(Math.floor(bar.minute_of_day / 60)).padStart(2, "0")}:${String(bar.minute_of_day % 60).padStart(2, "0")} ${phases[bar.phase]} 开${yuan(bar.open)} 高${yuan(bar.high)} 低${yuan(bar.low)} 收${yuan(bar.close)} 股数${bar.volume_shares} 成交额${formatDecimalCentsAsYuan(bar.turnover_cents)}元`}</title>
            <line className="upper-wick" x1={center} x2={center} y1={coordinates.priceY(wick.upper.start)} y2={coordinates.priceY(wick.upper.end)} vectorEffect="non-scaling-stroke" />
            <rect x={center - 2} y={coordinates.priceY(body.top)} width="4" height={Math.max(1, coordinates.priceY(body.bottom) - coordinates.priceY(body.top))} />
            <line className="lower-wick" x1={center} x2={center} y1={coordinates.priceY(wick.lower.start)} y2={coordinates.priceY(wick.lower.end)} vectorEffect="non-scaling-stroke" />
          </g>;
        })}
      </svg>
    </KlineCoordinatePlot>
    <KlineCoordinatePlot className="minute-kline-volume-plot" height={75} label="分钟K成交量坐标，单位为股" ticks={volumeTicks} times={times} format={volumeLabel}>
      <svg className="msd-k-volume minute-kline-volume" viewBox="0 0 390 75" preserveAspectRatio="none" role="img" aria-label={`${date}分钟K成交量`}>
        {bars.map((bar) => { const rise = BigInt(bar.close) >= BigInt(bar.open); const volume = BigInt(bar.volume_shares), top = volumeTop(volume); return <rect key={`${bar.minute_of_day}:${bar.phase}`} className={rise ? "rise" : "fall"} x={x(bar.minute_of_day) - 2} y={top} width="4" height={volumeHeight(volume)} />; })}
      </svg>
    </KlineCoordinatePlot>
    <div className="minute-kline-time-row"><div className="minute-kline-time-axis" aria-label="分钟自然时间坐标">{times.map(({ label, x: position }, index) => <span key={label} style={{ left: `${position / 390 * 100}%`, transform: index === 0 ? "none" : index === times.length - 1 ? "translateX(-100%)" : "translateX(-50%)" }}>{label}</span>)}</div></div>
    <div className="minute-kline-table"><table><caption>{labels[period]}真实成交OHLCV；金额保持十进制分精度</caption><thead><tr><th>时刻／阶段</th><th>开盘（元）</th><th>最高（元）</th><th>最低（元）</th><th>收盘（元）</th><th>成交股数</th><th>成交额（元）</th></tr></thead><tbody>{bars.map((bar) => <tr key={`${bar.minute_of_day}:${bar.phase}`}><td>{String(Math.floor(bar.minute_of_day / 60)).padStart(2, "0")}:{String(bar.minute_of_day % 60).padStart(2, "0")} {phases[bar.phase]}{activeMinute === bar.minute_of_day && bar.phase === "Continuous" ? "（形成中）" : ""}</td><td>{yuan(bar.open)}</td><td>{yuan(bar.high)}</td><td>{yuan(bar.low)}</td><td>{yuan(bar.close)}</td><td>{bar.volume_shares}</td><td>{formatDecimalCentsAsYuan(bar.turnover_cents)}</td></tr>)}</tbody></table></div>
  </section>;
}

export function MinuteKlinePanel({ query, scopeKey, code, currentDate, period, queryCurrentMinuteHistory, tickRefreshKey }: MinuteKlinePanelProps) {
  const initialFrom = currentDate === null ? "" : shiftDate(currentDate, -30);
  const initialTo = currentDate === null ? "" : shiftDate(currentDate, -1);
  const [from, setFrom] = useState(initialFrom);
  const [to, setTo] = useState(initialTo);
  const [pages, setPages] = useState<readonly MarketHistoryPage[]>([]);
  const [index, setIndex] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [liveHistory, setLiveHistory] = useState<CurrentMinuteHistoryResponse | null>(null);
  const [liveLoading, setLiveLoading] = useState(false);
  const [liveError, setLiveError] = useState<string | null>(null);
  const owner = useRef({ alive: false, sequence: 0 });
  const liveOwner = useRef({ alive: false, sequence: 0 });
  useEffect(() => {
    owner.current.alive = true;
    liveOwner.current.alive = true;
    owner.current.sequence += 1;
    liveOwner.current.sequence += 1;
    setFrom(currentDate === null ? "" : shiftDate(currentDate, -30));
    setTo(currentDate === null ? "" : shiftDate(currentDate, -1));
    setPages([]); setIndex(0); setLoading(false); setError(null); setLiveHistory(null); setLiveLoading(false); setLiveError(null);
    return () => { owner.current.alive = false; owner.current.sequence += 1; liveOwner.current.alive = false; liveOwner.current.sequence += 1; };
  }, [scopeKey, code, query, currentDate]);
  useEffect(() => {
    const sequence = ++liveOwner.current.sequence;
    if (queryCurrentMinuteHistory === undefined || currentDate === null) { setLiveHistory(null); setLiveLoading(false); setLiveError(null); return; }
    setLiveLoading(true); setLiveError(null);
    void queryCurrentMinuteHistory({ code }).then((response) => {
      if (!liveOwner.current.alive || liveOwner.current.sequence !== sequence) return;
      if (response.code !== code || response.live !== true || response.date !== currentDate) throw new Error("当前分钟响应证券、自然日错配或不是实时事实");
      setLiveHistory(response);
    }).catch((failure: unknown) => {
      if (liveOwner.current.alive && liveOwner.current.sequence === sequence) {
        setLiveHistory((previous) => previous?.date === currentDate ? previous : null);
        setLiveError(`当前分钟查询失败：${failure instanceof Error ? failure.message : String(failure)}；请重试并反馈错误详情。`);
      }
    }).finally(() => {
      if (liveOwner.current.alive && liveOwner.current.sequence === sequence) setLiveLoading(false);
    });
    return () => { liveOwner.current.sequence += 1; };
  }, [scopeKey, code, currentDate, queryCurrentMinuteHistory, tickRefreshKey]);
  const page = pages[index];
  const bars = useMemo(() => page === undefined ? [] : aggregateMinuteKlines(page.entries, period), [page, period]);
  const liveBars = useMemo(() => liveHistory === null ? [] : aggregateMinuteBars(liveHistory.date, liveHistory.bars, period), [liveHistory, period]);
  const visibleLiveHistory = currentDate !== null && liveHistory?.date === currentDate ? liveHistory : null;
  const liveActiveMinute = visibleLiveHistory?.phase === "Continuous" ? activeMinuteKlineBucketMinute(Math.floor(visibleLiveHistory.observed_at.second_of_day / 60), period) : null;
  const invalidate = () => { owner.current.sequence += 1; setPages([]); setIndex(0); setLoading(false); setError(null); };
  const load = async (request: MarketHistoryRequest, previous: readonly MarketHistoryPage[]) => {
    const sequence = ++owner.current.sequence;
    setLoading(true); setError(null);
    try {
      const scopedQuery = async (input: MarketHistoryRequest) => {
        if (!owner.current.alive || owner.current.sequence !== sequence) throw new Error("历史查询范围已切换");
        const result = await query(input);
        if (!owner.current.alive || owner.current.sequence !== sequence) throw new Error("历史响应属于已切换范围");
        return result;
      };
      const result = await scopedQuery(request);
      if (!owner.current.alive || owner.current.sequence !== sequence) return;
      setPages([...previous, result]); setIndex(previous.length);
    } catch (failure) {
      if (owner.current.alive && owner.current.sequence === sequence) setError(`分钟K线查询失败：${failure instanceof Error ? failure.message : String(failure)}；请核对日期并重试。`);
    } finally { if (owner.current.alive && owner.current.sequence === sequence) setLoading(false); }
  };
  const search = () => {
    if (from === "" || to === "") { setError("请先选择完整的起止日期。"); return; }
    if (from > to) { setError("起始日期不能晚于结束日期。"); return; }
    invalidate();
    void load({ code, date_from: from, date_to: to, after: null, page_size: 31 }, []);
  };
  const next = () => {
    if (page === undefined || page.next_cursor === null) throw new Error("分钟K线没有下一页游标");
    if (index + 1 < pages.length) { setIndex(index + 1); return; }
    void load({ code, date_from: from, date_to: to, after: page.next_cursor, page_size: 31 }, pages.slice(0, index + 1));
  };
  const byDate = new Map<string, MinuteKline[]>();
  for (const bar of bars) byDate.set(bar.date, [...(byDate.get(bar.date) ?? []), bar]);
  return <section className="minute-kline" aria-label="分钟K线历史">
    <h3>{labels[period]}历史K线</h3>
    <p>仅显示已成功日结的真实成交；自然日和午间时段分别聚合，集合竞价单独成柱。无成交时段不补柱，金额按精确成交分累计。当前交易日不在此历史页内。</p>
    <form onSubmit={(event) => { event.preventDefault(); search(); }}>
      <label>起始日期<input aria-label="分钟K线起始日期" type="date" value={from} onChange={(event) => { setFrom(event.currentTarget.value); invalidate(); }} required /></label>
      <label>结束日期<input aria-label="分钟K线结束日期" type="date" value={to} onChange={(event) => { setTo(event.currentTarget.value); invalidate(); }} required /></label>
      <button type="submit" disabled={loading}>查询历史K线</button>
    </form>
    <div className="minute-kline-pagination"><button type="button" disabled={index === 0 || loading} onClick={() => setIndex(index - 1)}>上一页</button><span>第{index + 1}页，每页最多31个自然日</span><button type="button" disabled={page === undefined || page.next_cursor === null || loading} onClick={next}>下一页</button></div>
    {loading && <p role="status">正在查询已结束日期…</p>}
    {error !== null && <p role="alert">{error}</p>}
    {page !== undefined && <><p>日终历史截止：{page.settled_through ?? "尚未首次日结"}；此页日期 {page.entries[0]?.date ?? "无"} 至 {page.entries.at(-1)?.date ?? "无"}。</p>{byDate.size === 0 ? <p>所选日期页没有已日结分钟成交。</p> : [...byDate].map(([date, values]) => <MinuteKlineDay key={date} date={date} bars={values} period={period} />)}</>}
    <section className="minute-kline-current" aria-label="当前交易日实时分钟K">
      <h4>当前交易日实时分钟K</h4>
      {queryCurrentMinuteHistory === undefined && <p>当前分钟历史宿主暂不可用。</p>}
      {liveLoading && <p role="status">正在读取当前真实分钟成交…</p>}
      {liveError !== null && <p role="alert">{liveError}</p>}
      {currentDate === null && <p>当前自然日尚未同步，暂不能查询活动分钟。</p>}
      {visibleLiveHistory !== null && <><p>{visibleLiveHistory.date}　{currentPhases[visibleLiveHistory.phase]}　观察时刻 {String(Math.floor(visibleLiveHistory.observed_at.second_of_day / 3600)).padStart(2, "0")}:{String(Math.floor(visibleLiveHistory.observed_at.second_of_day % 3600 / 60)).padStart(2, "0")}:{String(visibleLiveHistory.observed_at.second_of_day % 60).padStart(2, "0")}。柱仅来自已经发生的真实成交；{liveActiveMinute === null ? "当前没有形成中的连续竞价桶。" : "当前连续竞价桶标记为形成中。"}</p>{liveBars.length === 0 ? <p>当前尚无真实分钟成交，不生成K线柱。</p> : <MinuteKlineDay date={`${visibleLiveHistory.date}（实时）`} bars={liveBars} period={period} activeMinute={liveActiveMinute} />}</>}
    </section>
  </section>;
}
