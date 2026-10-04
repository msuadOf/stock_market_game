import type { KlinePoint, PricePoint } from "../components/PriceChart.tsx";
import { CALL_AUCTION_TICKS, TICKS_PER_TRADING_MINUTE, TOTAL_TICKS_PER_DAY } from "../config/defaults.ts";
import type { NormalizedTickFrame, ProtocolState } from "../host/protocol/index.ts";
import { candlesFromSnapshot, toChartCandle } from "../mobile/kline-sync.ts";
import { mergeMinutePoints, type AuctionPoint } from "../mobile/market-model.ts";
import type { Snapshot } from "../types/engine.ts";

const EMPTY_HISTORY: readonly never[] = Object.freeze([]);

/** 可重建的行情展示投影；协议、Redux 与存档的 authority 均留在原层。 */
export class MarketChartProjection {
  private pricesByCode: Record<string, readonly PricePoint[]> = {};
  private auctionsByCode: Record<string, readonly AuctionPoint[]> = {};
  private continuousVolumes: Record<string, number> = {};
  private completedDailyByCode: Record<string, readonly KlinePoint[]> = {};
  private activeDailyByCode: Record<string, KlinePoint> = {};
  private selectedCandlesByCode: Record<string, readonly KlinePoint[]> = {};
  private day: number | null = null;

  upsertFrames(frames: readonly NormalizedTickFrame[]): void {
    for (const frame of frames) {
      const day = Math.floor((frame.tick - 1) / TOTAL_TICKS_PER_DAY);
      if (this.day !== day) {
        this.pricesByCode = {};
        this.auctionsByCode = {};
        this.continuousVolumes = {};
        this.day = day;
      }
      for (const [code, points] of Object.entries(frame.auctionPoints)) {
        for (const point of points) {
          if (point.phase === "ClosingAuction") continue;
          if (point.kind === "Completion") this.continuousVolumes[code] = point.matched_volume;
          const history = this.auctionsByCode[code] ?? EMPTY_HISTORY;
          const value = point.indicative_price === null ? null : point.indicative_price / 100;
          const time = Math.floor(((point.tick - 1) % TOTAL_TICKS_PER_DAY) / (TICKS_PER_TRADING_MINUTE / 10));
          const previous = history.findLast((sample) => sample.time < time && sample.value !== null);
          const projected = Object.freeze({
            time,
            value, volume: point.matched_volume,
            buy: value !== null && (previous === undefined || previous.value === null || value >= previous.value),
          });
          if (!samePoint(history.find((sample) => sample.time === projected.time), projected)) {
            this.auctionsByCode[code] = freezeHistory(mergeMinutePoints(history, [projected]));
          }
        }
      }
      for (const [code, point] of Object.entries(frame.continuousPoints)) {
        const history = this.pricesByCode[code] ?? EMPTY_HISTORY;
        const time = Math.floor(((point.tick - 1) % TOTAL_TICKS_PER_DAY - CALL_AUCTION_TICKS) / TICKS_PER_TRADING_MINUTE);
        const previous = history.at(-1);
        const auctionPrice = this.auctionsByCode[code]?.findLast((sample) => sample.value !== null)?.value;
        const previousMinute = previous?.time === time ? history.at(-2) : previous;
        const previousPrice = previousMinute?.value ?? auctionPrice;
        const value = point.last_price / 100;
        const increment = Math.max(0, point.cumulative_volume - (this.continuousVolumes[code] ?? 0));
        const projected = Object.freeze({ time, value,
          volume: increment + (previous?.time === time ? previous.volume ?? 0 : 0),
          buy: previousPrice === null || previousPrice === undefined || value >= previousPrice,
        });
        if (!samePoint(previous, projected)) this.pricesByCode[code] = freezeHistory(mergeMinutePoints(history, [projected]));
        this.continuousVolumes[code] = point.cumulative_volume;
      }
    }
  }

  rebuildHistory(frames: readonly NormalizedTickFrame[]): void {
    this.pricesByCode = {};
    this.auctionsByCode = {};
    this.continuousVolumes = {};
    this.day = null;
    this.upsertFrames(frames);
  }

  replaceSnapshot(snapshot: Snapshot): void {
    const candles = candlesFromSnapshot(snapshot);
    const completed: Record<string, readonly KlinePoint[]> = {};
    for (const [code, history] of Object.entries(candles.completed)) {
      const previous = this.completedDailyByCode[code];
      completed[code] = previous !== undefined && previous.length === history.length
        && history.every((candle, index) => sameCandle(previous[index], candle))
        ? previous : freezeHistory(history.map(freezeCandle));
      if (completed[code] !== previous) delete this.selectedCandlesByCode[code];
    }
    for (const code of Object.keys(this.completedDailyByCode)) if (completed[code] === undefined) delete this.selectedCandlesByCode[code];
    this.completedDailyByCode = completed;
    this.replaceActiveCandles(snapshot.active_daily_candles);
  }

  replaceActiveCandles(candles: Snapshot["active_daily_candles"]): void {
    const next: Record<string, KlinePoint> = {};
    for (const [code, candle] of Object.entries(candles)) {
      const projected = toChartCandle(candle);
      const previous = this.activeDailyByCode[code];
      next[code] = previous !== undefined && sameCandle(previous, projected) ? previous : freezeCandle(projected);
      if (next[code] !== previous) delete this.selectedCandlesByCode[code];
    }
    for (const code of Object.keys(this.activeDailyByCode)) if (next[code] === undefined) delete this.selectedCandlesByCode[code];
    this.activeDailyByCode = next;
  }

  installBaseline(state: ProtocolState): void {
    this.rebuildHistory(state.intraday);
    this.replaceSnapshot(state.snapshot);
  }

  reset(snapshot: Snapshot): void {
    this.rebuildHistory([]);
    this.replaceSnapshot(snapshot);
  }

  pricePointsFor(code: string): readonly PricePoint[] { return this.pricesByCode[code] ?? EMPTY_HISTORY; }
  auctionPointsFor(code: string): readonly AuctionPoint[] { return this.auctionsByCode[code] ?? EMPTY_HISTORY; }
  candlesFor(code: string): readonly KlinePoint[] {
    const cached = this.selectedCandlesByCode[code];
    if (cached !== undefined) return cached;
    const completed = this.completedDailyByCode[code] ?? EMPTY_HISTORY;
    const active = this.activeDailyByCode[code];
    const selected = active === undefined ? completed : freezeHistory([...completed, active]);
    this.selectedCandlesByCode[code] = selected;
    return selected;
  }

  // 仅复制 code 索引；已冻结的点与数组复用，避免暴露内部可写 map。
  history(): Readonly<Record<string, readonly Readonly<PricePoint>[]>> { return Object.freeze({ ...this.pricesByCode }); }
  activeCandles(): Readonly<Record<string, Readonly<KlinePoint>>> { return Object.freeze({ ...this.activeDailyByCode }); }
}

function samePoint(previous: PricePoint | AuctionPoint | undefined, next: PricePoint | AuctionPoint): boolean {
  return previous !== undefined && previous.time === next.time && previous.value === next.value
    && previous.volume === next.volume && previous.buy === next.buy;
}

function sameCandle(previous: KlinePoint | undefined, next: KlinePoint): boolean {
  return previous !== undefined && previous.time === next.time && previous.open === next.open
    && previous.high === next.high && previous.low === next.low && previous.close === next.close
    && previous.volume === next.volume && previous.tradeStats?.turnoverCents === next.tradeStats?.turnoverCents
    && previous.tradeStats?.tradeCount === next.tradeStats?.tradeCount;
}

function freezeCandle(candle: KlinePoint): KlinePoint {
  if (candle.tradeStats !== undefined) Object.freeze(candle.tradeStats);
  return Object.freeze(candle);
}

function freezeHistory<T>(history: T[]): readonly T[] {
  return Object.freeze(history);
}
