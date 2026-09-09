import { useCallback, useRef, useState, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import type { AutoOrderManager } from "../components/auto-order-manager";
import type { KlinePoint, PricePoint } from "../components/PriceChart";
import {
  AUCTION_VOLUME_LINES_PER_MINUTE,
  CALL_AUCTION_ENTRY_MINUTES,
  DEFAULT_SETUP,
  STOCK_LIST,
  TRADING_MINUTES_PER_DAY,
} from "../config/defaults";
import { candlesFromSnapshot, reduceCandleEvents, toChartCandle } from "../mobile/kline-sync";
import {
  AuctionPointCollector,
  currentTradingDayEvents,
  mergeMinutePoints,
  MinutePointCollector,
  type AuctionPoint,
} from "../mobile/market-model";
import { appendTrades, applyEvents, setSnapshot, store } from "../store/store";
import type { EngineEvent, IntentRejectedEvent, SettlementErrorEvent, Snapshot, TradeEvent } from "../types/engine";
import { rejectionText } from "../utils/format";

const MAX_DAILY_CANDLES = 360;

interface UseMarketChartRuntimeOptions {
  autoOrderManagerRef: MutableRefObject<AutoOrderManager | null>;
  setNotice: Dispatch<SetStateAction<string | null>>;
}

/** Turns batched engine events into chart-only presentation state. */
export function useMarketChartRuntime({ autoOrderManagerRef, setNotice }: UseMarketChartRuntimeOptions) {
  const [chartCode, setChartCode] = useState<string>(STOCK_LIST[0].code);
  const priceHistoryByCodeRef = useRef<Record<string, PricePoint[]>>({});
  const minuteCollectorsRef = useRef<Record<string, MinutePointCollector>>({});
  const [chartData, setChartData] = useState<PricePoint[]>([]);
  const auctionHistoryByCodeRef = useRef<Record<string, AuctionPoint[]>>({});
  const auctionCollectorsRef = useRef<Record<string, AuctionPointCollector>>({});
  const [auctionChartData, setAuctionChartData] = useState<AuctionPoint[]>([]);
  const [dailyCandlesByCodeRef] = useState<{ current: Record<string, KlinePoint[]> }>(() => ({ current: {} }));
  const activeDailyCandlesRef = useRef<Record<string, KlinePoint>>({});
  const hasSyncedDailyCandlesRef = useRef(false);
  const [dailyChartData, setDailyChartData] = useState<KlinePoint[]>([]);

  const chartCandlesFor = useCallback((code: string): KlinePoint[] => {
    const completed = dailyCandlesByCodeRef.current[code] ?? [];
    const active = activeDailyCandlesRef.current[code];
    return active ? [...completed, active] : [...completed];
  }, [dailyCandlesByCodeRef]);

  const syncDailyCandleSnapshot = useCallback((nextSnapshot: Snapshot) => {
    const synced = candlesFromSnapshot(nextSnapshot);
    dailyCandlesByCodeRef.current = synced.completed;
    activeDailyCandlesRef.current = synced.active;
    hasSyncedDailyCandlesRef.current = true;
    setDailyChartData(chartCandlesFor(chartCode));
  }, [chartCandlesFor, chartCode, dailyCandlesByCodeRef]);

  const syncActiveDailyCandleSnapshot = useCallback((nextSnapshot: Snapshot) => {
    activeDailyCandlesRef.current = Object.fromEntries(
      Object.entries(nextSnapshot.active_daily_candles).map(([code, candle]) => [
        code,
        toChartCandle(candle),
      ]),
    );
    setDailyChartData(chartCandlesFor(chartCode));
  }, [chartCandlesFor, chartCode]);

  const acceptRuntimeSnapshot = useCallback((nextSnapshot: Snapshot) => {
    if (!hasSyncedDailyCandlesRef.current && Object.keys(nextSnapshot.daily_candles).length > 0) {
      syncDailyCandleSnapshot(nextSnapshot);
    } else {
      syncActiveDailyCandleSnapshot(nextSnapshot);
    }
    store.dispatch(setSnapshot(nextSnapshot));
  }, [syncActiveDailyCandleSnapshot, syncDailyCandleSnapshot]);

  const onEventsRef = useRef<(events: EngineEvent[]) => void>(() => {});
  onEventsRef.current = (events) => {
    const fills: TradeEvent[] = [];
    const dayChanged = events.some((event) => "DayBoundary" in event);
    const intradayEvents = currentTradingDayEvents(events);
    if (dayChanged) {
      priceHistoryByCodeRef.current = {};
      minuteCollectorsRef.current = {};
      auctionHistoryByCodeRef.current = {};
      auctionCollectorsRef.current = {};
    }

    const eventCodes = intradayEvents.flatMap((event) => "PriceTick" in event
      ? [event.PriceTick.code]
      : "AuctionTick" in event
        ? [event.AuctionTick.code]
        : "AuctionCompleted" in event
          ? [event.AuctionCompleted.code]
          : "Trade" in event
            ? [event.Trade.code]
            : []);
    const marketCodes = new Set([
      ...Object.keys(store.getState().snapshot.snapshot?.markets ?? {}),
      ...eventCodes,
    ]);
    const minuteTicksByCode = new Map<string, PricePoint[]>();
    for (const code of marketCodes) {
      const collector = minuteCollectorsRef.current[code]
        ?? (minuteCollectorsRef.current[code] = new MinutePointCollector(
          code,
          60,
          DEFAULT_SETUP.ticks_per_day,
          DEFAULT_SETUP.auction_ticks,
        ));
      minuteTicksByCode.set(code, collector.collect(intradayEvents));
    }
    const auctionTicksByCode = new Map<string, AuctionPoint[]>();
    for (const code of marketCodes) {
      const collector = auctionCollectorsRef.current[code]
        ?? (auctionCollectorsRef.current[code] = new AuctionPointCollector(
          code,
          60,
          DEFAULT_SETUP.ticks_per_day,
          DEFAULT_SETUP.auction_ticks,
        ));
      auctionTicksByCode.set(code, collector.collect(intradayEvents));
    }

    let selectedDailyChanged = false;
    for (const event of events) {
      if ("Trade" in event) fills.push(event.Trade);
      if ("PriceTick" in event && event.PriceTick.code === chartCode) selectedDailyChanged = true;
    }
    const candleState = reduceCandleEvents(
      dailyCandlesByCodeRef.current,
      activeDailyCandlesRef.current,
      events,
      MAX_DAILY_CANDLES,
    );
    dailyCandlesByCodeRef.current = candleState.completed;
    activeDailyCandlesRef.current = candleState.active;
    selectedDailyChanged ||= candleState.changedCodes.has(chartCode);
    if (fills.length > 0) store.dispatch(appendTrades(fills));

    if (dayChanged) {
      setChartData([]);
      setAuctionChartData([]);
      setDailyChartData(chartCandlesFor(chartCode));
    }
    for (const [code, auctionTicks] of auctionTicksByCode) {
      if (auctionTicks.length === 0) continue;
      auctionHistoryByCodeRef.current[code] = mergeMinutePoints(
        auctionHistoryByCodeRef.current[code] ?? [],
        auctionTicks,
      ).slice(-CALL_AUCTION_ENTRY_MINUTES * AUCTION_VOLUME_LINES_PER_MINUTE);
    }
    for (const [code, minuteTicks] of minuteTicksByCode) {
      if (minuteTicks.length === 0) continue;
      priceHistoryByCodeRef.current[code] = mergeMinutePoints(
        priceHistoryByCodeRef.current[code] ?? [],
        minuteTicks,
      ).slice(-TRADING_MINUTES_PER_DAY);
    }
    if ((minuteTicksByCode.get(chartCode)?.length ?? 0) > 0) {
      setChartData([...(priceHistoryByCodeRef.current[chartCode] ?? [])]);
    }
    if ((auctionTicksByCode.get(chartCode)?.length ?? 0) > 0) {
      setAuctionChartData([...(auctionHistoryByCodeRef.current[chartCode] ?? [])]);
    }
    if (selectedDailyChanged && !dayChanged) {
      setDailyChartData(chartCandlesFor(chartCode));
    }

    for (const event of events) {
      if ("IntentRejected" in event) {
        const rejected = (event as { IntentRejected: IntentRejectedEvent }).IntentRejected;
        setNotice(`委托被拒：${rejected.code} — ${rejectionText(rejected.reason)}`);
      } else if ("SettlementError" in event) {
        const settlement = (event as { SettlementError: SettlementErrorEvent }).SettlementError;
        setNotice(`结算错误：${settlement.code} — ${settlement.reason}`);
      } else if ("VError" in event) {
        const valuation = (event as { VError: { code: string; reason: string } }).VError;
        setNotice(`估值错误：${valuation.code} — ${valuation.reason}`);
      }
    }

    const currentSnapshot = store.getState().snapshot.snapshot;
    if (autoOrderManagerRef.current && currentSnapshot) {
      void autoOrderManagerRef.current.checkEvents(events, currentSnapshot);
    }
    store.dispatch(applyEvents(events));
  };

  const selectChart = useCallback((code: string) => {
    setChartCode(code);
    setChartData([...(priceHistoryByCodeRef.current[code] ?? [])]);
    setAuctionChartData([...(auctionHistoryByCodeRef.current[code] ?? [])]);
    setDailyChartData(chartCandlesFor(code));
  }, [chartCandlesFor]);

  const resetMarketHistory = useCallback((loadedSnapshot: Snapshot) => {
    priceHistoryByCodeRef.current = {};
    minuteCollectorsRef.current = {};
    auctionHistoryByCodeRef.current = {};
    auctionCollectorsRef.current = {};
    activeDailyCandlesRef.current = {};
    setChartData([]);
    setAuctionChartData([]);
    syncDailyCandleSnapshot(loadedSnapshot);
  }, [syncDailyCandleSnapshot]);

  const refreshDailyChart = useCallback(() => {
    setDailyChartData(chartCandlesFor(chartCode));
  }, [chartCandlesFor, chartCode]);

  return {
    chartCode,
    priceHistoryByCodeRef,
    chartData,
    auctionChartData,
    dailyChartData,
    activeDailyCandlesRef,
    onEventsRef,
    acceptRuntimeSnapshot,
    syncDailyCandleSnapshot,
    selectChart,
    resetMarketHistory,
    refreshDailyChart,
  };
}
