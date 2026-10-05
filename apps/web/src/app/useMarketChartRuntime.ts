import { useCallback, useRef, useState, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import type { AutoOrderManager } from "../components/auto-order-manager.ts";
import type { KlinePoint, PricePoint } from "../components/PriceChart.tsx";
import { MarketChartProjection } from "./market-chart-projection.ts";
import type { NormalizedEngineUpdate, ProtocolEffect, ProtocolReduction, ProtocolState } from "../host/protocol/index.ts";
import type { AuctionPoint } from "../mobile/market-model.ts";
import type { EngineHost } from "../host/engine-host.ts";
import { toChartCandle } from "../mobile/kline-sync.ts";
import { mergeStockHistoryCandles, StockHistoryRequestGate } from "./market-history-runtime.ts";
import { appendTrades, applyProtocolFrame, setSnapshot, store } from "../store/store.ts";
import type { Snapshot } from "../types/engine.ts";
import { TradingTimeline, DEFAULT_TRADING_TIMELINE, type TradingTiming } from "../components/trading-timeline.ts";

interface Options {
  readonly autoOrderManagerRef: MutableRefObject<AutoOrderManager | null>;
  readonly setNotice: Dispatch<SetStateAction<string | null>>;
  readonly hostRef: MutableRefObject<EngineHost | null>;
}

export function useMarketChartRuntime({ autoOrderManagerRef, setNotice, hostRef }: Options) {
  const [chartCode, setChartCode] = useState("600101");
  const [projection] = useState(() => new MarketChartProjection());
  const [timeline, setTimeline] = useState(DEFAULT_TRADING_TIMELINE);
  const [chartData, setChartData] = useState<readonly PricePoint[]>([]);
  const [auctionChartData, setAuctionChartData] = useState<readonly AuctionPoint[]>([]);
  const [dailyChartData, setDailyChartData] = useState<readonly KlinePoint[]>([]);
  const chartCodeRef = useRef(chartCode);
  const historyRequestRef = useRef(new StockHistoryRequestGate());
  chartCodeRef.current = chartCode;

  const configureMarketTiming = useCallback((timing: TradingTiming) => {
    const next = new TradingTimeline(timing);
    projection.configureTiming(next);
    setTimeline(next);
    setChartData([]);
    setAuctionChartData([]);
  }, [projection]);

  const getPriceHistory = useCallback(() => projection.history(), [projection]);
  const getActiveDailyCandles = useCallback(() => projection.activeCandles(), [projection]);

  const replaceSnapshot = useCallback((snapshot: Snapshot) => {
    projection.replaceSnapshot(snapshot);
    store.dispatch(setSnapshot(snapshot));
    setDailyChartData(projection.candlesFor(chartCode));
  }, [chartCode, projection]);

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
      projection.rebuildHistory(update.update.kinds.includes("BeforeOpen") ? [] : update.intraday);
      replaceSnapshot(reduction.state.snapshot);
    } else {
      projection.upsertFrames(update.frames);
      if (update.runtimeSnapshot !== null) {
        replaceSnapshot(reduction.state.snapshot);
      }
      const finalFrame = update.frames.at(-1);
      if (finalFrame !== undefined && update.runtimeSnapshot === null) {
        projection.replaceActiveCandles(finalFrame.activeDailyCandles);
        store.dispatch(applyProtocolFrame({
          tick: finalFrame.tick,
          seq: finalFrame.seqTo,
          markets: finalFrame.markets,
          activeDailyCandles: finalFrame.activeDailyCandles,
        }));
      }
    }
    setChartData(projection.pricePointsFor(chartCode));
    setAuctionChartData(projection.auctionPointsFor(chartCode));
    setDailyChartData(projection.candlesFor(chartCode));
    applyEffects(reduction.effects);
  }, [applyEffects, chartCode, projection, replaceSnapshot]);

  const installBaseline = useCallback((state: ProtocolState) => {
    projection.installBaseline(state);
    store.dispatch(setSnapshot(state.snapshot));
    setDailyChartData(projection.candlesFor(chartCode));
    setChartData(projection.pricePointsFor(chartCode));
    setAuctionChartData(projection.auctionPointsFor(chartCode));
  }, [chartCode, projection]);

  const selectChart = useCallback((code: string) => {
    chartCodeRef.current = code;
    setChartCode(code);
    setChartData(projection.pricePointsFor(code));
    setAuctionChartData(projection.auctionPointsFor(code));
    setDailyChartData(projection.candlesFor(code));
  }, [projection]);

  const resetMarketHistory = useCallback((snapshot: Snapshot) => {
    projection.reset(snapshot);
    store.dispatch(setSnapshot(snapshot));
    setDailyChartData(projection.candlesFor(chartCode));
    setChartData([]);
    setAuctionChartData([]);
  }, [chartCode, projection]);

  const refreshDailyChart = useCallback(() => setDailyChartData(projection.candlesFor(chartCode)), [chartCode, projection]);

  const queryChartHistory = useCallback(async (code: string): Promise<void> => {
    const host = hostRef.current;
    if (host === null) {
      setNotice(`读取 ${code} 历史日 K 失败：当前没有可用的 EngineHost；请反馈此错误或重新进入本局。`);
      return;
    }
    const request = historyRequestRef.current.begin();
    try {
      const history = await host.queryStockHistory(code);
      if (host !== hostRef.current || !historyRequestRef.current.isCurrent(request) || code !== chartCodeRef.current) return;
      const queried = [...history.daily_candles.map(toChartCandle), ...(history.active_daily_candle === null ? [] : [toChartCandle(history.active_daily_candle)])];
      const current = projection.candlesFor(code);
      const candles = mergeStockHistoryCandles(queried, current);
      setDailyChartData(candles);
    } catch (failure) {
      if (host === hostRef.current && historyRequestRef.current.isCurrent(request) && code === chartCodeRef.current) {
        setNotice(`读取 ${code} 历史日 K 失败：${failure instanceof Error ? failure.message : String(failure)}；请反馈此错误。`);
      }
    }
  }, [hostRef, projection, setNotice]);

  return {
    timeline,
    configureMarketTiming,
    chartCode,
    getPriceHistory,
    chartData,
    auctionChartData,
    dailyChartData,
    getActiveDailyCandles,
    acceptReduction,
    installBaseline,
    selectChart,
    resetMarketHistory,
    refreshDailyChart,
    queryChartHistory,
  };
}

function assertNever(value: never): never {
  throw new Error(`未处理协议效果：${String(value)}`);
}
