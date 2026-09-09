/* oxlint-disable react/only-export-components -- provider hooks intentionally share these private contexts */
import {
  createContext,
  useContext,
  useMemo,
  type Dispatch,
  type MutableRefObject,
  type ReactNode,
  type SetStateAction,
} from "react";
import type { AutoOrderManager } from "../components/auto-order-manager.ts";
import type { KlinePoint, PricePoint } from "../components/PriceChart.tsx";
import type { AuctionPoint } from "../mobile/market-model.ts";
import { useMarketChartRuntime } from "./useMarketChartRuntime.ts";

type Runtime = ReturnType<typeof useMarketChartRuntime>;

interface MarketRuntimeActions {
  priceHistoryByCodeRef: Runtime["priceHistoryByCodeRef"];
  activeDailyCandlesRef: Runtime["activeDailyCandlesRef"];
  onEventsRef: Runtime["onEventsRef"];
  acceptRuntimeSnapshot: Runtime["acceptRuntimeSnapshot"];
  selectChart: Runtime["selectChart"];
  resetMarketHistory: Runtime["resetMarketHistory"];
  refreshDailyChart: Runtime["refreshDailyChart"];
}

interface MarketRuntimeData {
  chartData: PricePoint[];
  auctionChartData: AuctionPoint[];
  dailyChartData: KlinePoint[];
}

const ActionsContext = createContext<MarketRuntimeActions | null>(null);
const SelectionContext = createContext<string | null>(null);
const DataContext = createContext<MarketRuntimeData | null>(null);

interface Props {
  autoOrderManagerRef: MutableRefObject<AutoOrderManager | null>;
  setNotice: Dispatch<SetStateAction<string | null>>;
  children: ReactNode;
}

export function MarketRuntimeProvider({ autoOrderManagerRef, setNotice, children }: Props) {
  const runtime = useMarketChartRuntime({ autoOrderManagerRef, setNotice });
  const actions = useMemo<MarketRuntimeActions>(() => ({
    priceHistoryByCodeRef: runtime.priceHistoryByCodeRef,
    activeDailyCandlesRef: runtime.activeDailyCandlesRef,
    onEventsRef: runtime.onEventsRef,
    acceptRuntimeSnapshot: runtime.acceptRuntimeSnapshot,
    selectChart: runtime.selectChart,
    resetMarketHistory: runtime.resetMarketHistory,
    refreshDailyChart: runtime.refreshDailyChart,
  }), [
    runtime.acceptRuntimeSnapshot,
    runtime.activeDailyCandlesRef,
    runtime.onEventsRef,
    runtime.priceHistoryByCodeRef,
    runtime.refreshDailyChart,
    runtime.resetMarketHistory,
    runtime.selectChart,
  ]);
  const data = useMemo<MarketRuntimeData>(() => ({
    chartData: runtime.chartData,
    auctionChartData: runtime.auctionChartData,
    dailyChartData: runtime.dailyChartData,
  }), [runtime.auctionChartData, runtime.chartData, runtime.dailyChartData]);

  return (
    <ActionsContext.Provider value={actions}>
      <SelectionContext.Provider value={runtime.chartCode}>
        <DataContext.Provider value={data}>{children}</DataContext.Provider>
      </SelectionContext.Provider>
    </ActionsContext.Provider>
  );
}

export function useMarketRuntimeActions(): MarketRuntimeActions {
  const value = useContext(ActionsContext);
  if (!value) throw new Error("MarketRuntimeProvider actions 未挂载");
  return value;
}

export function useMarketRuntimeSelection(): string {
  const value = useContext(SelectionContext);
  if (!value) throw new Error("MarketRuntimeProvider selection 未挂载");
  return value;
}

export function useMarketRuntimeData(): MarketRuntimeData {
  const value = useContext(DataContext);
  if (!value) throw new Error("MarketRuntimeProvider data 未挂载");
  return value;
}
