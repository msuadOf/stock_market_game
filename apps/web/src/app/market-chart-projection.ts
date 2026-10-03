import type { KlinePoint, PricePoint } from "../components/PriceChart.tsx";
import { CALL_AUCTION_TICKS, TICKS_PER_TRADING_MINUTE, TRADING_MINUTES_PER_DAY } from "../config/defaults.ts";
import type { NormalizedTickFrame, ProtocolState } from "../host/protocol/index.ts";
import { candlesFromSnapshot, toChartCandle } from "../mobile/kline-sync.ts";
import { mergeMinutePoints, type AuctionPoint } from "../mobile/market-model.ts";
import type { Snapshot } from "../types/engine.ts";

function continuousPoint(code: string, frame: NormalizedTickFrame, previousVolume: number): PricePoint | null {
  const point = frame.continuousPoints[code];
  if (point === undefined) return null;
  return Object.freeze({
    time: Math.floor(((point.tick - 1) % (CALL_AUCTION_TICKS + TRADING_MINUTES_PER_DAY * TICKS_PER_TRADING_MINUTE) - CALL_AUCTION_TICKS) / TICKS_PER_TRADING_MINUTE),
    value: point.last_price / 100,
    volume: Math.max(0, point.cumulative_volume - previousVolume),
    buy: true,
  });
}

function auctionPoints(frame: NormalizedTickFrame): Readonly<Record<string, readonly AuctionPoint[]>> {
  return Object.fromEntries(Object.entries(frame.auctionPoints).map(([code, points]) => [code, points.filter((point) => point.phase !== "ClosingAuction").map((point) => Object.freeze({
    time: Math.floor((((point.tick - 1) % (CALL_AUCTION_TICKS + 240 * TICKS_PER_TRADING_MINUTE)) / (TICKS_PER_TRADING_MINUTE / 10))),
    value: point.indicative_price === null ? null : point.indicative_price / 100,
    volume: point.matched_volume,
    buy: point.indicative_price !== null,
  }))]));
}

function upsertFrames(
  frames: readonly NormalizedTickFrame[],
  prices: Record<string, PricePoint[]>,
  auctions: Record<string, AuctionPoint[]>,
  continuousVolumes: Record<string, number>,
): void {
  for (const frame of frames) {
    for (const code of Object.keys(frame.continuousPoints)) {
      const point = continuousPoint(code, frame, continuousVolumes[code] ?? 0);
      if (point !== null) prices[code] = freezeHistory(mergeMinutePoints(prices[code] ?? [], [point]));
      const continuous = frame.continuousPoints[code];
      if (continuous !== undefined) continuousVolumes[code] = continuous.cumulative_volume;
    }
    for (const [code, points] of Object.entries(auctionPoints(frame))) {
      auctions[code] = freezeHistory(mergeMinutePoints(auctions[code] ?? [], [...points]));
    }
  }
}

function chartsFromHistory(frames: readonly NormalizedTickFrame[]): { readonly prices: Record<string, PricePoint[]>; readonly auctions: Record<string, AuctionPoint[]>; readonly volumes: Record<string, number> } {
  const prices: Record<string, PricePoint[]> = {};
  const auctions: Record<string, AuctionPoint[]> = {};
  const volumes: Record<string, number> = {};
  upsertFrames(frames, prices, auctions, volumes);
  return { prices, auctions, volumes };
}

/** 可重建的行情展示投影；协议、Redux 与存档的 authority 均留在原层。 */
export class MarketChartProjection {
  private pricesByCode: Record<string, PricePoint[]> = {};
  private auctionsByCode: Record<string, AuctionPoint[]> = {};
  private continuousVolumes: Record<string, number> = {};
  private completedDailyByCode: Record<string, KlinePoint[]> = {};
  private activeDailyByCode: Record<string, KlinePoint> = {};

  upsertFrames(frames: readonly NormalizedTickFrame[]): void {
    upsertFrames(frames, this.pricesByCode, this.auctionsByCode, this.continuousVolumes);
  }

  rebuildHistory(frames: readonly NormalizedTickFrame[]): void {
    const history = chartsFromHistory(frames);
    this.pricesByCode = history.prices;
    this.auctionsByCode = history.auctions;
    this.continuousVolumes = history.volumes;
  }

  replaceSnapshot(snapshot: Snapshot): void {
    const candles = candlesFromSnapshot(snapshot);
    this.completedDailyByCode = Object.fromEntries(Object.entries(candles.completed).map(([code, history]) => [code, freezeHistory(history.map(freezeCandle))]));
    this.activeDailyByCode = Object.fromEntries(Object.entries(candles.active).map(([code, candle]) => [code, freezeCandle(candle)]));
  }

  replaceActiveCandles(candles: Snapshot["active_daily_candles"]): void {
    this.activeDailyByCode = Object.fromEntries(Object.entries(candles).map(([code, candle]) => [code, freezeCandle(toChartCandle(candle))]));
  }

  installBaseline(state: ProtocolState): void {
    this.rebuildHistory(state.intraday);
    this.replaceSnapshot(state.snapshot);
  }

  reset(snapshot: Snapshot): void {
    this.rebuildHistory([]);
    this.replaceSnapshot(snapshot);
  }

  pricePointsFor(code: string): PricePoint[] { return [...(this.pricesByCode[code] ?? [])]; }
  auctionPointsFor(code: string): AuctionPoint[] { return [...(this.auctionsByCode[code] ?? [])]; }
  candlesFor(code: string): KlinePoint[] {
    const completed = this.completedDailyByCode[code] ?? [];
    const active = this.activeDailyByCode[code];
    return active === undefined ? [...completed] : [...completed, active];
  }

  // 仅复制 code 索引；已冻结的点与数组复用，避免暴露内部可写 map。
  history(): Readonly<Record<string, readonly Readonly<PricePoint>[]>> { return Object.freeze({ ...this.pricesByCode }); }
  activeCandles(): Readonly<Record<string, Readonly<KlinePoint>>> { return Object.freeze({ ...this.activeDailyByCode }); }
}

function freezeCandle(candle: KlinePoint): KlinePoint {
  if (candle.tradeStats !== undefined) Object.freeze(candle.tradeStats);
  return Object.freeze(candle);
}

function freezeHistory<T>(history: T[]): T[] {
  Object.freeze(history);
  return history;
}
