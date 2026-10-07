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
import type { AccountDividendTaxStatusView, CompanyPreferenceRejectionView, DividendTaxOutstandingView, PersonalTradeHistoryRequest, PersonalTradeHistoryPage, RejectedRightsSubscriptionView } from "../host/engine-host.ts";
import type { PublicReportAvailability, PublicReportAvailabilityQuery } from "../types/engine.ts";
import type { MarketHistoryRequest, MarketHistoryPage } from "../host/market-history.ts";
import type { CurrentMinuteHistoryRequest, CurrentMinuteHistoryResponse } from "../host/current-minute-history.ts";
import { store, selectPlayerAccountId } from "../store/store.ts";
import type { EngineHost, IndicatorCapabilities, IntradayAverageCurveInput, IntradayAverageInput, IntradayAverageResult, PersonalTradeConfirmation } from "../host/engine-host.ts";
import type { KlinePoint, PricePoint } from "../components/PriceChart.tsx";
import type { AuctionPoint } from "../mobile/market-model.ts";
import type { IndicatorCalculator } from "../components/indicator-results.ts";
import { useMarketChartRuntime } from "./useMarketChartRuntime.ts";
import { TradingTimelineContext } from "../components/TradingTimelineContext.tsx";
import { queryPrivateHistory } from "./private-history-query.ts";

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
  queryChartHistory: Runtime["queryChartHistory"];
  setIndicatorCalculator: (calculator: IndicatorCalculator) => () => void;
  queryPersonalTradeConfirmations: (beforeReceipt?: string | null) => Promise<readonly PersonalTradeConfirmation[]>;
  queryPersonalTradeHistory: (request: PersonalTradeHistoryRequest) => Promise<PersonalTradeHistoryPage>;
  queryMarketHistory: (request: MarketHistoryRequest) => Promise<MarketHistoryPage>;
  queryCurrentMinuteHistory: (request: CurrentMinuteHistoryRequest) => Promise<CurrentMinuteHistoryResponse>;
  queryPublicReportAvailability: (query: PublicReportAvailabilityQuery) => Promise<PublicReportAvailability>;
  queryDividendTaxStatus: () => Promise<AccountDividendTaxStatusView>;
  queryDividendTaxOutstanding: () => Promise<readonly DividendTaxOutstandingView[]>;
  queryRejectedRightsSubscriptions: () => Promise<readonly RejectedRightsSubscriptionView[]>;
  queryCompanyPreferenceRejections: (company: string) => Promise<readonly CompanyPreferenceRejectionView[]>;
  calculateIntradayAverage: (input: IntradayAverageInput) => Promise<IntradayAverageResult | null>;
  calculateIntradayAverageCurve: (input: IntradayAverageCurveInput) => Promise<readonly (IntradayAverageResult | null)[]>;
}

interface MarketRuntimeData {
  chartData: readonly PricePoint[];
  auctionChartData: readonly AuctionPoint[];
  dailyChartData: readonly KlinePoint[];
  indicatorCalculator: IndicatorCalculator | null;
  indicatorCapabilities: IndicatorCapabilities;
}

const ActionsContext = createContext<MarketRuntimeActions | null>(null);
const SelectionContext = createContext<string | null>(null);
const DataContext = createContext<MarketRuntimeData | null>(null);

interface Props {
  autoOrderManagerRef: MutableRefObject<AutoOrderManager | null>;
  setNotice: Dispatch<SetStateAction<string | null>>;
  hostRef: MutableRefObject<EngineHost | null>;
  children: ReactNode;
}

export function MarketRuntimeProvider({ autoOrderManagerRef, setNotice, hostRef, children }: Props) {
  const runtime = useMarketChartRuntime({ autoOrderManagerRef, setNotice, hostRef });
  const [calculatorRegistration, setCalculatorRegistration] = useState<{
    readonly token: symbol;
    readonly calculator: IndicatorCalculator;
  } | null>(null);
  const setIndicatorCalculator = useCallback((calculator: IndicatorCalculator) => {
    const token = Symbol("indicator-calculator");
    setCalculatorRegistration({ token, calculator });
    return () => setCalculatorRegistration((current) => current?.token === token ? null : current);
  }, []);
  const queryPersonalTradeConfirmations = useCallback(async (beforeReceipt: string | null = null) => {
    const host = hostRef.current;
    if (host === null) throw new Error("游戏宿主尚未就绪，不能查询本人交割单");
    if (!host.capabilities.personalTradeHistory) throw new Error("当前宿主明确不支持本人交割单查询");
    return host.queryPersonalTradeConfirmations(beforeReceipt);
  }, [hostRef]);
  const queryPersonalTradeHistory = useCallback(async (request: PersonalTradeHistoryRequest) => {
    return queryPrivateHistory(() => {
      const state = store.getState();
      return { host: hostRef.current, generation: state.snapshot.generation, account: selectPlayerAccountId(state) };
    }, async host => {
      if (!host.capabilities.personalTradeHistory) throw new Error("当前宿主不支持本人交割历史查询");
      return host.queryPersonalTradeHistory(request);
    });
  }, [hostRef]);
  const calculateIntradayAverage = useCallback(async (input: IntradayAverageInput) => {
    const host = hostRef.current;
    if (host === null) throw new Error("游戏宿主尚未就绪，不能计算分时均价");
    if (!host.capabilities.indicatorCapabilities.intradayAverage) throw new Error("当前宿主明确不支持 Rust 分时均价计算");
    return host.calculateIntradayAverage(input);
  }, [hostRef]);
  const queryMarketHistory = useCallback(async (request: MarketHistoryRequest) => {
    const host = hostRef.current;
    if (host === null) throw new Error("游戏宿主尚未就绪，不能查询分钟历史");
    const before = store.getState();
    const generation = before.snapshot.generation;
    const account = selectPlayerAccountId(before);
    const page = await host.queryMarketHistory(request);
    const after = store.getState();
    if (hostRef.current !== host || after.snapshot.generation !== generation || selectPlayerAccountId(after) !== account) throw new Error("分钟历史响应属于已切换的宿主、市场或账户");
    return page;
  }, [hostRef]);
  const queryCurrentMinuteHistory = useCallback(async (request: CurrentMinuteHistoryRequest) => {
    const host = hostRef.current;
    if (host === null) throw new Error("游戏宿主尚未就绪，不能查询当前分钟");
    const before = store.getState(), generation = before.snapshot.generation, account = selectPlayerAccountId(before);
    const response = await host.queryCurrentMinuteHistory(request);
    const after = store.getState();
    if (hostRef.current !== host || after.snapshot.generation !== generation || selectPlayerAccountId(after) !== account) throw new Error("当前分钟响应属于已切换的宿主、市场或账户");
    return response;
  }, [hostRef]);
  const queryPublicReportAvailability = useCallback(async (query: PublicReportAvailabilityQuery) => {
    const host = hostRef.current;
    if (host === null) throw new Error("游戏宿主尚未就绪，不能查询公开报告可用性");
    if (host.queryPublicReportAvailability === undefined) throw new Error("当前宿主不支持公开报告可用性查询");
    const before = store.getState();
    const generation = before.snapshot.generation;
    const result = await host.queryPublicReportAvailability(query);
    const after = store.getState();
    if (hostRef.current !== host || after.snapshot.generation !== generation) throw new Error("公开报告可用性响应属于已切换的宿主或市场");
    return result;
  }, [hostRef]);
  const queryDividendTaxStatus = useCallback(async () => {
    const host = hostRef.current;
    if (host === null) throw new Error("游戏宿主尚未就绪，不能查询股息税状态");
    if (host.dividendTaxStatus === undefined) throw new Error("当前宿主不支持股息税状态查询");
    const before = store.getState();
    const generation = before.snapshot.generation;
    const result = await host.dividendTaxStatus();
    const after = store.getState();
    if (hostRef.current !== host || after.snapshot.generation !== generation) throw new Error("股息税状态响应属于已切换的宿主或市场");
    return result;
  }, [hostRef]);
  const queryDividendTaxOutstanding = useCallback(async () => {
    const host = hostRef.current;
    if (host === null) throw new Error("游戏宿主尚未就绪，不能查询股息税未清税额");
    if (host.dividendTaxOutstanding === undefined) throw new Error("当前宿主不支持股息税未清税额查询");
    const before = store.getState();
    const generation = before.snapshot.generation;
    const result = await host.dividendTaxOutstanding();
    const after = store.getState();
    if (hostRef.current !== host || after.snapshot.generation !== generation) throw new Error("股息税未清税额响应属于已切换的宿主或市场");
    return result;
  }, [hostRef]);
  const queryRejectedRightsSubscriptions = useCallback(async () => {
    const host = hostRef.current;
    if (host === null) throw new Error("游戏宿主尚未就绪，不能查询配股认购拒绝回执");
    if (host.rejectedRightsSubscriptions === undefined) throw new Error("当前宿主不支持配股认购拒绝回执查询");
    const before = store.getState();
    const generation = before.snapshot.generation;
    const result = await host.rejectedRightsSubscriptions();
    const after = store.getState();
    if (hostRef.current !== host || after.snapshot.generation !== generation) throw new Error("配股认购拒绝回执响应属于已切换的宿主或市场");
    return result;
  }, [hostRef]);
  const queryCompanyPreferenceRejections = useCallback(async (company: string) => {
    const host = hostRef.current;
    if (host === null) throw new Error("游戏宿主尚未就绪，不能查询偏好拒绝台账");
    if (host.companyPreferenceRejections === undefined) throw new Error("当前宿主不支持偏好拒绝台账查询");
    const before = store.getState();
    const generation = before.snapshot.generation;
    const result = await host.companyPreferenceRejections(company);
    const after = store.getState();
    if (hostRef.current !== host || after.snapshot.generation !== generation) throw new Error("偏好拒绝台账响应属于已切换的宿主或市场");
    return result;
  }, [hostRef]);
  const calculateIntradayAverageCurve = useCallback(async (input: IntradayAverageCurveInput) => {
    const host = hostRef.current;
    if (host === null) throw new Error("游戏宿主尚未就绪，不能计算分时均价曲线");
    if (!host.capabilities.indicatorCapabilities.intradayAverage) throw new Error("当前宿主明确不支持 Rust 分时均价曲线计算");
    return host.calculateIntradayAverageCurve(input);
  }, [hostRef]);
  const actions = useMemo<MarketRuntimeActions>(() => ({
    configureMarketTiming: runtime.configureMarketTiming,
    getPriceHistory: runtime.getPriceHistory,
    getActiveDailyCandles: runtime.getActiveDailyCandles,
    acceptReduction: runtime.acceptReduction,
    installBaseline: runtime.installBaseline,
    selectChart: runtime.selectChart,
    resetMarketHistory: runtime.resetMarketHistory,
    refreshDailyChart: runtime.refreshDailyChart,
    queryChartHistory: runtime.queryChartHistory,
    setIndicatorCalculator,
    queryPersonalTradeConfirmations,
    queryPersonalTradeHistory,
    queryMarketHistory,
    queryCurrentMinuteHistory,
    queryPublicReportAvailability,
    queryDividendTaxStatus,
    queryDividendTaxOutstanding,
    queryRejectedRightsSubscriptions,
    queryCompanyPreferenceRejections,
    calculateIntradayAverage,
    calculateIntradayAverageCurve,
  }), [
    runtime.configureMarketTiming,
    runtime.acceptReduction,
    runtime.getActiveDailyCandles,
    runtime.installBaseline,
    runtime.getPriceHistory,
    runtime.refreshDailyChart,
    runtime.resetMarketHistory,
    runtime.selectChart,
    runtime.queryChartHistory,
    setIndicatorCalculator,
    queryPersonalTradeConfirmations,
    queryPersonalTradeHistory,
    queryMarketHistory,
    queryCurrentMinuteHistory,
    queryPublicReportAvailability,
    queryDividendTaxStatus,
    queryDividendTaxOutstanding,
    queryRejectedRightsSubscriptions,
    queryCompanyPreferenceRejections,
    calculateIntradayAverage,
    calculateIntradayAverageCurve,
  ]);
  const data = useMemo<MarketRuntimeData>(() => ({
    chartData: runtime.chartData,
    auctionChartData: runtime.auctionChartData,
    dailyChartData: runtime.dailyChartData,
    indicatorCalculator: calculatorRegistration?.calculator ?? null,
    indicatorCapabilities: hostRef.current?.capabilities.indicatorCapabilities ?? { intradayAverage: false, macd: false, priceKdj: false, candleKdj: false },
  }), [calculatorRegistration, hostRef, runtime.auctionChartData, runtime.chartData, runtime.dailyChartData]);

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
