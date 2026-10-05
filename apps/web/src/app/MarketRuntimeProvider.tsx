/* oxlint-disable react/only-export-components -- Provider hooks 需要共用这些私有 Context */
import {
  createContext,
  useContext,
  useCallback,
  useMemo,
  useState,
  type Dispatch,
  type MutableRefObject,
  type ReactNode,
  type SetStateAction,
} from "react";
import type { AutoOrderManager } from "../components/auto-order-manager.ts";
import type { KlinePoint, PricePoint } from "../components/PriceChart.tsx";
import type { AuctionPoint } from "../mobile/market-model.ts";
import type { IndicatorCalculator } from "../components/indicator-results.ts";
import { useMarketChartRuntime } from "./useMarketChartRuntime.ts";
import { TradingTimelineContext } from "../components/TradingTimelineContext.tsx";

type Runtime = ReturnType<typeof useMarketChartRuntime>;

interface MarketRuntimeActions {
  configureMarketTiming: Runtime["configureMarketTiming"];
  getPriceHistory: Runtime["getPriceHistory"];
  getActiveDailyCandles: Runtime["getActiveDailyCandles"];
  acceptReduction: Runtime["acceptReduction"];
  installBaseline: Runtime["installBaseline"];
  selectChart: Runtime["selectChart"];
  resetMarketHistory: Runtime["resetMarketHistory"];
  refreshDailyChart: Runtime["refreshDailyChart"];
  setIndicatorCalculator: (calculator: IndicatorCalculator) => () => void;
}

interface MarketRuntimeData {
  chartData: readonly PricePoint[];
  auctionChartData: readonly AuctionPoint[];
  dailyChartData: readonly KlinePoint[];
  indicatorCalculator: IndicatorCalculator | null;
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
  const [calculatorRegistration, setCalculatorRegistration] = useState<{
    readonly token: symbol;
    readonly calculator: IndicatorCalculator;
  } | null>(null);
  const setIndicatorCalculator = useCallback((calculator: IndicatorCalculator) => {
    const token = Symbol("indicator-calculator");
    setCalculatorRegistration({ token, calculator });
    return () => setCalculatorRegistration((current) => current?.token === token ? null : current);
  }, []);
  const actions = useMemo<MarketRuntimeActions>(() => ({
    configureMarketTiming: runtime.configureMarketTiming,
    getPriceHistory: runtime.getPriceHistory,
    getActiveDailyCandles: runtime.getActiveDailyCandles,
    acceptReduction: runtime.acceptReduction,
    installBaseline: runtime.installBaseline,
    selectChart: runtime.selectChart,
    resetMarketHistory: runtime.resetMarketHistory,
    refreshDailyChart: runtime.refreshDailyChart,
    setIndicatorCalculator,
  }), [
    runtime.configureMarketTiming,
    runtime.acceptReduction,
    runtime.getActiveDailyCandles,
    runtime.installBaseline,
    runtime.getPriceHistory,
    runtime.refreshDailyChart,
    runtime.resetMarketHistory,
    runtime.selectChart,
    setIndicatorCalculator,
  ]);
  const data = useMemo<MarketRuntimeData>(() => ({
    chartData: runtime.chartData,
    auctionChartData: runtime.auctionChartData,
    dailyChartData: runtime.dailyChartData,
    indicatorCalculator: calculatorRegistration?.calculator ?? null,
  }), [calculatorRegistration, runtime.auctionChartData, runtime.chartData, runtime.dailyChartData]);

  return (
    <ActionsContext.Provider value={actions}>
      <SelectionContext.Provider value={runtime.chartCode}>
        <TradingTimelineContext.Provider value={runtime.timeline}><DataContext.Provider value={data}>{children}</DataContext.Provider></TradingTimelineContext.Provider>
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
