import { useCallback, useRef, useState, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import type { AutoOrderManager } from "../components/auto-order-manager.ts";
import type { KlinePoint, PricePoint } from "../components/PriceChart.tsx";
import { CALL_AUCTION_TICKS, TICKS_PER_TRADING_MINUTE, TRADING_MINUTES_PER_DAY } from "../config/defaults.ts";
import type { NormalizedEngineUpdate, NormalizedTickFrame, ProtocolEffect, ProtocolReduction, ProtocolState } from "../host/protocol/index.ts";
import { candlesFromSnapshot } from "../mobile/kline-sync.ts";
import { mergeMinutePoints, type AuctionPoint } from "../mobile/market-model.ts";
import { appendTrades, applyProtocolFrame, setSnapshot, store } from "../store/store.ts";
import type { Snapshot } from "../types/engine.ts";

interface Options {
  readonly autoOrderManagerRef: MutableRefObject<AutoOrderManager | null>;
  readonly setNotice: Dispatch<SetStateAction<string | null>>;
}

function continuousPoint(code: string, frame: NormalizedTickFrame, previousVolume: number): PricePoint | null {
  const point = frame.continuousPoints[code];
  if (point === undefined) return null;
  return {
    time: Math.floor(((point.tick - 1) % (CALL_AUCTION_TICKS + TRADING_MINUTES_PER_DAY * TICKS_PER_TRADING_MINUTE) - CALL_AUCTION_TICKS) / TICKS_PER_TRADING_MINUTE),
    value: point.last_price / 100,
    volume: Math.max(0, point.cumulative_volume - previousVolume),
    buy: true,
  };
}

function auctionPoints(frame: NormalizedTickFrame): Readonly<Record<string, readonly AuctionPoint[]>> {
  return Object.fromEntries(Object.entries(frame.auctionPoints).map(([code, points]) => [code, points.filter((point) => point.phase !== "ClosingAuction").map((point) => ({
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
      if (point !== null) prices[code] = mergeMinutePoints(prices[code] ?? [], [point]);
      const continuous = frame.continuousPoints[code];
      if (continuous !== undefined) continuousVolumes[code] = continuous.cumulative_volume;
    }
    for (const [code, points] of Object.entries(auctionPoints(frame))) {
      auctions[code] = mergeMinutePoints(auctions[code] ?? [], [...points]);
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

export function useMarketChartRuntime({ autoOrderManagerRef, setNotice }: Options) {
  const [chartCode, setChartCode] = useState("600101");
  const priceHistoryByCodeRef = useRef<Record<string, PricePoint[]>>({});
  const auctionHistoryByCodeRef = useRef<Record<string, AuctionPoint[]>>({});
  const continuousVolumesRef = useRef<Record<string, number>>({});
  const [chartData, setChartData] = useState<PricePoint[]>([]);
  const [auctionChartData, setAuctionChartData] = useState<AuctionPoint[]>([]);
  const [dailyCandlesByCodeRef] = useState<{ current: Record<string, KlinePoint[]> }>(() => ({ current: {} }));
  const activeDailyCandlesRef = useRef<Record<string, KlinePoint>>({});
  const [dailyChartData, setDailyChartData] = useState<KlinePoint[]>([]);

  const chartCandlesFor = useCallback((code: string): KlinePoint[] => {
    const active = activeDailyCandlesRef.current[code];
    return active === undefined ? [...(dailyCandlesByCodeRef.current[code] ?? [])] : [...(dailyCandlesByCodeRef.current[code] ?? []), active];
  }, [dailyCandlesByCodeRef]);

  const replaceSnapshot = useCallback((snapshot: Snapshot) => {
    const candles = candlesFromSnapshot(snapshot);
    dailyCandlesByCodeRef.current = candles.completed;
    activeDailyCandlesRef.current = candles.active;
    store.dispatch(setSnapshot(snapshot));
    setDailyChartData(chartCandlesFor(chartCode));
  }, [chartCandlesFor, chartCode, dailyCandlesByCodeRef]);

  const applyEffects = useCallback((effects: readonly ProtocolEffect[]) => {
    for (const effect of effects) {
      switch (effect.kind) {
        case "notice":
          setNotice(effect.message);
          break;
        case "trade":
          store.dispatch(appendTrades([effect.event]));
          break;
        case "automatic-order":
          if (autoOrderManagerRef.current !== null) void autoOrderManagerRef.current.consumePoints(effect.points);
          break;
        case "civil-barrier":
          break;
        default:
          assertNever(effect);
      }
    }
  }, [autoOrderManagerRef, setNotice]);

  const acceptReduction = useCallback((reduction: Extract<ProtocolReduction, { kind: "applied" }>) => {
    const update: NormalizedEngineUpdate = reduction.update;
    if (update.kind === "civil-update") {
      const history = chartsFromHistory(update.intraday);
      priceHistoryByCodeRef.current = history.prices;
      auctionHistoryByCodeRef.current = history.auctions;
      continuousVolumesRef.current = history.volumes;
      replaceSnapshot(reduction.state.snapshot);
    } else {
      upsertFrames(update.frames, priceHistoryByCodeRef.current, auctionHistoryByCodeRef.current, continuousVolumesRef.current);
      if (update.runtimeSnapshot !== null) {
        replaceSnapshot(reduction.state.snapshot);
      }
      const finalFrame = update.frames.at(-1);
      if (finalFrame !== undefined && update.runtimeSnapshot === null) {
        store.dispatch(applyProtocolFrame({
          tick: finalFrame.tick,
          seq: finalFrame.seqTo,
          markets: finalFrame.markets,
          activeDailyCandles: finalFrame.activeDailyCandles,
        }));
      }
    }
    setChartData([...(priceHistoryByCodeRef.current[chartCode] ?? [])]);
    setAuctionChartData([...(auctionHistoryByCodeRef.current[chartCode] ?? [])]);
    setDailyChartData(chartCandlesFor(chartCode));
    applyEffects(reduction.effects);
  }, [applyEffects, chartCandlesFor, chartCode, replaceSnapshot]);

  const installBaseline = useCallback((state: ProtocolState) => {
    const history = chartsFromHistory(state.intraday);
    priceHistoryByCodeRef.current = history.prices;
    auctionHistoryByCodeRef.current = history.auctions;
    continuousVolumesRef.current = history.volumes;
    replaceSnapshot(state.snapshot);
    setChartData([...(history.prices[chartCode] ?? [])]);
    setAuctionChartData([...(history.auctions[chartCode] ?? [])]);
  }, [chartCode, replaceSnapshot]);

  const selectChart = useCallback((code: string) => {
    setChartCode(code);
    setChartData([...(priceHistoryByCodeRef.current[code] ?? [])]);
    setAuctionChartData([...(auctionHistoryByCodeRef.current[code] ?? [])]);
    setDailyChartData(chartCandlesFor(code));
  }, [chartCandlesFor]);

  const resetMarketHistory = useCallback((snapshot: Snapshot) => {
    priceHistoryByCodeRef.current = {};
    auctionHistoryByCodeRef.current = {};
    continuousVolumesRef.current = {};
    replaceSnapshot(snapshot);
    setChartData([]);
    setAuctionChartData([]);
  }, [replaceSnapshot]);

  const refreshDailyChart = useCallback(() => setDailyChartData(chartCandlesFor(chartCode)), [chartCandlesFor, chartCode]);

  return {
    chartCode,
    priceHistoryByCodeRef,
    chartData,
    auctionChartData,
    dailyChartData,
    activeDailyCandlesRef,
    acceptReduction,
    installBaseline,
    selectChart,
    resetMarketHistory,
    refreshDailyChart,
  };
}

function assertNever(value: never): never {
  throw new Error(`未处理协议效果：${String(value)}`);
}
