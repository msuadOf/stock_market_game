import { useCallback, useState, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import type { AutoOrderManager } from "../components/auto-order-manager.ts";
import type { KlinePoint, PricePoint } from "../components/PriceChart.tsx";
import { MarketChartProjection } from "./market-chart-projection.ts";
import type { NormalizedEngineUpdate, ProtocolEffect, ProtocolReduction, ProtocolState } from "../host/protocol/index.ts";
import type { AuctionPoint } from "../mobile/market-model.ts";
import { appendTrades, applyProtocolFrame, setSnapshot, store } from "../store/store.ts";
import type { Snapshot } from "../types/engine.ts";

interface Options {
  readonly autoOrderManagerRef: MutableRefObject<AutoOrderManager | null>;
  readonly setNotice: Dispatch<SetStateAction<string | null>>;
}

export function useMarketChartRuntime({ autoOrderManagerRef, setNotice }: Options) {
  const [chartCode, setChartCode] = useState("600101");
  const [projection] = useState(() => new MarketChartProjection());
  const [chartData, setChartData] = useState<PricePoint[]>([]);
  const [auctionChartData, setAuctionChartData] = useState<AuctionPoint[]>([]);
  const [dailyChartData, setDailyChartData] = useState<KlinePoint[]>([]);

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
      projection.rebuildHistory(update.intraday);
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

  return {
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
  };
}

function assertNever(value: never): never {
  throw new Error(`未处理协议效果：${String(value)}`);
}
