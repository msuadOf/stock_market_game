import { useEffect, useMemo, useState } from "react";
import type { CurrentMinuteHistoryResponse } from "../host/current-minute-history.ts";
import { activeMinuteKlineBucketMinute, aggregateMinuteBars, minuteVolumeBarHeight, type MinuteKline, type MinuteKlinePeriod } from "./minute-kline.ts";
import { formatDecimalCentsAsYuan, yuan } from "../utils/format.ts";
import { candleBodyPrices, candleWickPrices } from "../mobile/market-model.ts";
import { KlineCoordinatePlot } from "./KlineCoordinatePlot.tsx";
import { coordinateTicks, formatCoordinateValue, type KlineTimeTick } from "./kline-coordinates.ts";
import "./MinuteKlinePanel.css";

export interface MinuteKlinePanelProps {
  readonly scopeKey: string;
  readonly code: string;
  readonly currentDate: string | null;
  readonly period: MinuteKlinePeriod;
  readonly queryCurrentMinuteHistory?: (request: { readonly code: string }) => Promise<CurrentMinuteHistoryResponse>;
  readonly tickRefreshKey?: string | number;
  /** 当前连续竞价墙钟分钟（570..=690 / 780..=900），与 engine
   *  `MinuteBarFact.minute_of_day` 同口径——"形成中"桶标记与观察时刻用；
   *  B2 wire 不再下发 observed_at，缺省 null 不标记（不伪造观察时刻）。 */
  readonly currentMinuteOfDay?: number | null;
}

const phases = { OpenAuction: "开盘集合竞价", Continuous: "连续竞价", ClosingAuction: "收盘集合竞价" };
const currentPhases = { CallAuction: "开盘集合竞价", PreOpen: "开盘前", Continuous: "连续竞价", ClosingAuction: "收盘集合竞价", AfterClose: "收盘后", Closed: "休市" };
const labels: Record<MinuteKlinePeriod, string> = { 120: "120分钟", 60: "60分钟", 30: "30分钟", 15: "15分钟", 5: "5分钟", 1: "1分钟" };
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

export function MinuteKlinePanel({ scopeKey, code, currentDate, period, queryCurrentMinuteHistory, tickRefreshKey, currentMinuteOfDay = null }: MinuteKlinePanelProps) {
  const [live, setLive] = useState<{ scopeKey: string; response: CurrentMinuteHistoryResponse } | null>(null);
  const [liveLoading, setLiveLoading] = useState(false);
  const [liveError, setLiveError] = useState<string | null>(null);
  useEffect(() => {
    if (queryCurrentMinuteHistory === undefined || currentDate === null) {
      setLive(null); setLiveLoading(false); setLiveError(null);
      return;
    }
    let active = true;
    setLiveLoading(true); setLiveError(null);
    void queryCurrentMinuteHistory({ code }).then((response) => {
      if (!active) return;
      if (response.code !== code || response.live !== true || response.date !== currentDate) throw new Error("当前分钟响应证券、自然日错配或不是实时事实");
      setLive({ scopeKey, response });
    }).catch((failure: unknown) => {
      if (active) {
        setLive((previous) => previous?.scopeKey === scopeKey && previous.response.code === code && previous.response.date === currentDate ? previous : null);
        setLiveError(`当前分钟查询失败：${failure instanceof Error ? failure.message : String(failure)}；请重试并反馈错误详情。`);
      }
    }).finally(() => { if (active) setLiveLoading(false); });
    return () => { active = false; };
  }, [scopeKey, code, currentDate, queryCurrentMinuteHistory, tickRefreshKey]);
  const visibleLiveHistory = live?.scopeKey === scopeKey && live.response.code === code && live.response.date === currentDate ? live.response : null;
  const liveBars = useMemo(() => visibleLiveHistory === null ? [] : aggregateMinuteBars(visibleLiveHistory.date, visibleLiveHistory.bars, period), [visibleLiveHistory, period]);
  // 形成中的连续竞价桶以引擎墙钟为准，不从成交记录推测观察时刻。
  const liveActiveMinute = visibleLiveHistory?.phase === "Continuous" && currentMinuteOfDay !== null ? activeMinuteKlineBucketMinute(currentMinuteOfDay, period) : null;
  return <section className="minute-kline" aria-label="当前交易日分钟K线">
    <h3>{labels[period]}K线（当前交易日）</h3>
    <p>分钟成交仅在当前交易日可用，日结后仅保留日K。午休前后分别聚合，集合竞价单独成柱，无成交不补柱。</p>
    <section className="minute-kline-current" aria-label="当前交易日实时分钟K">
      <h4>当前交易日实时分钟K</h4>
      {queryCurrentMinuteHistory === undefined && <p>当前分钟历史宿主暂不可用。</p>}
      {liveLoading && <p role="status">正在读取当前真实分钟成交…</p>}
      {liveError !== null && <p role="alert">{liveError}</p>}
      {currentDate === null && <p>当前自然日尚未同步，暂不能查询活动分钟。</p>}
      {visibleLiveHistory !== null && <><p>{visibleLiveHistory.date}　{currentPhases[visibleLiveHistory.phase]}{currentMinuteOfDay !== null ? `　观察时刻 ${String(Math.floor(currentMinuteOfDay / 60)).padStart(2, "0")}:${String(currentMinuteOfDay % 60).padStart(2, "0")}` : ""}。柱仅来自已经发生的真实成交；{liveActiveMinute === null ? "当前没有形成中的连续竞价桶。" : "当前连续竞价桶标记为形成中。"}</p>{liveBars.length === 0 ? <p>当前尚无真实分钟成交，不生成K线柱。</p> : <MinuteKlineDay date={`${visibleLiveHistory.date}（实时）`} bars={liveBars} period={period} activeMinute={liveActiveMinute} />}</>}
    </section>
  </section>;
}
