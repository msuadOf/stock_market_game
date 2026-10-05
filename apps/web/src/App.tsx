/**
 * 应用根组件（WS-4 完整版）。
 *
 * 架构：
 * - WASM 引擎在 Web Worker 中运行（不阻塞 UI）。
 * - 桌面/移动按横竖屏切换布局。
 * - AG Grid 行情表 + Lightweight Charts 分时图（量能/MACD/KDJ）。
 * - 自动单/条件单（客户端侧）。
 * - 亮/暗主题切换。
 */
import { useEffect, useRef, useState, useCallback, lazy, Suspense, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import { Button, Card, HTMLSelect, Switch } from "@blueprintjs/core";
import { useSelector } from "react-redux";
import type { DeliveryMode, EngineHost, SpeedMetrics } from "./host/engine-host";
import type { HostFailure, HostUpdate } from "./host/host-update.ts";
import { CompanyQueryCoordinator } from "./host/company-query-coordinator.ts";
import { ProtocolCoordinator } from "./host/protocol-coordinator.ts";
import { SpeedMetricsRequestGate, speedMetricsMatchesUiState } from "./host/speed";
import { browserWasmEnvironment, initialStartupTarget, resolveStartupTarget, type StartupMode, type StartupTarget } from "./host/startup-policy";
import { DEFAULT_SETUP, DEFAULT_SEED } from "./config/defaults";
import { createNewSessionSeed } from "./config/session-seed.ts";
import { changeSeedDraft, changeSettlementCycleDraft, createSeedDraft, parseSessionSeed, regenerateSeedDraft, type SeedDraft, type DraftSettlementCycle } from "./config/seed-draft.ts";
import { parseCompanySystemConfig } from "./save/schema/company/system-config.ts";
import { SessionControlCommands } from "./app/session-control-commands.ts";
import { buildErrorFeedback } from "./app/error-details.ts";
import { StartDateInput } from "./components/StartDateInput.tsx";
import { PriceCageInput } from "./components/PriceCageInput.tsx";
import { ReportFrequencyInput, type ReportFrequencyDraft } from "./components/ReportFrequencyInput.tsx";
import { FloatAllocationInput } from "./components/FloatAllocationInput.tsx";
import { CompanySystemInput } from "./components/company/CompanySystemInput.tsx";
import { InitialAllocationSummary } from "./components/InitialAllocationSummary.tsx";
import type { InitialAllocation } from "./host/initial-allocation.ts";
import { FatalHostError, SpeedMetricsAlert } from "./app/HostStatusViews.tsx";
import { StartupScreen } from "./app/StartupScreen.tsx";
import type { DesktopView } from "./app/DesktopTerminal.tsx";
import { RemoteLoginScreen } from "./app/RemoteLoginScreen.tsx";
import { RemoteAuthClient } from "./host/remote-auth.ts";
import { IndexedDbCredentialStore } from "./auth/credential-store.ts";
import { logoutRemoteIdentity } from "./app/remote-logout.ts";
import { WorkspaceGrid } from "./app/WorkspaceGrid.tsx";
import type { SessionSetup } from "./types/engine";
import {
  setRunning,
  setSpeed,
  setTheme,
  setPauseAfterClose,
  setPauseBeforeOpen,
  store,
  removeAutoOrder,
  toggleAutoOrder,
  markTriggered,
  clearTriggeredOrders,
  clearAutoOrders,
  setRemoteMembership,
  selectPlayerAccount,
  selectCanControl,
  type RootState,
} from "./store/store";
import { useSessionHostLifecycle } from "./app/useSessionHostLifecycle.ts";
import { useSaveCommands } from "./app/useSaveCommands.ts";
import { useTradingCommands } from "./app/useTradingCommands.ts";
import { useSpeedMetricsPolling } from "./app/useSpeedMetricsPolling.ts";
import { useSecurityBrowser } from "./app/useSecurityBrowser.ts";
import type { MobilePrimaryTab } from "./mobile/mobile-ui-state";
import { usePausePreferences } from "./app/usePausePreferences.ts";
import "./App.css";
import "./app/desktop-terminal.css";
import "ag-grid-community/styles/ag-grid.css";
import "ag-grid-community/styles/ag-theme-alpine.css";
import { AutoOrderManager, AUTO_ORDER_LABELS, type AutoOrderType } from "./components/auto-order-manager";
import { useOrientation } from "./hooks/useOrientation";
import { selectDayEndFileTarget, loadFromFile, type DayEndFileTarget } from "./save/save-file";
import { DayEndPersistence } from "./save/day-end-persistence.ts";
import { validateDayEndCandidate } from "./save/day-end-candidate.ts";
import type { StrictSaveEnvelope } from "./save/schema/root.ts";
import { writeDayEndTargets } from "./save/day-end-targets.ts";
import { InitialSaveSource, SessionReplacementGate } from "./save/session-replacement.ts";
import { IndexedDbSaveRepository } from "./save/indexeddb-save-repository.ts";
import { ArchiveManager } from "./components/ArchiveManager.tsx";
import type { ArchiveStore } from "./save/archive-store.ts";
import { PlayerOrderRefreshGate, playerOrderFactsRequireRefresh } from "./components/player-orders.ts";
import { MobileSpeedSelect } from "./mobile/MobileSpeedSelect";
import { MobileRunToggle } from "./mobile/MobileRunToggle";
import { MOBILE_PRIMARY_NAV, formatMeasuredSpeed, mobilePrimaryTitle } from "./mobile/mobile-ui-state";
import { yuan } from "./utils/format";
import { QuickTrading } from "./app/quick-trading.ts";
import { QuickTradingPanel } from "./app/QuickTradingPanel.tsx";
import { QuickTradingContext } from "./app/QuickTradingContext.ts";
import { useMobileUiController } from "./app/useMobileUiController";
import { MobileDetailLayer } from "./mobile/MobileDetailLayer.tsx";
import { MarketRuntimeProvider, useMarketRuntimeActions, useMarketRuntimeSelection } from "./app/MarketRuntimeProvider.tsx";
import {
  ClockMarker,
  ConnectedChartPanel,
  ConnectedCompanyPanel,
  ConnectedMarketPanel,
  ConnectedTerminalStockList,
  ConnectedMobileDetail,
  ConnectedMobileGameClock,
  DesktopAssets,
  DesktopDayTag,
  PositionsPanel,
  TradesPanel,
  UserPanel,
} from "./app/LocalRefreshViews.tsx";

const DevNpcInspector = import.meta.env.DEV ? lazy(() => import("./dev/NpcDecisionInspector.tsx").then((module) => ({ default: module.NpcDecisionInspector }))) : null;
const MAX_DAILY_CANDLES = 360;
const TRADING_E2E_MODE = import.meta.env.MODE === "e2e"
  && new URLSearchParams(window.location.search).get("tradingE2E") === "1";
const INITIAL_SESSION_SETUP: SessionSetup = TRADING_E2E_MODE
  ? {
      ...DEFAULT_SETUP,
      npcs: { ...DEFAULT_SETUP.npcs, retail_count: 0, inst_count: 0, hot_count: 0 },
      float_allocation: { between_kinds: "Random", within_kind: "Random" },
      ticks_per_day: 30,
      auction_ticks: 9,
      closing_auction_ticks: 3,
    }
  : DEFAULT_SETUP;
const INITIAL_SEED_PREVIEW = (() => {
  try { return { draft: createSeedDraft(INITIAL_SESSION_SETUP, TRADING_E2E_MODE ? DEFAULT_SEED : createNewSessionSeed()), error: null }; }
  catch (failure) { return { draft: null, error: `新局预览初始化失败：${failure instanceof Error ? failure.message : String(failure)}` }; }
})();
const DELIVERY_MODE_LABELS: Record<DeliveryMode, string> = {
  push: "服务端推送 60Hz",
  pull: "客户端拉取 60Hz",
};
let browserSaveRepository: IndexedDbSaveRepository | null = null;

declare global {
  interface Window {
    __STOCK_GAME_E2E__?: {
      pause(): Promise<void>;
      advanceToTick(target: number): Promise<number>;
      snapshot(): ReturnType<EngineHost["snapshot"]>;
    };
  }
}

function getBrowserSaveRepository(): IndexedDbSaveRepository {
  if (typeof window === "undefined") throw new Error("浏览器存储在当前运行环境不可用");
  browserSaveRepository ??= new IndexedDbSaveRepository(window.indexedDB);
  return browserSaveRepository;
}

interface AppShellProps {
  startupTarget: StartupTarget;
  initialSaveSourceRef: MutableRefObject<InitialSaveSource<StrictSaveEnvelope>>;
  dayEndPersistenceRef: MutableRefObject<DayEndPersistence>;
  sessionSetup: SessionSetup;
  chosenSessionSeed: string;
  chosenConfigOrigin: SeedDraft["origin"];
  setSessionCreation(setup: SessionSetup, seed: string): void;
  returningToStartup: boolean;
  returningToStartupRef: MutableRefObject<boolean>;
  startupReturnError: string | null;
  onSelectHost: (stopSession: () => Promise<void>) => Promise<void>;
  autoOrderMgrRef: MutableRefObject<AutoOrderManager | null>;
  hostRef: MutableRefObject<EngineHost | null>;
  notice: string | null;
  setNotice: Dispatch<SetStateAction<string | null>>;
}

function AppShell({ startupTarget, initialSaveSourceRef, dayEndPersistenceRef, sessionSetup, chosenSessionSeed, chosenConfigOrigin, setSessionCreation, returningToStartup, returningToStartupRef, startupReturnError, onSelectHost, autoOrderMgrRef, hostRef, notice, setNotice }: AppShellProps) {
  const [managedArchives, setManagedArchives] = useState<ArchiveStore | null>(null);
  const hasSnapshot = useSelector((s: RootState) => s.snapshot.snapshot !== null);
  const snapshotGeneration = useSelector((state: RootState) => state.snapshot.generation);
  const playerAccount = useSelector(selectPlayerAccount);
  const canControl = useSelector(selectCanControl);
  const membership = useSelector((state: RootState) => state.remoteMembership);
  const [remoteConfirmation, setRemoteConfirmation] = useState<"reset" | "load" | "file" | "logout" | "rejoin" | null>(null);
  const [admissionCashDraft, setAdmissionCashDraft] = useState(sessionSetup.config.starting_cash);
  const contextSubscriptionRef = useRef<(() => void) | null>(null);
  const speed = useSelector((s: RootState) => s.settings.speed);
  const running = useSelector((s: RootState) => s.settings.running);
  const theme = useSelector((s: RootState) => s.settings.theme);
  const pauseAfterClose = useSelector((s: RootState) => s.settings.pauseAfterClose);
  const pauseBeforeOpen = useSelector((s: RootState) => s.settings.pauseBeforeOpen);
  const autoOrders = useSelector((s: RootState) => s.autoOrders.items);
  const orientation = useOrientation();
  const [ready, setReady] = useState(false);
  const [hostBaselineReady, setHostBaselineReady] = useState(false);
  const [showNpcInspector, setShowNpcInspector] = useState(false);
  const [error, setError] = useState<string | HostFailure | null>(null);
  const [activeSetup, setActiveSetup] = useState<SessionSetup>(sessionSetup);
  const [startDateDraft, setStartDateDraft] = useState(sessionSetup.start_date);
  const [priceCageEnabledDraft, setPriceCageEnabledDraft] = useState(sessionSetup.config.price_cage_enabled);
  const [reportFrequencyDraft, setReportFrequencyDraft] = useState<ReportFrequencyDraft>(sessionSetup.report_frequency);
  const [companyDraft, setCompanyDraft] = useState<SeedDraft>(() => ({ seed: chosenSessionSeed, origin: chosenConfigOrigin, companySystem: JSON.stringify(sessionSetup.company_system, null, 2) }));
  const companySystemDraft = companyDraft.companySystem;
  const setCompanySystemDraft = (config: string) => setCompanyDraft(draft => ({ ...draft, companySystem: config }));
  const setSeedDraft = (seed: string, preserveOrigin = false) => setCompanyDraft(draft => ({ ...draft, seed, origin: preserveOrigin ? draft.origin : "custom" }));
  const regenerateCompanyDraft = () => {
    try { setCompanyDraft(regenerateSeedDraft(activeSetup, companyDraft)); }
    catch (failure) { setNotice(`重新生成预览失败：${failure instanceof Error ? failure.message : String(failure)}；当前 seed 与配置未改变。`); }
  };
  const changeCompanySeed = (seed: string) => {
    try { setCompanyDraft(changeSeedDraft(activeSetup, companyDraft, seed)); }
    catch (failure) { setNotice(`更新预览失败：${failure instanceof Error ? failure.message : String(failure)}；当前配置未改变。`); }
  };
  const changeCompanySettlementCycle = (cycle: DraftSettlementCycle) => {
    try { setCompanyDraft(changeSettlementCycleDraft(activeSetup, companyDraft, cycle)); }
    catch (failure) { setNotice(`更新结算周期失败：${failure instanceof Error ? failure.message : String(failure)}；当前配置未改变。`); }
  };
  const [floatAllocationDraft, setFloatAllocationDraft] = useState(sessionSetup.float_allocation);
  const [initialAllocation, setInitialAllocation] = useState<InitialAllocation | null>(null);
  const [startDateError, setStartDateError] = useState<string | null>(null);
  const [speedMetrics, setSpeedMetrics] = useState<SpeedMetrics | null>(null);
  const [speedMetricsError, setSpeedMetricsError] = useState<string | null>(null);
  const [speedMetricsPollingGeneration, setSpeedMetricsPollingGeneration] = useState(0);
  const [deliveryMode, setDeliveryModeState] = useState<DeliveryMode | null>(null);
  const [deliveryModes, setDeliveryModes] = useState<readonly DeliveryMode[]>([]);
  const [pausePreferencesReady, setPausePreferencesReady] = useState(false);
  const speedMetricsRequestGateRef = useRef(new SpeedMetricsRequestGate());
  const speedMetricsLoadInProgressRef = useRef(false);
  const playerOrderRefreshGateRef = useRef(new PlayerOrderRefreshGate());
  const pausePreferencesRef = useRef({ pauseAfterClose, pauseBeforeOpen });
  pausePreferencesRef.current = { pauseAfterClose, pauseBeforeOpen };
  const activeSetupRef = useRef(activeSetup);
  activeSetupRef.current = activeSetup;
  const [quickTrading] = useState(() => new QuickTrading({ setup: () => activeSetupRef.current, snapshot: () => store.getState().snapshot.snapshot, account: () => selectPlayerAccount(store.getState()), autoAllowed: () => store.getState().settings.running || TRADING_E2E_MODE, submit: async (intent) => { const host = hostRef.current; if (!host) throw new Error("游戏引擎尚未就绪"); await host.submitIntent(intent); } }));
  const stopStartupRef = useRef<() => Promise<void>>(async () => {});
  const dayEndFileTargetRef = useRef<DayEndFileTarget | null>(null);
  const saveSelectionGenerationRef = useRef(0);
  const sessionReplacementGateRef = useRef(new SessionReplacementGate());
  const companyCoordinatorRef = useRef<CompanyQueryCoordinator | null>(null);
  const protocolCoordinatorRef = useRef<ProtocolCoordinator | null>(null);
  const fatalHostErrorRef = useRef<(failure: string | HostFailure) => void>(() => {});
  fatalHostErrorRef.current = (failure) => {
    store.dispatch(setRunning(false));
    if (typeof failure !== "string" && failure.code === "REPORT_CORRECTION_REJECTED" && failure.recoverable === true) {
      setNotice(`日终财报更正失败，已暂停；无部分账务或部分公开。请在公司信息的市场控制区查询、取消问题待办后重试。错误反馈：${buildErrorFeedback(failure)}`);
      return;
    }
    setError(failure);
  };
  const chartCode = useMarketRuntimeSelection();
  const {
    mobileUi,
    dispatchMobileUi,
    mobileTab,
    tradeSheetOpen,
    mobileDetail,
    tradeSheetRef,
    switchMobileTab: switchPrimaryTab,
    openTradeSheet,
    closeTradeSheet,
    showDetailInfo,
    openDetail,
  } = useMobileUiController(orientation, chartCode);
  const { acceptReduction, installBaseline, selectChart, resetMarketHistory, refreshDailyChart, setIndicatorCalculator, configureMarketTiming } = useMarketRuntimeActions();
  const acceptReductionRef = useRef(acceptReduction);
  acceptReductionRef.current = acceptReduction;
  const installBaselineRef = useRef(installBaseline);
  installBaselineRef.current = installBaseline;
  const hostUpdateRef = useRef<(update: HostUpdate) => void | boolean>(() => false);
  hostUpdateRef.current = (update) => {
    const coordinator = protocolCoordinatorRef.current;
    return coordinator === null ? false : coordinator.accept(update);
  };
  const runningRef = useRef(running);
  runningRef.current = running;
  const [sessionControls] = useState(() => new SessionControlCommands({ hostRef, runningRef,
    onUpdate: (update) => hostUpdateRef.current(update),
    onFatal: (failure) => fatalHostErrorRef.current(failure),
    onRunning: (value) => { runningRef.current = value; store.dispatch(setRunning(value)); },
    onSpeed: (value) => { store.dispatch(setSpeed(value)); setSpeedMetrics(null); setSpeedMetricsError(null); },
    onError: setError,
  }));
  const [desktopView, setDesktopView] = useState<DesktopView>("quotes");
  const [desktopTradingOpen, setDesktopTradingOpen] = useState(false);
  const [desktopTradeSide, setDesktopTradeSide] = useState<"Buy" | "Sell">("Buy");
  const desktopTradeTriggerRef = useRef<HTMLElement | null>(null);
  const changeDesktopTradingOpen = (open: boolean) => {
    if (open && document.activeElement instanceof HTMLElement && !document.activeElement.closest("#section-order")) desktopTradeTriggerRef.current = document.activeElement;
    setDesktopTradingOpen(open);
    const trigger = desktopTradeTriggerRef.current;
    if (!open && trigger?.isConnected && trigger.getClientRects().length > 0) trigger.focus();
  };
  const openDesktopTrade = (side: "Buy" | "Sell" = "Buy") => {
    if (tradeCode !== chartCode || priceText.length === 0) {
      setTradeCode(chartCode);
      const market = store.getState().snapshot.snapshot?.markets[chartCode];
      if (market) setPriceText(yuan(market.last_price));
    }
    setDesktopTradeSide(side);
    changeDesktopTradingOpen(true);
  };
  useEffect(() => {
    if (desktopTradingOpen && orientation === "landscape") tradeSheetRef.current?.querySelector<HTMLInputElement>(`.trade-ticket[data-side="${desktopTradeSide}"] input:not(:disabled)`)?.focus();
  }, [desktopTradingOpen, desktopTradeSide, orientation, tradeSheetRef]);
  const securityBrowser = useSecurityBrowser(setNotice);
  function switchMobileTab(tab: MobilePrimaryTab) {
    if (tab === "market" || tab === "watchlist") securityBrowser.setView(tab === "market" ? "all" : "watchlist");
    switchPrimaryTab(tab);
  }
  const chartPeriod = mobileUi.chartPeriod;
  const [klineDays, setKlineDays] = useState<number>(MAX_DAILY_CANDLES);

  function selectStock(code: string) {
    selectChart(code);
    if (tradeCode !== code || priceText.length === 0) {
      setTradeCode(code);
      const market = store.getState().snapshot.snapshot?.markets[code];
      if (market) setPriceText(yuan(market.last_price));
    }
    if (orientation === "portrait") openDetail(code);
  }

  const protocolPlayerOrders = useSelector((state: RootState) => state.snapshot.playerWorkingOrders);
  const playerOrdersReady = useSelector((state: RootState) => state.snapshot.playerOrdersReady);
  const tradingCommands = useTradingCommands({ hostRef, playerOrderRefreshGateRef, autoOrderMgrRef, activeSetup, playerAccount, protocolPlayerOrders, playerOrdersReady, setNotice });
  const { tradeCode, priceText, autoType, autoTrigger, autoQty } = tradingCommands.form;
  const { autoFieldErrors } = tradingCommands;
  const { setTradeCode, setPriceText, setAutoType, setAutoTrigger, setAutoQty,
    playerOrders, cancelingOrderIds, clearPlayerOrders, clearCancelingOrderIds, cancelPlayerOrder, addAuto, refreshPlayerOrders } = tradingCommands;

  const [cancelAllPending, setCancelAllPending] = useState(false);
  async function cancelAllOrders() {
    if (cancelAllPending) return;
    setCancelAllPending(true);
    try {
      const host = hostRef.current;
      if (!host) throw new Error("游戏引擎尚未就绪");
      const generation = saveSelectionGenerationRef.current;
      const orders = await host.playerWorkingOrders();
      if (host !== hostRef.current || generation !== saveSelectionGenerationRef.current) throw new Error("游戏会话已发生变化，请重新核对挂单后撤单");
      await quickTrading.cancelAll(orders);
      setNotice(orders.length ? `已提交全部 ${orders.length} 笔挂单的撤单请求；自动保持开启` : "暂无活动挂单；自动保持开启");
    } catch (error) { setNotice(`取消所有失败：${error instanceof Error ? error.message : String(error)}`); }
    finally { setCancelAllPending(false); }
  }

  function connectProtocol(host: EngineHost) {
    contextSubscriptionRef.current?.();
    if (host.onMarketContext !== undefined) {
      contextSubscriptionRef.current = host.onMarketContext((context) => {
        if (host !== hostRef.current) return;
        const previous = store.getState().remoteMembership;
        const accountId = context.member === null ? null : context.member.account_id;
        const changingAccount = !previous.remote || previous.accountId !== accountId || previous.generation !== context.generation;
        if (changingAccount) {
          quickTrading.reset();
          autoOrderMgrRef.current?.clear();
          store.dispatch(clearAutoOrders());
          clearPlayerOrders();
        }
        store.dispatch(setRemoteMembership({ accountId, canControl: context.can_control, needsRejoin: context.needs_rejoin, remote: true, generation: context.generation, ready: changingAccount ? false : previous.ready }));
        setActiveSetup(context.setup);
        setAdmissionCashDraft(context.setup.config.starting_cash);
        setStartDateDraft(context.setup.start_date);
        setPriceCageEnabledDraft(context.setup.config.price_cage_enabled);
        setFloatAllocationDraft(context.setup.float_allocation);
        setReportFrequencyDraft(context.setup.report_frequency);
        setCompanySystemDraft(JSON.stringify(context.setup.company_system, null, 2));
        setSeedDraft(context.seed);
      });
    } else {
      store.dispatch(setRemoteMembership({ accountId: "0", canControl: true, needsRejoin: false, remote: false }));
    }
    companyCoordinatorRef.current = new CompanyQueryCoordinator(host, store.dispatch);
    protocolCoordinatorRef.current = new ProtocolCoordinator({
      onBaseline(protocolState, baseline) {
        if (quickTrading.jobs().some(job => job.enabled)) setNotice("已重新同步游戏，定时自动下单已停止，请核对后重新开启");
        quickTrading.reset();
        speedMetricsRequestGateRef.current.invalidate();
        setSpeedMetricsPollingGeneration(speedMetricsRequestGateRef.current.capture());
        setSpeedMetrics(null);
        setSpeedMetricsError(null);
        saveSelectionGenerationRef.current += 1;
        dayEndPersistenceRef.current.install(baseline.generation);
        clearPlayerOrders();
        companyCoordinatorRef.current?.installBaseline({ civilDate: baseline.civilDate, revision: baseline.revision, seq: protocolState.snapshot.seq });
        installBaselineRef.current(protocolState);
        if (host.capabilities.persistence === "remote") store.dispatch(setRemoteMembership({ ...store.getState().remoteMembership, ready: true }));
        setHostBaselineReady(true);
        if (!TRADING_E2E_MODE) void refreshPlayerOrders();
      },
      onApplied(reduction, metadata) {
        const companyCoordinator = companyCoordinatorRef.current;
        if (reduction.update.kind === "tick-batch") {
          for (const frame of reduction.update.frames) companyCoordinator?.acceptFrame(frame, metadata);
        } else {
          companyCoordinator?.acceptCivil(reduction.update, metadata);
          clearPlayerOrders();
          const target = dayEndFileTargetRef.current;
          const browserPersistence = host.capabilities.persistence === "browser";
          if (!browserPersistence && target === null) {
            void refreshPlayerOrders();
            acceptReductionRef.current(reduction);
            return;
          }
          const reference = { seq: reduction.update.update.seq_to, settledDate: reduction.update.update.boundary.settled_date };
          void dayEndPersistenceRef.current.completed(
            reduction.state.cursor.generation,
            host.save(reference).then((slot) => validateDayEndCandidate(slot, reference)),
            async (slot, isCurrent) => {
              return await writeDayEndTargets(slot, isCurrent, [
                ...(browserPersistence ? [{ label: "浏览器 IndexedDB", write: (value: unknown, current: () => boolean) => getBrowserSaveRepository().save(value, current) }] : []),
                ...(target === null ? [] : [{ label: "授权文件", write: (value: unknown, current: () => boolean) => target.write(value, () => current() && target === dayEndFileTargetRef.current) }]),
              ]);
            },
          ).then((saved) => {
            if (saved && host === hostRef.current) setNotice(browserPersistence ? "日终存档已更新（IndexedDB）" : "授权日终文件已更新；数据库由 Native 宿主保存");
          }, (saveError: unknown) => {
            if (host === hostRef.current) setNotice(`日终存档更新失败：${saveError instanceof Error ? saveError.message : String(saveError)}`);
          });
          void refreshPlayerOrders();
        }
        acceptReductionRef.current(reduction);
        void quickTrading.advance(reduction.state.snapshot);
        const playerOrderChanged = reduction.update.kind === "tick-batch" && reduction.update.frames.some((frame) =>
          frame.facts.some(({ event }) =>
            ("OrderAccepted" in event && String(event.OrderAccepted.account) === store.getState().remoteMembership.accountId)
            || ("OrderCanceled" in event && String(event.OrderCanceled.account) === store.getState().remoteMembership.accountId)
            || ("IntentRejected" in event && String(event.IntentRejected.account) === store.getState().remoteMembership.accountId)
            || ("Trade" in event && (String(event.Trade.maker) === store.getState().remoteMembership.accountId || String(event.Trade.taker) === store.getState().remoteMembership.accountId)),
          ),
        );
        if (playerOrderChanged) {
          clearCancelingOrderIds();
        }
        if (reduction.update.kind === "tick-batch") {
          const frames = reduction.update.frames;
          if (frames.some((frame) => playerOrderFactsRequireRefresh(frame.facts, store.getState().remoteMembership.accountId))) {
            void refreshPlayerOrders();
          }
        }
        const matchedBarrier = reduction.effects.find((effect) => effect.kind === "civil-barrier" && (
          (effect.barrier === "AfterClose" && pausePreferencesRef.current.pauseAfterClose)
          || (effect.barrier === "BeforeOpen" && pausePreferencesRef.current.pauseBeforeOpen)
        ));
        if (matchedBarrier !== undefined && matchedBarrier.kind === "civil-barrier") {
          store.dispatch(setRunning(false));
          setNotice(`已暂停：${matchedBarrier.message}，等待继续`);
        }
      },
      onFailure(failure) {
        fatalHostErrorRef.current(failure);
      },
    });
  }

  useSessionHostLifecycle({ hostRef, initialSaveSourceRef, dayEndPersistenceRef, autoOrderMgrRef, sessionReplacementGateRef,
    saveSelectionGenerationRef, playerOrderRefreshGateRef, hostUpdateRef, fatalHostErrorRef, stopStartupRef, returningToStartupRef,
    startupTarget, sessionSetup, consumeChosenSessionSeed: () => parseSessionSeed(chosenSessionSeed), setSeedDraft, speed, pauseAfterClose, pauseBeforeOpen, TRADING_E2E_MODE, pausePreferencesReady,
    malformedProtocolFixture: () => import.meta.env.DEV && new URLSearchParams(window.location.search).get("protocolFixture") === "malformed",
    setIndicatorCalculator, configureMarketTiming, setActiveSetup, setStartDateDraft, setPriceCageEnabledDraft, setFloatAllocationDraft, setReportFrequencyDraft, setCompanySystemDraft, setInitialAllocation, setDeliveryModes, setDeliveryModeState,
    setNotice, setReady, setError, setHostBaselineReady, refreshPlayerOrders, getBrowserSaveRepository, connectProtocol,
    disconnectProtocol() {
      quickTrading.reset();
      contextSubscriptionRef.current?.();
      contextSubscriptionRef.current = null;
      companyCoordinatorRef.current?.dispose();
      companyCoordinatorRef.current = null;
      protocolCoordinatorRef.current = null;
    },
    onRunning: (value) => { store.dispatch(setRunning(value)); },
    onAutoTriggered: (id) => { store.dispatch(markTriggered(id)); },
  });

  const pausePreferences = usePausePreferences({ hostRef, pauseAfterClose, pauseBeforeOpen,
    apply(preferences) {
      store.dispatch(setPauseAfterClose(preferences.pause_after_close));
      store.dispatch(setPauseBeforeOpen(preferences.pause_before_open));
    },
    onReady: () => setPausePreferencesReady(true),
    onError: setError,
  });

  useEffect(() => {
    setSpeedMetrics(null);
    setSpeedMetricsError(null);
  }, [running]);

  useSpeedMetricsPolling({ hostRef, ready, speed, running, pollingGeneration: speedMetricsPollingGeneration,
    requestGate: speedMetricsRequestGateRef.current, loadInProgress: speedMetricsLoadInProgressRef,
    onMetrics(metrics) {
      if (startupTarget.kind === "remote") {
        store.dispatch(setRunning(metrics.running));
        store.dispatch(setSpeed(metrics.requested.mode === "fastest" ? Infinity : metrics.requested.multiplier));
      }
      setSpeedMetrics(metrics);
    }, onError: setSpeedMetricsError });

  // 一个浏览器中可能同时打开多个游戏页。隐藏页继续以 720x/MAX 运算会与当前页
  // 抢占全部 CPU，并让可见页的 K 线看似停止；隐藏时暂停宿主，重新可见时按 UI
  // 的运行状态恢复。游戏状态仍保留在各自 Worker 中，不会重建或丢失。
  useEffect(() => {
    const syncHostVisibility = () => {
      if (hostRef.current) void sessionControls.syncVisibility(document.hidden);
    };
    document.addEventListener("visibilitychange", syncHostVisibility);
    return () => document.removeEventListener("visibilitychange", syncHostVisibility);
  }, [sessionControls, hostRef]);

  useEffect(() => {
    refreshDailyChart();
  }, [chartCode, refreshDailyChart]);

  const saveCommands = useSaveCommands({ hostRef, initialSaveSourceRef, dayEndPersistenceRef, autoOrderMgrRef,
    browserLocal: startupTarget.kind === "wasm",
    sessionReplacementGateRef, saveSelectionGenerationRef, dayEndFileTargetRef, playerOrderRefreshGateRef,
    speedMetricsLoadInProgressRef, speedMetricsRequestGateRef, fatalHostErrorRef,
    activeSetup, startDateDraft, priceCageEnabledDraft, floatAllocationDraft, reportFrequencyDraft, companySystemDraft, seedDraft: companyDraft.seed, setSeedDraft, loadFromFile, selectDayEndFileTarget, getBrowserSaveRepository,
    resetMarketHistory, configureMarketTiming, refreshPlayerOrders, clearPlayerOrders, setNotice, setError, setReady, setSessionCreation,
    setActiveSetup, setStartDateDraft, setPriceCageEnabledDraft, setFloatAllocationDraft, setReportFrequencyDraft, setCompanySystemDraft, setInitialAllocation, setStartDateError, setSpeedMetricsPollingGeneration,
    setSpeedMetrics, setSpeedMetricsError });
  const { recoverFromFile, noticeSavePolicy: handleSave, load: loadMarketArchive, selectFile: handleSaveFile,
    loadFile: loadMarketFile, newGame: resetMarket } = saveCommands;
  const handleLoad = (slotId?: string) => {
    if (!canControl) { setNotice("当前身份没有加载全市场日终存档的控制授权"); return Promise.resolve(); }
    if (slotId === undefined) {
      try {
        const host = hostRef.current;
        if (host === null) throw new Error("宿主尚未就绪");
        const repository = host.capabilities.persistence === "browser" ? getBrowserSaveRepository() : host.archiveStore;
        if (repository === undefined) throw new Error("宿主未提供存档管理接口");
        setManagedArchives(repository);
      } catch (failure) { setNotice(`打开存档管理失败：${failure instanceof Error ? failure.message : String(failure)}；请反馈错误。`); }
      return Promise.resolve();
    }
    if (membership.remote && !window.confirm("读档会替换全市场时间线，影响所有玩家；控制授权保留，缺席主体须本人明确重新加入才发一次资金。确认读取此档？")) return Promise.resolve();
    return loadMarketArchive(slotId);
  };
  const handleLoadFile = () => {
    if (!canControl) { setNotice("当前身份没有加载全市场日终存档的控制授权"); return; }
    if (membership.remote) setRemoteConfirmation("file");
    else void loadMarketFile();
  };
  const handleNewGame = () => {
    if (!canControl) { setNotice("当前身份没有重置全市场的控制授权"); return; }
    if (membership.remote) setRemoteConfirmation("reset");
    else void resetMarket();
  };
  async function confirmRemoteOperation() {
    const operation = remoteConfirmation;
    setRemoteConfirmation(null);
    try {
      if (operation === "reset") await resetMarket();
      if (operation === "load") await loadMarketArchive();
      if (operation === "file") await loadMarketFile();
      if (operation === "rejoin") {
        const host = hostRef.current;
        if (host?.rejoinMarket === undefined) throw new Error("当前宿主没有明确重新加入市场的接口");
        await host.rejoinMarket();
        setNotice("已由本人明确重新加入；Server 按本次入场规则发放一次资金，重复请求不重复发放。");
      }
      if (operation === "logout" && startupTarget.kind === "remote") {
        if (startupTarget.token === undefined) throw new Error("当前连接缺少身份 credential");
        const token = startupTarget.token;
        await logoutRemoteIdentity({
          revoke: () => new RemoteAuthClient(startupTarget.baseUrl).logout(token),
          forget: () => new IndexedDbCredentialStore().remove(startupTarget.baseUrl),
          disconnect: () => onSelectHost(stopStartupRef.current),
        });
      }
    } catch (failure) { setNotice(`远程操作失败：${failure instanceof Error ? failure.message : String(failure)}；请反馈错误。`); }
  }

  const handlePauseToggle = useCallback(() => {
    if (!selectCanControl(store.getState())) { setNotice("当前身份没有暂停或继续全市场的控制授权"); return; }
    void sessionControls.toggleRunning();
  }, [sessionControls, setNotice]);
  const handleSpeedChange = useCallback((value: number) => {
    if (!selectCanControl(store.getState())) { setNotice("当前身份没有修改全市场速度的控制授权"); return; }
    void sessionControls.setSpeed(value);
  }, [sessionControls, setNotice]);

  const handleDeliveryModeChange = useCallback((mode: DeliveryMode) => {
    const host = hostRef.current;
    if (!host) return;
    try {
      if (!host.capabilities.deliveryModes.includes(mode) || !host.setDeliveryMode) {
        throw new Error(`当前宿主不支持 ${mode} 刷新模式`);
      }
      host.setDeliveryMode(mode);
      setDeliveryModeState(mode);
      setNotice(`已切换为${DELIVERY_MODE_LABELS[mode]}`);
    } catch (deliveryError) {
      setNotice(`Publisher 模式切换失败：${deliveryError instanceof Error ? deliveryError.message : String(deliveryError)}`);
    }
  }, [setNotice, hostRef]);

  const queryCompanyReports = useCallback((companyId: string, cursor: string | null) => {
    void companyCoordinatorRef.current?.query({ companyId, cursor });
  }, []);

  const advanceCivilDay = useCallback(async () => {
    if (!store.getState().remoteMembership.canControl) { setNotice("当前身份没有推进全市场自然日的控制授权"); return; }
    const host = hostRef.current;
    if (!host?.endCivilDay) {
      setNotice("当前宿主不支持手动推进模拟自然日");
      return;
    }
    try {
      await host.endCivilDay();
    } catch (advanceError) {
      setNotice(`推进模拟自然日失败：${advanceError instanceof Error ? advanceError.message : String(advanceError)}`);
    }
  }, [setNotice, hostRef]);

  const selectHost = () => { void onSelectHost(stopStartupRef.current); };

  if (returningToStartup) {
    return <div className="app-loading"><p role="status" aria-live="polite">正在结束当前启动，等待已提交的日终存档写入完成；完成前不会启动另一局…</p></div>;
  }
  if (error) {
    return <>
      <FatalHostError error={error} onRetry={() => window.location.reload()} />
      <section aria-label="宿主启动恢复">
        <p>重新选择将结束当前内存会话，丢失未保存的日内进度；不会重读快速槽，也不会无缝迁移。</p>
        <Button onClick={selectHost}>返回启动选择</Button>
        {startupReturnError !== null && <p role="alert">{startupReturnError}</p>}
      </section>
      <section aria-label="存档错误恢复">
        <p>不会自动忽略坏档或改写原档；可明确创建新游戏，或选择另一份日终存档。</p>
        <Button onClick={handleNewGame}>创建新游戏</Button>
        <Button onClick={() => void recoverFromFile()}>选择其他日终存档</Button>
        {notice !== null && <p role="status">{notice}</p>}
      </section>
    </>;
  }
  if (!ready || !hostBaselineReady || !hasSnapshot) {
    return <div className="app-loading">
      <p role="status" aria-live="polite">正在加载行情引擎…</p>
      <Button onClick={selectHost}>取消加载，返回启动选择</Button>
      {startupReturnError !== null && <p role="alert">{startupReturnError}</p>}
    </div>;
  }

  const currentSpeedMetrics = speedMetrics && speedMetricsMatchesUiState(speedMetrics, speed, running)
    ? speedMetrics
    : null;
  const measuredSpeedText = speedMetricsError
    ? "实测不可用"
    : formatMeasuredSpeed(currentSpeedMetrics?.actual_multiplier ?? null);
  const measuredSpeedTitle = speedMetricsError
    ?? (currentSpeedMetrics && !currentSpeedMetrics.running
      ? "模拟已暂停；实际倍率为 0x"
      : currentSpeedMetrics?.actual_multiplier !== null && currentSpeedMetrics?.actual_multiplier !== undefined
        ? `最近 ${currentSpeedMetrics.sample_duration_ms}ms 推进 ${currentSpeedMetrics.sample_ticks} tick；实际倍率按 tick/现实秒计算`
        : "等待完成首个至少 500ms 的采样窗口");

  return (
    <QuickTradingContext.Provider value={{ trading: quickTrading, notice: setNotice, open: (code, side) => { setTradeCode(code); selectChart(code); setDesktopTradeSide(side); if (orientation === "portrait") openTradeSheet(); else changeDesktopTradingOpen(true); } }}>
    <div
      className={`app-root ${orientation === "portrait" ? "layout-mobile" : "layout-desktop"}`}
      data-theme={theme}
    >
      <ClockMarker />
      {/* 顶栏 */}
      <header className="top-bar">
        <div className="mobile-brand-bar">
          <button type="button" aria-label="打开我的与存档" onClick={() => switchMobileTab("user")}><span aria-hidden="true">☰</span></button>
          <ConnectedMobileGameClock />
          <strong>{mobilePrimaryTitle(mobileTab, securityBrowser.view)}</strong>
          <span className="mobile-head-tools">
            <MobileRunToggle disabled={!canControl} running={running} onToggle={handlePauseToggle} variant="global" />
            <MobileSpeedSelect
              disabled={!canControl}
              speed={speed}
              measuredSpeed={measuredSpeedText}
              measuredSpeedTitle={measuredSpeedTitle}
              onChange={handleSpeedChange}
            />
          </span>
        </div>
        <div className="brand"><span className="desktop-brand-mark" aria-hidden="true">行情</span>股票模拟<span className="desktop-brand-caption">交易终端</span></div>
        <div className="controls">
          <span className="label">速度</span>
          <HTMLSelect disabled={!canControl} className="speed-select" value={speed === Infinity ? "Infinity" : String(speed)} onChange={(e) => {
            const raw = e.target.value;
            const v = raw === "Infinity" ? Infinity : Number(raw);
            handleSpeedChange(v);
          }}            options={[
              { label: "1x", value: "1" },
              { label: "2x", value: "2" },
              { label: "3x", value: "3" },
              { label: "5x", value: "5" },
              { label: "10x", value: "10" },
              { label: "30x", value: "30" },
              { label: "60x", value: "60" },
              { label: "180x", value: "180" },
              { label: "360x", value: "360" },
              { label: "720x", value: "720" },
              { label: "MAX", value: "Infinity" },
            ]} />
          <output className={`speed-actual ${speedMetricsError ? "is-error" : ""}`} title={measuredSpeedTitle}>
            {measuredSpeedText}
          </output>
          <Button disabled={!canControl} className="simulation-button" intent={running ? "danger" : "success"} onClick={handlePauseToggle}>{running ? "暂停" : "继续"}</Button>
          <DesktopDayTag />
          <span className={`session-status ${running ? "is-running" : "is-paused"}`} aria-live="polite">
            <i aria-hidden="true" />{running ? "交易中" : "已暂停"}
          </span>
          <Button className="theme-toggle" minimal onClick={() => store.dispatch(setTheme(theme === "light" ? "dark" : "light"))} title="切换主题">{theme === "light" ? "🌙" : "☀️"}</Button>
          <Button minimal onClick={() => setDesktopView("settings")}>游戏与存档</Button>
        </div>
      </header>

      <WorkspaceGrid orientation={orientation} desktopView={desktopView} securityView={securityBrowser.view} onDesktopViewChange={setDesktopView} onTradeCurrent={() => openDesktopTrade()} desktopTradingOpen={desktopTradingOpen} onDesktopTradingOpenChange={changeDesktopTradingOpen} stockList={<ConnectedTerminalStockList browser={securityBrowser} onSelect={selectStock} />} data-mobile-tab={mobileTab} data-mobile-detail={mobileDetail ? "1" : "0"}>
        {/* 行情表（AG Grid） */}
        <Card className="panel market-panel" id="section-market" tabIndex={-1} aria-label="行情列表">
          <h3 className="panel-title">行情</h3>
          <ConnectedMarketPanel browser={securityBrowser} onSelect={selectStock} onOpen={(code) => { selectStock(code); setDesktopView("stock"); }} />
        </Card>

        {/* 分时走势图 + 股票详情头 + 盘口 */}
        <Card className="panel chart-panel" id="section-trade">
          <ConnectedChartPanel browser={securityBrowser} chartPeriod={chartPeriod} setChartPeriod={(period) => dispatchMobileUi({ type: "select-period", period })} klineDays={klineDays} onTrade={openDesktopTrade} />
        </Card>

        <Card className="panel company-panel-shell" id="section-company">
          <h3 className="panel-title">公司信息</h3>
          <ConnectedCompanyPanel stockContext={orientation === "landscape"} initialCivilDate={activeSetup.start_date} onCompanyQuery={queryCompanyReports} onAdvanceCivilDay={advanceCivilDay} reportCorrectionControl={canControl ? hostRef.current : null} timelineGeneration={snapshotGeneration} />
        </Card>

        {/* 委托面板 + 自动单（移动端为底页弹出） */}
        <Card
          ref={tradeSheetRef}
          className={`panel order-panel ${orientation === "portrait" ? "mobile-sheet" : ""} ${tradeSheetOpen ? "sheet-open" : ""}`}
          id="section-order"
          data-trade-side={orientation === "landscape" ? desktopTradeSide : undefined}
          role={orientation === "portrait" && tradeSheetOpen ? "dialog" : undefined}
          aria-modal={orientation === "portrait" && tradeSheetOpen ? true : undefined}
          aria-hidden={orientation === "portrait" && !tradeSheetOpen ? true : undefined}
          hidden={orientation === "portrait" && !tradeSheetOpen}
          aria-label={orientation === "portrait" ? "交易面板" : undefined}
        >
          <h3 className="panel-title">委托下单</h3>
          <QuickTradingPanel trading={quickTrading} code={tradeCode} setup={activeSetup} side={desktopTradeSide} onSideChange={setDesktopTradeSide} onSelect={(code) => { setTradeCode(code); selectChart(code); }} notice={setNotice} />

          <section className="player-orders" aria-labelledby="player-orders-title">
            <h4 id="player-orders-title" className="auto-title">当前活动委托 <Button small disabled={cancelAllPending} loading={cancelAllPending} onClick={() => void cancelAllOrders()}>取消所有</Button></h4><p className="trade-timing-note">撤销本人全部股票的买卖挂单；自动保持开启。撤单结果以引擎受理为准。</p>
            {playerOrders.length === 0 ? (
              <p className="player-orders-empty">暂无活动委托；限价单未成交时会显示在这里。</p>
            ) : (
              <ul className="player-order-list">
                {playerOrders.map((order) => (
                  <li key={`${order.code}-${order.id}`} className="player-order-item" data-order-venue={order.venue}>
                    <span className="player-order-id">#{order.id}</span>
                    <span className={order.side === "Buy" ? "up" : "down"}>{order.side === "Buy" ? "买" : "卖"}</span>
                    <span className="mono">{order.code}</span>
                    <span className="num">{yuan(order.price)} 元 · {order.remainingQty} 股</span>
                    <span className="player-order-freeze">{order.frozen === "cash" ? "资金已冻结" : "股份已冻结"}</span>
                    <Button small minimal intent="warning" loading={cancelingOrderIds.has(order.id)} disabled={cancelingOrderIds.has(order.id)} onClick={() => void cancelPlayerOrder(order)}>撤单</Button>
                  </li>
                ))}
              </ul>
            )}
          </section>

          {/* 条件单 */}
          <div className="auto-order-section">
            <h4 className="auto-title">条件单</h4>
            <div className="auto-form">
              <HTMLSelect value={autoType} onChange={(e) => setAutoType(e.target.value as AutoOrderType)}
                options={(Object.keys(AUTO_ORDER_LABELS) as AutoOrderType[]).map((t) => ({ label: AUTO_ORDER_LABELS[t], value: t }))} />
              <label><span className="sr-only">条件单触发价（元）</span><input className="auto-input" type="text" value={autoTrigger} aria-invalid={Boolean(autoFieldErrors.price)} aria-describedby={autoFieldErrors.price ? "auto-price-error" : undefined} onChange={(e) => setAutoTrigger(e.target.value)} placeholder="触发价（元）" />{autoFieldErrors.price && <span id="auto-price-error" className="field-error" role="alert">{autoFieldErrors.price}</span>}</label>
              <label><span className="sr-only">条件单数量（股）</span><input className="auto-input" type="text" value={autoQty} aria-invalid={Boolean(autoFieldErrors.quantity)} aria-describedby={autoFieldErrors.quantity ? "auto-quantity-error" : undefined} onChange={(e) => setAutoQty(e.target.value)} placeholder="数量（股）" />{autoFieldErrors.quantity && <span id="auto-quantity-error" className="field-error" role="alert">{autoFieldErrors.quantity}</span>}</label>
              <Button disabled={membership.accountId === null} small intent="primary" onClick={addAuto}>添加</Button>
            </div>
            {autoFieldErrors.code && <p className="field-error" role="alert">{autoFieldErrors.code}</p>}
            <div className="auto-list">
              {autoOrders.length === 0 && <span className="auto-empty">暂无条件单</span>}
              {autoOrders.map((o) => (
                <div key={o.id} className={`auto-item ${o.triggered ? "triggered" : ""} ${!o.enabled ? "disabled" : ""}`}>
                  <Switch checked={o.enabled} onChange={() => { store.dispatch(toggleAutoOrder(o.id)); autoOrderMgrRef.current?.toggle(o.id); }} />
                  <span>{AUTO_ORDER_LABELS[o.type]}</span>
                  <span className="mono">{o.code}</span>
                  <span className="num">{yuan(o.triggerPrice)} 元</span>
                  <span className="num">{o.qty} 股</span>
                  {o.triggered && <span className="triggered-tag">已触发</span>}
                  <Button small minimal intent="danger" onClick={() => { store.dispatch(removeAutoOrder(o.id)); autoOrderMgrRef.current?.remove(o.id); }}>删除</Button>
                </div>
              ))}
            </div>
            {autoOrders.some((o) => o.triggered) && (
              <Button small minimal onClick={() => {
                autoOrderMgrRef.current?.clearTriggered();
                store.dispatch(clearTriggeredOrders());
              }}>清除已触发</Button>
            )}
          </div>

          {/* 移动端底页关闭按钮 */}
          {orientation === "portrait" && (
            <button className="sheet-close" type="button" onClick={closeTradeSheet}>收起交易面板</button>
          )}
        </Card>

        {/* 持仓 */}
        <Card className="panel pos-panel" id="section-positions">
          <h3 className="panel-title">持仓</h3>
          <PositionsPanel onOpenMarket={() => orientation === "landscape" ? setDesktopView("quotes") : switchMobileTab("market")} />
        </Card>

        {/* 分时成交 */}
        <Card className="panel trades-panel" id="section-trades">
          <h3 className="panel-title">分时成交</h3>
          <TradesPanel />
        </Card>

        <Card className="panel user-panel" id="section-user">
          <h3 className="panel-title">我的</h3>
          {membership.remote && <section aria-label="登录身份与市场账户">
            <p>登录身份与日终经济存档独立；暂停、设置、重置与读档影响全市场，控制授权不来自资金账户。</p>
            {membership.accountId === null && <p>本时间线没有本人资金账户，金融资产和委托为空；{membership.canControl ? "市场控制授权仍然有效。" : "可继续查看公开市场。"}</p>}
            {membership.accountId === null && <Button onClick={() => setRemoteConfirmation("rejoin")}>{membership.needsRejoin ? "明确重新加入并领取一次入场资金" : "明确加入公开市场"}</Button>}
            <Button onClick={() => setRemoteConfirmation("logout")}>退出登录（不删除市场或持仓）</Button>
            {canControl && <fieldset aria-label="市场入场资金设置"><legend>之后入场玩家的初始资金</legend>
              <p>只影响之后明确加入的资金账户，不重置已有余额。默认 100 亿元（1,000,000,000,000 分）。</p>
              <label>入场资金（分）<input inputMode="numeric" value={admissionCashDraft} onChange={(event) => setAdmissionCashDraft(event.currentTarget.value)} /></label>
              <Button onClick={() => {
                const host = hostRef.current;
                if (host?.setAdmissionCash === undefined) { setNotice("当前宿主没有入场资金设置接口"); return; }
                void host.setAdmissionCash(admissionCashDraft).then(() => setNotice("已修改之后入场玩家的资金设置；已有账户余额不变。"), (failure) => setNotice(`入场资金设置失败：${failure instanceof Error ? failure.message : String(failure)}`));
              }}>应用全市场入场资金设置</Button>
            </fieldset>}
          </section>}
          <fieldset disabled={!canControl} className="new-game-panel" aria-label="新游戏">
            <h4>新游戏</h4>
            <StartDateInput value={startDateDraft} error={startDateError} onChange={(value) => { setStartDateDraft(value); setStartDateError(null); }} />
            <PriceCageInput enabled={priceCageEnabledDraft} onChange={setPriceCageEnabledDraft} />
            <FloatAllocationInput value={floatAllocationDraft} onChange={setFloatAllocationDraft} />
            <ReportFrequencyInput value={reportFrequencyDraft} onChange={setReportFrequencyDraft} />
            <CompanySystemInput value={companySystemDraft} onChange={(companySystem) => setCompanyDraft(draft => ({ ...draft, companySystem, origin: "custom" }))} seed={companyDraft.seed} origin={companyDraft.origin} onSeedChange={changeCompanySeed} onRegenerate={regenerateCompanyDraft} onSettlementCycleChange={changeCompanySettlementCycle} />
            <InitialAllocationSummary allocation={initialAllocation} />
            <Button onClick={handleNewGame}>创建新游戏</Button>
          </fieldset>
          <UserPanel running={running} pauseAfterClose={pauseAfterClose} pauseBeforeOpen={pauseBeforeOpen} pausePreferencesPending={pausePreferences.pending} deliveryMode={deliveryMode} deliveryModes={deliveryModes} deliveryLabels={DELIVERY_MODE_LABELS} onPauseAfterCloseChange={(value) => void pausePreferences.changePreferences({ pause_after_close: value, pause_before_open: pauseBeforeOpen })} onPauseBeforeOpenChange={(value) => void pausePreferences.changePreferences({ pause_after_close: pauseAfterClose, pause_before_open: value })} onDeliveryModeChange={handleDeliveryModeChange} onSave={() => void handleSave()} onLoad={() => void handleLoad()} onSaveFile={() => void handleSaveFile()} onLoadFile={() => void handleLoadFile()} />
        </Card>
      </WorkspaceGrid>
      {orientation === "landscape" && <footer className="desktop-account-bar" aria-label="模拟账户摘要">
        <span className="desktop-account-label">模拟账户</span>
        <DesktopAssets />
        <span className="desktop-account-note">行情量：手 · 委托量：股</span>
      </footer>}

      {DevNpcInspector !== null && hostRef.current?.capabilities.npcDecisionDiagnostics === true && <section>
        <Button disabled={hostRef.current?.capabilities.npcDecisionDiagnostics !== true}
          title="仅当当前后端以诊断 feature 的 debug 构建明确启用时可用"
          onClick={() => setShowNpcInspector((shown) => !shown)}>当前局 NPC 诊断</Button>
        {showNpcInspector && hostRef.current !== null && <Suspense fallback={<p role="status">正在加载当前局诊断视图…</p>}>
          <DevNpcInspector key={snapshotGeneration ?? "pending"} host={hostRef.current} timelineGeneration={snapshotGeneration} />
        </Suspense>}
      </section>}

      {/* 移动端浮动交易按钮（贴 ref .ctrl-btn） */}
      {orientation === "portrait" && (
        <>
        <MobileDetailLayer ui={mobileUi}>
            <ConnectedMobileDetail browser={securityBrowser} klineDays={klineDays} setKlineDays={setKlineDays} period={mobileUi.chartPeriod} infoTab={mobileUi.infoTab} speed={speed} measuredSpeed={measuredSpeedText} measuredSpeedTitle={measuredSpeedTitle} running={running} initialCivilDate={activeSetup.start_date} onCompanyQuery={queryCompanyReports} onAdvanceCivilDay={advanceCivilDay} reportCorrectionControl={canControl ? hostRef.current : null} timelineGeneration={snapshotGeneration} onPeriodChange={(period) => dispatchMobileUi({ type: "select-period", period })} onInfoTabChange={showDetailInfo} onSpeedChange={handleSpeedChange} onPauseToggle={handlePauseToggle} onBack={() => dispatchMobileUi({ type: "back" })} onSelect={selectStock} />
        </MobileDetailLayer>
        <nav className="mobile-tabbar mobile-main-tabbar" aria-label="主导航">
          {MOBILE_PRIMARY_NAV.map(([tab, label]) => (
            <button key={tab} type="button" aria-current={mobileTab === tab ? "page" : undefined} aria-expanded={tab === "trades" ? tradeSheetOpen : undefined} aria-controls={tab === "trades" ? "section-order" : undefined} className={`tab-btn ${mobileTab === tab ? "active" : ""}`} onClick={() => {
              if (tab === "trades") openTradeSheet();
              else switchMobileTab(tab);
            }}>
              <span className={`tab-icon tab-icon-${tab}`} aria-hidden="true" />
              <span>{label}</span>
            </button>
          ))}
        </nav>
        {/* 底页遮罩 */}
        {tradeSheetOpen && <button className="sheet-mask" type="button" aria-label="关闭交易面板" onClick={closeTradeSheet} />}
        </>
      )}
      <SpeedMetricsAlert error={speedMetricsError} />
      {managedArchives !== null && <ArchiveManager repository={managedArchives} onLoad={async (slotId) => { await handleLoad(slotId); }} onClose={() => setManagedArchives(null)} />}
      {remoteConfirmation !== null && <section role="dialog" aria-modal="true" aria-label="确认远程操作">
        <p>{remoteConfirmation === "logout" ? "只撤销当前登录凭据并断开连接，不删除市场或持仓。匿名身份退出后不可找回，请确认。" : remoteConfirmation === "rejoin" ? "本人确认在当前市场建立资金账户，按当前设置发放一次入场资金，不恢复旧存档之外的资产。" : "此操作改变整个共享市场，所有玩家切换到同一时间线；旧委托和异步请求失效。控制授权保留，缺席玩家须本人明确重新加入，不自动发资金。"}</p>
        <Button onClick={() => void confirmRemoteOperation()}>确认</Button><Button onClick={() => setRemoteConfirmation(null)}>取消</Button>
      </section>}
      {notice && <div className="notice" role="status" aria-live="polite">{notice}</div>}
    </div>
    </QuickTradingContext.Provider>
  );
}

function App() {
  const initialSaveSourceRef = useRef(new InitialSaveSource<StrictSaveEnvelope>());
  const dayEndPersistenceRef = useRef(new DayEndPersistence());
  const [sessionCreation, setSessionCreationState] = useState(() => ({ setup: INITIAL_SEED_PREVIEW.draft === null ? INITIAL_SESSION_SETUP : { ...INITIAL_SESSION_SETUP, company_system: parseCompanySystemConfig(JSON.parse(INITIAL_SEED_PREVIEW.draft.companySystem)) }, draft: INITIAL_SEED_PREVIEW.draft }));
  const sessionSetup = sessionCreation.setup;
  const [seedPreviewError, setSeedPreviewError] = useState(INITIAL_SEED_PREVIEW.error);
  const setSessionCreation = (setup: SessionSetup, seed: string) => setSessionCreationState({ setup, draft: { seed, origin: "custom", companySystem: JSON.stringify(setup.company_system, null, 2) } });
  const returningToStartupRef = useRef(false);
  const [returningToStartup, setReturningToStartup] = useState(false);
  const [startupReturnError, setStartupReturnError] = useState<string | null>(null);
  const [startupTarget, setStartupTarget] = useState<StartupTarget | null>(() => initialStartupTarget(import.meta.env.MODE));
  const [startupMode, setStartupMode] = useState<StartupMode>(import.meta.env.DEV && import.meta.env.VITE_ENGINE_HOST === "remote" ? "remote" : "local");
  const [remoteAddress, setRemoteAddress] = useState(import.meta.env.DEV ? import.meta.env.VITE_REMOTE_BASE_URL ?? "" : "");
  const [startupError, setStartupError] = useState<string | null>(null);
  const [remoteLoginAddress, setRemoteLoginAddress] = useState<string | null>(null);
  const autoOrderMgrRef = useRef<AutoOrderManager | null>(null);
  const hostRef = useRef<EngineHost | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const regenerateStartupPreview = () => {
    try {
      const draft = sessionCreation.draft === null
        ? createSeedDraft(sessionSetup, TRADING_E2E_MODE ? DEFAULT_SEED : createNewSessionSeed())
        : regenerateSeedDraft(sessionSetup, sessionCreation.draft);
      setSessionCreationState({ setup: sessionSetup, draft });
      setSeedPreviewError(null);
    } catch (failure) { setSeedPreviewError(`重新生成预览失败：${failure instanceof Error ? failure.message : String(failure)}`); }
  };
  const startupCompanySettings = sessionCreation.draft === null ? null : <CompanySystemInput
    value={sessionCreation.draft.companySystem} seed={sessionCreation.draft.seed} origin={sessionCreation.draft.origin}
    onChange={(companySystem) => setSessionCreationState(current => ({ ...current, draft: { ...current.draft!, companySystem, origin: "custom" } }))}
    onSeedChange={(seed) => {
      try { setSessionCreationState({ setup: sessionSetup, draft: changeSeedDraft(sessionSetup, sessionCreation.draft!, seed) }); setSeedPreviewError(null); }
      catch (failure) { setSeedPreviewError(`更新预览失败：${failure instanceof Error ? failure.message : String(failure)}`); }
    }} onSettlementCycleChange={(cycle) => {
      try { setSessionCreationState({ setup: sessionSetup, draft: changeSettlementCycleDraft(sessionSetup, sessionCreation.draft!, cycle) }); setSeedPreviewError(null); }
      catch (failure) { setSeedPreviewError(`更新结算周期失败：${failure instanceof Error ? failure.message : String(failure)}；当前配置未改变。`); }
    }} onRegenerate={regenerateStartupPreview} />;
  const returnToStartup = useCallback(async (stopSession: () => Promise<void>) => {
    if (returningToStartupRef.current) return;
    returningToStartupRef.current = true;
    setReturningToStartup(true);
    setStartupReturnError(null);
    try {
      await stopSession();
      store.dispatch(setRunning(false));
      dayEndPersistenceRef.current.invalidate();
      await dayEndPersistenceRef.current.idle();
      store.dispatch(clearAutoOrders());
      setNotice(null);
      setStartupTarget(null);
    } catch (failure) {
      setStartupReturnError(`返回启动选择失败：${failure instanceof Error ? failure.message : String(failure)}；尚未开始下一局，请反馈此错误或重新打开应用。`);
    } finally {
      returningToStartupRef.current = false;
      setReturningToStartup(false);
    }
  }, []);
  if (sessionCreation.draft === null) return <main className="app-error"><h1>新局预览尚未建立</h1><p role="alert">{seedPreviewError}</p><button type="button" onClick={regenerateStartupPreview}>重新生成新局预览</button></main>;
  if (startupTarget === null) {
    if (remoteLoginAddress !== null) return <RemoteLoginScreen baseUrl={remoteLoginAddress} setup={sessionSetup} seed={sessionCreation.draft.seed} companySystemDraft={sessionCreation.draft.companySystem} creationSettings={startupCompanySettings} onBack={() => setRemoteLoginAddress(null)} onConnected={(token, context) => {
      setRemoteLoginAddress(null);
      setStartupTarget({ kind: "remote", baseUrl: remoteLoginAddress, token, context });
    }} />;
    return <StartupScreen mode={startupMode} remoteAddress={remoteAddress} error={seedPreviewError ?? startupError ?? notice} creationSettings={startupCompanySettings}
      developmentHint={import.meta.env.DEV && (import.meta.env.VITE_ENGINE_HOST !== undefined || import.meta.env.VITE_REMOTE_BASE_URL !== undefined)}
      onModeChange={(mode) => { setStartupMode(mode); setStartupError(null); }}
      onAddressChange={(address) => { setRemoteAddress(address); setStartupError(null); }}
      onStart={() => {
        try {
          const target = resolveStartupTarget(startupMode, remoteAddress, "__TAURI_INTERNALS__" in window, browserWasmEnvironment);
          setStartupError(null);
          if (target.kind === "remote") setRemoteLoginAddress(target.baseUrl);
          else {
            parseSessionSeed(sessionCreation.draft!.seed);
            const setup = { ...sessionSetup, company_system: parseCompanySystemConfig(JSON.parse(sessionCreation.draft!.companySystem)) };
            setSessionCreationState({ setup, draft: sessionCreation.draft });
            setStartupTarget(target);
          }
        } catch (startupFailure) {
          setStartupError(startupFailure instanceof Error ? startupFailure.message : String(startupFailure));
        }
      }} />;
  }
  return (
    <MarketRuntimeProvider autoOrderManagerRef={autoOrderMgrRef} setNotice={setNotice} hostRef={hostRef}>
      <AppShell startupTarget={startupTarget} initialSaveSourceRef={initialSaveSourceRef} onSelectHost={returnToStartup}
        dayEndPersistenceRef={dayEndPersistenceRef} sessionSetup={sessionSetup} chosenSessionSeed={sessionCreation.draft.seed} chosenConfigOrigin={sessionCreation.draft.origin} setSessionCreation={setSessionCreation}
        returningToStartup={returningToStartup} returningToStartupRef={returningToStartupRef} startupReturnError={startupReturnError}
        autoOrderMgrRef={autoOrderMgrRef} hostRef={hostRef} notice={notice} setNotice={setNotice} />
    </MarketRuntimeProvider>
  );
}

export default App;
