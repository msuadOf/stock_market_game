import type { EngineEvent, PriceLevel } from "../types/engine";
import type { KlinePoint, PricePoint } from "../components/PriceChart";
import { AUCTION_VOLUME_LINES_PER_MINUTE, CALL_AUCTION_ENTRY_MINUTES, CALL_AUCTION_TICKS, TOTAL_TICKS_PER_DAY, TRADING_MINUTES_PER_DAY } from "../config/defaults.ts";
import { formatSharesAsLots } from "../utils/format.ts";

export { AUCTION_VOLUME_LINES_PER_MINUTE, CALL_AUCTION_ENTRY_MINUTES } from "../config/defaults.ts";

export type MobileMarketView = "watchlist" | "holdings";
export const MOBILE_KLINE_DEFAULT_CAPACITY = 72;
export const MOBILE_KLINE_ZOOM_LEVELS = [120, 96, 72, 48, 30] as const;

export interface KlineViewport {
  capacity: number;
  offsetFromEnd: number;
}

export interface KlineWindow extends KlineViewport {
  start: number;
  end: number;
  maxOffset: number;
}

export type KlineViewportAction = "zoom-in" | "zoom-out" | "pan-left" | "pan-right" | "earliest" | "reset";

export interface CandleBodyPrices {
  top: number;
  bottom: number;
}

export interface CandleWickPrices {
  upper: { start: number; end: number };
  lower: { start: number; end: number };
}

/**
 * 蜡烛实体只连接当日开盘价和收盘价。昨收不参与实体边界，因而高开或低开时会保留跳空。
 */
export function candleBodyPrices(candle: Pick<KlinePoint, "open" | "close">): CandleBodyPrices {
  if (!Number.isFinite(candle.open) || candle.open < 0 || !Number.isFinite(candle.close) || candle.close < 0) {
    throw new RangeError(`invalid candle body: open=${candle.open}, close=${candle.close}`);
  }
  return {
    top: Math.max(candle.open, candle.close),
    bottom: Math.min(candle.open, candle.close),
  };
}

/**
 * 影线只连接最高/最低价与实体边界，不能贯穿空心上涨实体。
 */
export function candleWickPrices(candle: Pick<KlinePoint, "open" | "close" | "high" | "low">): CandleWickPrices {
  if (!Number.isFinite(candle.high) || candle.high < 0 || !Number.isFinite(candle.low) || candle.low < 0) {
    throw new RangeError(`invalid candle wick: high=${candle.high}, low=${candle.low}`);
  }
  const body = candleBodyPrices(candle);
  if (candle.high < body.top || candle.low > body.bottom) {
    throw new RangeError(`invalid candle range: high=${candle.high}, bodyTop=${body.top}, bodyBottom=${body.bottom}, low=${candle.low}`);
  }
  return {
    upper: { start: candle.high, end: body.top },
    lower: { start: body.bottom, end: candle.low },
  };
}

/** 用每个真实成交/价格点更新当日 OHLC，避免 tick 末价吞掉盘中极值和影线。 */
export function updateDailyCandle(existing: KlinePoint | undefined, time: KlinePoint["time"], price: number, volume: number): KlinePoint {
  if (!Number.isFinite(price) || price < 0 || !Number.isSafeInteger(volume) || volume < 0) {
    throw new RangeError(`invalid daily candle point: price=${price}, volume=${volume}`);
  }
  if (!existing) return { time, open: price, high: price, low: price, close: price, volume };
  return {
    ...existing,
    high: Math.max(existing.high, price),
    low: Math.min(existing.low, price),
    close: price,
    volume,
  };
}

export function klineWindow(total: number, capacity: number, offsetFromEnd: number): KlineWindow {
  if (!Number.isInteger(total) || total < 0 || !Number.isInteger(capacity) || capacity <= 0 || !Number.isInteger(offsetFromEnd) || offsetFromEnd < 0) {
    throw new RangeError(`invalid K-line window: total=${total}, capacity=${capacity}, offset=${offsetFromEnd}`);
  }
  const maxOffset = Math.max(0, total - capacity);
  const normalizedOffset = Math.min(offsetFromEnd, maxOffset);
  const end = total - normalizedOffset;
  return {
    start: Math.max(0, end - capacity),
    end,
    capacity,
    offsetFromEnd: normalizedOffset,
    maxOffset,
  };
}

export function reduceKlineViewport(viewport: KlineViewport, total: number, action: KlineViewportAction): KlineViewport {
  const zoomIndex = MOBILE_KLINE_ZOOM_LEVELS.indexOf(viewport.capacity as (typeof MOBILE_KLINE_ZOOM_LEVELS)[number]);
  if (zoomIndex < 0) throw new RangeError(`unsupported K-line zoom capacity: ${viewport.capacity}`);
  let capacity = viewport.capacity;
  let offsetFromEnd = viewport.offsetFromEnd;
  if (action === "zoom-in") capacity = MOBILE_KLINE_ZOOM_LEVELS[Math.min(zoomIndex + 1, MOBILE_KLINE_ZOOM_LEVELS.length - 1)];
  else if (action === "zoom-out") capacity = MOBILE_KLINE_ZOOM_LEVELS[Math.max(zoomIndex - 1, 0)];
  else if (action === "pan-left") offsetFromEnd += Math.max(1, Math.floor(capacity / 4));
  else if (action === "pan-right") offsetFromEnd = Math.max(0, offsetFromEnd - Math.max(1, Math.floor(capacity / 4)));
  else if (action === "earliest") offsetFromEnd = Math.max(0, total - capacity);
  else if (action === "reset") return { capacity: MOBILE_KLINE_DEFAULT_CAPACITY, offsetFromEnd: 0 };
  else action satisfies never;
  const normalized = klineWindow(total, capacity, offsetFromEnd);
  return { capacity: normalized.capacity, offsetFromEnd: normalized.offsetFromEnd };
}

export interface ChartSlotGeometry {
  center: number;
  markWidth: number;
}

/** 将每个交易日映射到共享横轴槽位，供 K 线、成交量与指标复用。 */
export function chartSlotGeometry(index: number, count: number, width = 390, capacity = count): ChartSlotGeometry {
  if (!Number.isInteger(index) || index < 0 || index >= count) {
    throw new RangeError(`chart slot index ${index} is outside 0..${count - 1}`);
  }
  if (!Number.isInteger(count) || count <= 0 || !Number.isFinite(width) || width <= 0) {
    throw new RangeError(`invalid chart slot geometry: count=${count}, width=${width}`);
  }
  if (!Number.isInteger(capacity) || capacity < count) {
    throw new RangeError(`chart slot capacity ${capacity} must cover ${count} points`);
  }
  const slotWidth = width / capacity;
  const defaultGap = width / MOBILE_KLINE_DEFAULT_CAPACITY * 0.3;
  const slotGap = Math.min(slotWidth * 0.3, defaultGap);
  return {
    center: (index + 0.5) * slotWidth,
    markWidth: capacity >= MOBILE_KLINE_DEFAULT_CAPACITY ? slotWidth * 0.7 : Math.min(slotWidth - slotGap, 10),
  };
}

/** A 股涨跌幅：当前价相对上一交易日收盘价。首日或异常零基准显示 0。 */
export function priceChangePercent(lastPrice: number, previousClose: number): number {
  return previousClose === 0 ? 0 : ((lastPrice - previousClose) / previousClose) * 100;
}

/** A 股逐笔成交按手展示；非整手数量保留两位精度，不能四舍五入成 0。 */
export function formatTradeLots(shares: number, lotSize = 100): string {
  return formatSharesAsLots(shares, lotSize);
}

export interface SymmetricIntradayScale {
  top: number;
  bottom: number;
  topPercent: number;
  bottomPercent: number;
}

/**
 * 详情分时图以昨收为 0% 中轴，上下使用同一价格距离。
 * 集合竞价和连续竞价的全部有效价格都必须传入，任何一侧的极值都会同步扩展另一侧。
 */
export function symmetricIntradayScale(
  values: readonly number[],
  previousClose: number,
  paddingRatio = 0.04,
): SymmetricIntradayScale {
  if (!Number.isFinite(previousClose) || previousClose <= 0) {
    throw new RangeError(`分时图昨收必须是有限正数，收到 ${String(previousClose)}`);
  }
  if (!Number.isFinite(paddingRatio) || paddingRatio < 0) {
    throw new RangeError(`分时图留白比例必须是非负有限数，收到 ${String(paddingRatio)}`);
  }
  for (const value of values) {
    if (!Number.isFinite(value) || value <= 0) {
      throw new RangeError(`分时图价格必须是有限正数，收到 ${String(value)}`);
    }
  }
  const largestDeviation = values.reduce(
    (largest, value) => Math.max(largest, Math.abs(value - previousClose)),
    0,
  );
  const distance = Math.max(largestDeviation, previousClose * 0.006) * (1 + paddingRatio);
  const percent = distance / previousClose * 100;
  return {
    top: previousClose + distance,
    bottom: previousClose - distance,
    topPercent: percent,
    bottomPercent: -percent,
  };
}

const AUCTION_AXIS_END = 16;
/** 固定横轴：集合竞价占左侧 16%，连续竞价压缩午休并等距铺满剩余区域。 */
export function intradayChartX(point: { phase: "auction" | "continuous"; minute: number }): number {
  if (!Number.isSafeInteger(point.minute)) {
    throw new RangeError(`分时槽位必须是安全整数，收到 ${String(point.minute)}`);
  }
  if (point.phase === "auction") {
    const auctionSlotCount = CALL_AUCTION_ENTRY_MINUTES * AUCTION_VOLUME_LINES_PER_MINUTE;
    if (point.minute < 0 || point.minute >= auctionSlotCount) {
      throw new RangeError(`集合竞价细线槽必须在 0-${auctionSlotCount - 1}，收到 ${point.minute}`);
    }
    return point.minute / (auctionSlotCount - 1) * AUCTION_AXIS_END;
  }
  if (point.minute < 0 || point.minute >= TRADING_MINUTES_PER_DAY) {
    throw new RangeError(`连续竞价分钟必须在 0-${TRADING_MINUTES_PER_DAY - 1}，收到 ${point.minute}`);
  }
  return AUCTION_AXIS_END + point.minute / (TRADING_MINUTES_PER_DAY - 1) * (100 - AUCTION_AXIS_END);
}

/** 同一买卖方向内按最大挂单量归一；极小非零挂单保留 2% 可见色块。 */
export function orderBookDepthPercent(quantity: number, sideMaximum: number): number {
  if (!Number.isFinite(quantity) || quantity < 0) {
    throw new RangeError(`盘口挂单量必须是非负有限数，收到 ${String(quantity)}`);
  }
  if (!Number.isFinite(sideMaximum) || sideMaximum <= 0) {
    throw new RangeError(`盘口最大挂单量必须是正有限数，收到 ${String(sideMaximum)}`);
  }
  if (quantity === 0) return 0;
  return Math.min(100, Math.max(2, quantity / sideMaximum * 100));
}

export interface IntradayVolumeScale {
  auctionMax: number;
  continuousMax: number;
}

/** 竞价时间槽始终存在；没有可成交指示价时仅 price 为空，真实累计量仍保留。 */
export interface AuctionPoint {
  time: number;
  value: number | null;
  volume: number;
  buy: boolean;
}

/**
 * 集合竞价量是累计值，连续竞价量是每分钟增量，二者不能共用绝对最大值。
 * 两个阶段分别以当前已有最高柱为 100%，随真实数据自适应各自的纵轴。
 */
export function intradayVolumeScale(
  auctionVolumes: readonly number[],
  continuousVolumes: readonly number[],
): IntradayVolumeScale {
  const assertVolumes = (phase: string, volumes: readonly number[]) => {
    for (const volume of volumes) {
      if (!Number.isFinite(volume) || volume < 0) {
        throw new RangeError(`${phase}分时量必须是非负有限数，收到 ${String(volume)}`);
      }
    }
  };
  assertVolumes("集合竞价", auctionVolumes);
  assertVolumes("连续竞价", continuousVolumes);
  return {
    auctionMax: Math.max(1, ...auctionVolumes),
    continuousMax: Math.max(1, ...continuousVolumes),
  };
}

/**
 * 以引擎快照为行情真源；默认配置仅决定熟悉股票的显示顺序。
 * 引擎运行时新增的证券会稳定追加，而不是被静态 STOCK_LIST 丢弃。
 */
export function marketCodesForView(
  marketCodes: readonly string[],
  preferredOrder: readonly string[],
  view: MobileMarketView,
  heldCodes: ReadonlySet<string>,
): string[] {
  const available = new Set(marketCodes);
  const preferred = preferredOrder.filter((code) => available.delete(code));
  const ordered = [...preferred, ...Array.from(available).sort()];
  return view === "holdings" ? ordered.filter((code) => heldCodes.has(code)) : ordered;
}

function sparklineSlotX(time: number, width: number): number {
  if (!Number.isSafeInteger(time) || time < 0 || time >= TRADING_MINUTES_PER_DAY) {
    throw new RangeError(`迷你走势图分钟槽位必须在 0-${TRADING_MINUTES_PER_DAY - 1}，收到 ${String(time)}`);
  }
  return Number((time * width / (TRADING_MINUTES_PER_DAY - 1)).toFixed(2));
}

/** 把真实分钟价格放入全天固定槽位；不足两个点时不伪造曲线。 */
export function sparklinePoints(points: readonly PricePoint[], width = 64, height = 48): string {
  if (points.length < 2) return "";
  const values = points.map((point) => point.value);
  const min = Math.min(...values);
  const max = Math.max(...values);
  const range = max - min;
  return points.map((point) => {
    const x = sparklineSlotX(point.time, width);
    const y = range === 0 ? height / 2 : (max - point.value) / range * height;
    return `${x},${Number(y.toFixed(2))}`;
  }).join(" ");
}

export interface SparklineGeometry {
  linePoints: string;
  areaPoints: string;
  axisY: number;
}

/**
 * 以昨收为真实 0% 轴生成自适应坐标域。
 * 坐标域包含昨收和全部价格，并在上下各留 12% 呼吸空间；因此单边行情的零轴会靠近边缘而非强制居中。
 */
export function sparklineGeometry(points: readonly PricePoint[], baseline: number, width = 64, height = 48): SparklineGeometry {
  if (!Number.isFinite(baseline)) throw new RangeError("迷你走势图基准价必须是有限数值");
  if (!Number.isFinite(width) || width <= 0 || !Number.isFinite(height) || height <= 0) {
    throw new RangeError("迷你走势图尺寸必须是有限正数");
  }
  if (points.length < 2) return { linePoints: "", areaPoints: "", axisY: height / 2 };

  const values = points.map((point) => point.value);
  const rawMin = Math.min(baseline, ...values);
  const rawMax = Math.max(baseline, ...values);
  const rawRange = rawMax - rawMin;
  if (rawRange === 0) {
    const axisY = Number((height / 2).toFixed(2));
    const pointCoordinates = points.map((point) => ({ x: sparklineSlotX(point.time, width), y: axisY }));
    const linePoints = pointCoordinates.map(({ x, y }) => `${x},${y}`).join(" ");
    const firstX = pointCoordinates[0].x;
    const lastX = pointCoordinates.at(-1)!.x;
    return { linePoints, areaPoints: `${firstX},${axisY} ${linePoints} ${lastX},${axisY}`, axisY };
  }

  const padding = rawRange * 0.12;
  const min = rawMin - padding;
  const max = rawMax + padding;
  const range = max - min;
  const y = (value: number) => Number((((max - value) / range) * height).toFixed(2));
  const axisY = y(baseline);
  const pointCoordinates = points.map((point) => ({ x: sparklineSlotX(point.time, width), y: y(point.value) }));
  const linePoints = pointCoordinates.map(({ x, y: pointY }) => `${x},${pointY}`).join(" ");
  const firstX = pointCoordinates[0].x;
  const lastX = pointCoordinates.at(-1)!.x;
  return { linePoints, areaPoints: `${firstX},${axisY} ${linePoints} ${lastX},${axisY}`, axisY };
}

/** 按游戏交易日聚合 OHLCV；一周 5 日、一月 20 日。 */
export function aggregateCandles(candles: readonly KlinePoint[], period: "日K" | "周K" | "月K"): KlinePoint[] {
  const daysPerCandle = period === "日K" ? 1 : period === "周K" ? 5 : 20;
  if (daysPerCandle === 1) return candles.map((candle) => ({ ...candle }));

  const groups: KlinePoint[][] = [];
  for (let index = 0; index < candles.length; index += daysPerCandle) {
    groups.push(candles.slice(index, index + daysPerCandle));
  }
  return groups.map((group) => ({
    time: group[0].time,
    open: group[0].open,
    high: Math.max(...group.map((candle) => candle.high)),
    low: Math.min(...group.map((candle) => candle.low)),
    close: group.at(-1)!.close,
    volume: group.reduce((sum, candle) => sum + (candle.volume ?? 0), 0),
  }));
}

/** 标准 KDJ(9,3,3)：K/D 使用递推平滑，初值均为 50。 */
export function calculateKdj(candles: readonly KlinePoint[]): { k: number[]; d: number[]; j: number[] } {
  const k: number[] = [];
  const d: number[] = [];
  const j: number[] = [];
  let previousK = 50;
  let previousD = 50;
  candles.forEach((candle, index) => {
    const window = candles.slice(Math.max(0, index - 8), index + 1);
    const high = Math.max(...window.map((item) => item.high));
    const low = Math.min(...window.map((item) => item.low));
    const rsv = high === low ? 50 : (candle.close - low) / (high - low) * 100;
    previousK = previousK * 2 / 3 + rsv / 3;
    previousD = previousD * 2 / 3 + previousK / 3;
    k.push(previousK);
    d.push(previousD);
    j.push(previousK * 3 - previousD * 2);
  });
  return { k, d, j };
}

/** 一个游戏世界分钟包含的 tick 数：每 tick = 游戏世界 1 秒。 */
export const TICKS_PER_TRADING_MINUTE = 60;

export interface BookSlot {
  label: string;
  level: PriceLevel | null;
}

/**
 * 把引擎按最优价优先提供的盘口深度映射成固定五档视图。
 * 空档只保留占位，不推算或伪造成交价格。
 */
export function buildFiveLevelBook(bids: PriceLevel[], asks: PriceLevel[], depth = 5): { sells: BookSlot[]; buys: BookSlot[] } {
  if (!Number.isSafeInteger(depth) || depth <= 0) throw new RangeError("盘口档位数必须是正整数");
  return {
    sells: Array.from({ length: depth }, (_, index) => {
      const rank = depth - index;
      return { label: `卖${rank}`, level: asks[rank - 1] ?? null };
    }),
    buys: Array.from({ length: depth }, (_, index) => ({ label: `买${index + 1}`, level: bids[index] ?? null })),
  };
}

/**
 * 将单只股票的逐秒事件累积成一分钟分时点。
 *
 * Worker 会分批推送事件，因此聚合状态必须跨批次保存；到第 60 个 PriceTick
 * 才生成一个价格点和对应的一分钟成交量柱。
 */
export class MinutePointCollector {
  private currentDay: number | null = null;
  private currentMinute: number | null = null;
  private minuteOpeningVolume = 0;
  private lastDailyVolume = 0;
  private lastTradePrice: number | null = null;
  private previousMinutePrice: number | null = null;
  private currentMinutePrice: number | null = null;
  private readonly code: string;
  private readonly ticksPerMinute: number;
  private readonly ticksPerDay: number;
  private readonly auctionTicks: number;

  constructor(code: string, ticksPerMinute = TICKS_PER_TRADING_MINUTE, ticksPerDay = 14_400, auctionTicks = 0) {
    if (!Number.isSafeInteger(ticksPerMinute) || ticksPerMinute <= 0) {
      throw new RangeError("ticksPerMinute 必须是正整数");
    }
    if (!Number.isSafeInteger(ticksPerDay) || ticksPerDay <= 0) {
      throw new RangeError("ticksPerDay 必须是正整数");
    }
    if (!Number.isSafeInteger(auctionTicks) || auctionTicks < 0 || auctionTicks >= ticksPerDay) {
      throw new RangeError("auctionTicks 必须是小于 ticksPerDay 的非负整数");
    }
    this.code = code;
    this.ticksPerMinute = ticksPerMinute;
    this.ticksPerDay = ticksPerDay;
    this.auctionTicks = auctionTicks;
  }

  reset(): void {
    this.currentDay = null;
    this.currentMinute = null;
    this.minuteOpeningVolume = 0;
    this.lastDailyVolume = 0;
    this.lastTradePrice = null;
    this.previousMinutePrice = null;
    this.currentMinutePrice = null;
  }

  collect(events: EngineEvent[]): PricePoint[] {
    const byMinute = new Map<number, PricePoint>();

    for (const event of events) {
      if ("AuctionCompleted" in event && event.AuctionCompleted.code === this.code) {
        const auction = event.AuctionCompleted;
        if (!Number.isSafeInteger(auction.tick) || auction.tick <= 0) {
          throw new RangeError(`AuctionCompleted.tick 必须是正整数，股票 ${this.code} 收到 ${String(auction.tick)}`);
        }
        if (!Number.isSafeInteger(auction.matched_volume) || auction.matched_volume < 0) {
          throw new RangeError(`集合竞价成交量必须是非负安全整数，股票 ${this.code} 收到 ${String(auction.matched_volume)}`);
        }
        const auctionDay = Math.floor((auction.tick - 1) / this.ticksPerDay);
        if (this.currentDay !== auctionDay) this.reset();
        this.currentDay = auctionDay;
        this.currentMinute = null;
        this.minuteOpeningVolume = auction.matched_volume;
        this.lastDailyVolume = auction.matched_volume;
        this.currentMinutePrice = auction.opening_price === null ? null : auction.opening_price / 100;
        this.lastTradePrice = null;
        continue;
      }
      if ("Trade" in event && event.Trade.code === this.code) {
        this.lastTradePrice = event.Trade.price / 100;
        continue;
      }
      if (!("PriceTick" in event) || event.PriceTick.code !== this.code) continue;

      if (!Number.isSafeInteger(event.PriceTick.tick) || event.PriceTick.tick <= 0) {
        throw new RangeError(`PriceTick.tick 必须是正整数，股票 ${this.code} 收到 ${String(event.PriceTick.tick)}`);
      }
      const absoluteTick = event.PriceTick.tick;
      const day = Math.floor((absoluteTick - 1) / this.ticksPerDay);
      const dayTick = (absoluteTick - 1) % this.ticksPerDay;
      if (dayTick < this.auctionTicks) {
        throw new RangeError(`集合竞价阶段不应收到 PriceTick，股票 ${this.code} tick=${absoluteTick}`);
      }
      const minute = Math.floor((dayTick - this.auctionTicks) / this.ticksPerMinute);
      const dailyVolume = event.PriceTick.daily_candle.volume ?? 0;
      const value = event.PriceTick.last_price / 100;
      const directionPrice = this.lastTradePrice ?? value;

      if (this.currentDay !== day) {
        this.reset();
        this.currentDay = day;
        this.currentMinute = minute;
        // 若在日中首次收到压缩采样，之前的累计量不能冒充当前分钟量。
        this.minuteOpeningVolume = minute === 0 ? 0 : dailyVolume;
      } else if (this.currentMinute !== minute) {
        this.previousMinutePrice = this.currentMinutePrice ?? this.previousMinutePrice;
        this.currentMinute = minute;
        this.minuteOpeningVolume = this.lastDailyVolume;
      }

      byMinute.set(minute, {
        time: minute,
        value,
        volume: Math.max(0, dailyVolume - this.minuteOpeningVolume),
        buy: this.previousMinutePrice === null ? dailyVolume > this.minuteOpeningVolume : directionPrice >= this.previousMinutePrice,
      });
      this.lastDailyVolume = dailyVolume;
      this.currentMinutePrice = value;
      this.lastTradePrice = null;
    }

    return [...byMinute.values()];
  }
}

/** 将引擎权威的集合竞价指示价和累计量聚合到固定的 6 秒细线槽。 */
export class AuctionPointCollector {
  private currentDay: number | null = null;
  private previousPrice: number | null = null;
  private readonly code: string;
  private readonly ticksPerMinute: number;
  private readonly ticksPerDay: number;
  private readonly auctionTicks: number;

  constructor(
    code: string,
    ticksPerMinute = TICKS_PER_TRADING_MINUTE,
    ticksPerDay = TOTAL_TICKS_PER_DAY,
    auctionTicks = CALL_AUCTION_TICKS,
  ) {
    if (!Number.isSafeInteger(ticksPerMinute) || ticksPerMinute <= 0) throw new RangeError("ticksPerMinute 必须是正整数");
    if (!Number.isSafeInteger(ticksPerDay) || ticksPerDay <= 0) throw new RangeError("ticksPerDay 必须是正整数");
    if (!Number.isSafeInteger(auctionTicks) || auctionTicks <= 0 || auctionTicks >= ticksPerDay) {
      throw new RangeError("auctionTicks 必须是小于 ticksPerDay 的正整数");
    }
    this.code = code;
    this.ticksPerMinute = ticksPerMinute;
    this.ticksPerDay = ticksPerDay;
    this.auctionTicks = auctionTicks;
  }

  reset(): void {
    this.currentDay = null;
    this.previousPrice = null;
  }

  collect(events: EngineEvent[]): AuctionPoint[] {
    const byMinute = new Map<number, AuctionPoint>();
    for (const event of events) {
      const data = "AuctionTick" in event
        ? event.AuctionTick
        : "AuctionCompleted" in event
          ? { ...event.AuctionCompleted, indicative_price: event.AuctionCompleted.opening_price }
          : null;
      if (!data || data.code !== this.code) continue;
      if (!Number.isSafeInteger(data.tick) || data.tick <= 0) {
        throw new RangeError(`竞价事件 tick 必须是正整数，股票 ${this.code} 收到 ${String(data.tick)}`);
      }
      const day = Math.floor((data.tick - 1) / this.ticksPerDay);
      const dayTick = (data.tick - 1) % this.ticksPerDay;
      const auctionEntryTicks = this.auctionTicks - Math.floor(this.auctionTicks / 3);
      if (dayTick >= auctionEntryTicks) {
        throw new RangeError(`竞价事件超出开盘集合竞价阶段，股票 ${this.code} tick=${data.tick}`);
      }
      if (this.currentDay !== day) {
        this.currentDay = day;
        this.previousPrice = null;
      }
      const value = data.indicative_price === null ? null : data.indicative_price / 100;
      const ticksPerLine = this.ticksPerMinute / AUCTION_VOLUME_LINES_PER_MINUTE;
      const lineSlot = Math.min(Math.ceil(this.auctionTicks / ticksPerLine) - 1, Math.floor(dayTick / ticksPerLine));
      byMinute.set(lineSlot, {
        time: lineSlot,
        value,
        volume: data.matched_volume,
        buy: value === null ? false : this.previousPrice === null || value >= this.previousPrice,
      });
      if (value !== null) this.previousPrice = value;
    }
    return [...byMinute.values()].sort((left, right) => left.time - right.time);
  }
}

/** 将同一分钟的实时更新原位替换，避免一秒一个点把横轴挤满。 */
export function mergeMinutePoints<T extends { time: number }>(history: T[], incoming: T[]): T[] {
  const merged = new Map(history.map((point) => [point.time, point]));
  for (const point of incoming) merged.set(point.time, point);
  return [...merged.values()].sort((left, right) => left.time - right.time);
}

/** 批次跨过多个日界时，分时只消费最后一个日界之后的当前交易日事件。 */
export function currentTradingDayEvents(events: EngineEvent[]): EngineEvent[] {
  let boundary = -1;
  events.forEach((event, index) => { if ("DayBoundary" in event) boundary = index; });
  return boundary < 0 ? events : events.slice(boundary + 1);
}

export function tradingDayProgress(elapsedMinutes: number, totalMinutes: number): number {
  if (!Number.isFinite(totalMinutes) || totalMinutes <= 0) {
    throw new RangeError("totalMinutes 必须是正数");
  }
  return Math.min(1, Math.max(0, elapsedMinutes / totalMinutes));
}

/** A 股连续竞价时间；120 分钟后跳过午间休市。 */
export function formatTradingMinute(minute: number): string {
  const safeMinute = Math.min(239, Math.max(0, Math.floor(minute)));
  const total = safeMinute < 120 ? 9 * 60 + 30 + safeMinute : 13 * 60 + safeMinute - 120;
  const hours = Math.floor(total / 60);
  const minutes = total % 60;
  return `${String(hours).padStart(2, "0")}:${String(minutes).padStart(2, "0")}`;
}

/** 将引擎的权威世界 tick 换算为 A 股交易时钟；每 tick 为一秒并跳过午间休市。 */
export function formatGameClock(tick: number): string {
  if (!Number.isSafeInteger(tick) || tick < 0) {
    throw new RangeError(`游戏 tick 必须是非负安全整数，收到 ${String(tick)}`);
  }
  const secondOfDay = tick % TOTAL_TICKS_PER_DAY;
  const continuousSecond = secondOfDay - CALL_AUCTION_TICKS;
  const secondsFromMidnight = secondOfDay < CALL_AUCTION_TICKS
    ? 9 * 3_600 + 15 * 60 + secondOfDay
    : continuousSecond < 7_200
      ? 9 * 3_600 + 30 * 60 + continuousSecond
      : 13 * 3_600 + continuousSecond - 7_200;
  const hours = Math.floor(secondsFromMidnight / 3_600);
  const minutes = Math.floor((secondsFromMidnight % 3_600) / 60);
  const seconds = secondsFromMidnight % 60;
  return [hours, minutes, seconds].map((value) => String(value).padStart(2, "0")).join(":");
}
