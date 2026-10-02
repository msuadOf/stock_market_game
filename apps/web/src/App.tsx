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
import { useEffect, useRef, useState, useCallback, useMemo, lazy, Suspense, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import { Button, Card, InputGroup, HTMLSelect, Switch } from "@blueprintjs/core";
import { useSelector } from "react-redux";
import type { DeliveryMode, EngineHost, SpeedMetrics } from "./host/engine-host";
import type { HostFailure, HostUpdate } from "./host/host-update.ts";
import { createProtocolUpdate } from "./host/host-update.ts";
import { createTauriHost } from "./host/tauri-host";
import { createRemoteHost } from "./host/remote-host";
import { createWorkerHost, type WorkerE2EHost } from "./host/worker-host";
import { CompanyQueryCoordinator } from "./host/company-query-coordinator.ts";
import { ProtocolCoordinator } from "./host/protocol-coordinator.ts";
import { SpeedMetricsRequestGate, speedMetricsMatchesUiState } from "./host/speed";
import { assertWasmEnvironment, browserWasmEnvironment, initialStartupTarget, resolveStartupTarget, fatalDesktopInitializationMessage, fatalRemoteInitializationMessage, fatalWasmInitializationMessage, type StartupMode, type StartupTarget } from "./host/startup-policy";
import { DEFAULT_SEED, DEFAULT_SETUP, STOCK_LIST } from "./config/defaults";
import { loadPausePreferences, savePausePreferences } from "./config/pause-preferences.ts";
import { StartDateInput } from "./components/StartDateInput.tsx";
import { PriceCageInput } from "./components/PriceCageInput.tsx";
import { DeliveryModeControl, FatalHostError, SpeedMetricsAlert } from "./app/HostStatusViews.tsx";
import { StartupScreen } from "./app/StartupScreen.tsx";
import { WorkspaceGrid } from "./app/WorkspaceGrid.tsx";
import { parseStartDate, setupWithStartDate } from "./components/start-date.ts";
import type { Intent, SessionSetup } from "./types/engine";
import {
  setRunning,
  setSpeed,
  setTheme,
  setPauseAfterClose,
  setPauseBeforeOpen,
  store,
  addAutoOrder,
  removeAutoOrder,
  toggleAutoOrder,
  markTriggered,
  clearTriggeredOrders,
  clearAutoOrders,
  type RootState,
} from "./store/store";
import "./App.css";
import "ag-grid-community/styles/ag-grid.css";
import "ag-grid-community/styles/ag-theme-alpine.css";
import { AutoOrderManager, AUTO_ORDER_LABELS, type AutoOrderType } from "./components/auto-order-manager";
import { useOrientation } from "./hooks/useOrientation";
import { selectDayEndFileTarget, loadFromFile, type DayEndFileTarget } from "./save/save-file";
import { DayEndPersistence } from "./save/day-end-persistence.ts";
import { validateDayEndArchive, validateDayEndCandidate } from "./save/day-end-candidate.ts";
import type { StrictSaveEnvelope } from "./save/schema/root.ts";
import { writeDayEndTargets } from "./save/day-end-targets.ts";
import { InitialSaveSource, SessionReplacementGate, synchronizeCurrentBaseline } from "./save/session-replacement.ts";
import { CompressedLocalStorageSaveRepository } from "./save/save-repository";
import { PlayerOrderRefreshGate, playerOrderFactsRequireRefresh, type PlayerWorkingOrder } from "./components/player-orders.ts";
import { MobileSpeedSelect } from "./mobile/MobileSpeedSelect";
import { MobileRunToggle } from "./mobile/MobileRunToggle";
import { MOBILE_PRIMARY_NAV, formatMeasuredSpeed, mobilePrimaryTitle } from "./mobile/mobile-ui-state";
import { yuan } from "./utils/format";
import {
  maxAShareOrderQuantity,
  parseShareQuantity,
  parseYuanPrice,
  validateAShareQuantity,
} from "./utils/trade-input";
import { buildPlayerOrderIntent, orderPriceInputState, playerOrderDescription, type LimitPriceChoice } from "./utils/symbolic-limit-order.ts";
import { useMobileUiController } from "./app/useMobileUiController";
import { MarketRuntimeProvider, useMarketRuntimeActions, useMarketRuntimeSelection } from "./app/MarketRuntimeProvider.tsx";
import {
  ClockMarker,
  ConnectedChartPanel,
  ConnectedCompanyPanel,
  ConnectedMarketPanel,
  ConnectedMobileDetail,
  ConnectedMobileGameClock,
  DesktopAssets,
  DesktopDayTag,
  PositionsPanel,
  TradeMarketControls,
  TradesPanel,
  UserPanel,
} from "./app/LocalRefreshViews.tsx";

const PLAYER_ACCOUNT_KEY = "0";
const DevNpcInspector = import.meta.env.DEV ? lazy(() => import("./dev/NpcDecisionInspector.tsx").then((module) => ({ default: module.NpcDecisionInspector }))) : null;
const MAX_DAILY_CANDLES = 360;
const TRADING_E2E_MODE = import.meta.env.MODE === "e2e"
  && new URLSearchParams(window.location.search).get("tradingE2E") === "1";
const INITIAL_SESSION_SETUP: SessionSetup = TRADING_E2E_MODE
  ? {
      ...DEFAULT_SETUP,
      npcs: { ...DEFAULT_SETUP.npcs, retail_count: 0, inst_count: 0, hot_count: 0 },
      float_allocation: "Random",
      ticks_per_day: 30,
      auction_ticks: 9,
      closing_auction_ticks: 3,
    }
  : DEFAULT_SETUP;
const DELIVERY_MODE_LABELS: Record<DeliveryMode, string> = {
  push: "服务端推送 60Hz",
  pull: "客户端拉取 60Hz",
};
let browserSaveRepository: CompressedLocalStorageSaveRepository | null = null;

declare global {
  interface Window {
    __STOCK_GAME_E2E__?: {
      pause(): void;
      advanceToTick(target: number): Promise<number>;
      snapshot(): ReturnType<EngineHost["snapshot"]>;
    };
  }
}

function getBrowserSaveRepository(): CompressedLocalStorageSaveRepository {
  if (typeof window === "undefined") throw new Error("浏览器存储在当前运行环境不可用");
  browserSaveRepository ??= new CompressedLocalStorageSaveRepository(window.localStorage);
  return browserSaveRepository;
}

interface AppShellProps {
  startupTarget: StartupTarget;
  initialSaveSourceRef: MutableRefObject<InitialSaveSource<StrictSaveEnvelope>>;
  dayEndPersistenceRef: MutableRefObject<DayEndPersistence>;
  sessionSetup: SessionSetup;
  setSessionSetup: Dispatch<SetStateAction<SessionSetup>>;
  returningToStartup: boolean;
  returningToStartupRef: MutableRefObject<boolean>;
  startupReturnError: string | null;
  onSelectHost: (stopSession: () => void) => Promise<void>;
  autoOrderMgrRef: MutableRefObject<AutoOrderManager | null>;
  notice: string | null;
  setNotice: Dispatch<SetStateAction<string | null>>;
}

function AppShell({ startupTarget, initialSaveSourceRef, dayEndPersistenceRef, sessionSetup, setSessionSetup, returningToStartup, returningToStartupRef, startupReturnError, onSelectHost, autoOrderMgrRef, notice, setNotice }: AppShellProps) {
  const hasSnapshot = useSelector((s: RootState) => s.snapshot.snapshot !== null);
  const snapshotGeneration = useSelector((state: RootState) => state.snapshot.generation);
  const playerAccount = useSelector((s: RootState) => s.snapshot.snapshot?.accounts[PLAYER_ACCOUNT_KEY] ?? null);
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
  const [startDateError, setStartDateError] = useState<string | null>(null);
  const [speedMetrics, setSpeedMetrics] = useState<SpeedMetrics | null>(null);
  const [speedMetricsError, setSpeedMetricsError] = useState<string | null>(null);
  const [speedMetricsPollingGeneration, setSpeedMetricsPollingGeneration] = useState(0);
  const [deliveryMode, setDeliveryModeState] = useState<DeliveryMode | null>(null);
  const [deliveryModes, setDeliveryModes] = useState<readonly DeliveryMode[]>([]);
  const [pausePreferencesReady, setPausePreferencesReady] = useState(false);
  const pausePreferencesLoadedRef = useRef(false);
  const speedMetricsRequestGateRef = useRef(new SpeedMetricsRequestGate());
  const speedMetricsLoadInProgressRef = useRef(false);
  const playerOrderRefreshGateRef = useRef(new PlayerOrderRefreshGate());
  const pausePreferencesRef = useRef({ pauseAfterClose, pauseBeforeOpen });
  pausePreferencesRef.current = { pauseAfterClose, pauseBeforeOpen };
  const hostRef = useRef<EngineHost | null>(null);
  const stopStartupRef = useRef<() => void>(() => {});
  const dayEndFileTargetRef = useRef<DayEndFileTarget | null>(null);
  const saveSelectionGenerationRef = useRef(0);
  const sessionReplacementGateRef = useRef(new SessionReplacementGate());
  const companyCoordinatorRef = useRef<CompanyQueryCoordinator | null>(null);
  const protocolCoordinatorRef = useRef<ProtocolCoordinator | null>(null);
  const fatalHostErrorRef = useRef<(failure: string | HostFailure) => void>(() => {});
  fatalHostErrorRef.current = (failure) => {
    store.dispatch(setRunning(false));
    setError(failure);
  };
  const {
    mobileUi,
    dispatchMobileUi,
    mobileTab,
    tradeSheetOpen,
    mobileDetail,
    tradeSheetRef,
    switchMobileTab,
    openTradeSheet,
    closeTradeSheet,
    showDetailInfo,
  } = useMobileUiController(orientation);
  const chartCode = useMarketRuntimeSelection();
  const { acceptReduction, installBaseline, selectChart, resetMarketHistory, refreshDailyChart, setIndicatorCalculator } = useMarketRuntimeActions();
  const acceptReductionRef = useRef(acceptReduction);
  acceptReductionRef.current = acceptReduction;
  const installBaselineRef = useRef(installBaseline);
  installBaselineRef.current = installBaseline;
  const hostUpdateRef = useRef<(update: HostUpdate) => void>(() => {});
  hostUpdateRef.current = (update) => {
    protocolCoordinatorRef.current?.accept(update);
  };
  const runningRef = useRef(running);
  runningRef.current = running;
  const [chartPeriod, setChartPeriod] = useState<"分时" | "日K">("分时");
  const [klineDays, setKlineDays] = useState<number>(MAX_DAILY_CANDLES);

  function selectStock(code: string) {
    selectChart(code);
    setTradeCode(code);
    const market = store.getState().snapshot.snapshot?.markets[code];
    if (market) setPriceText(yuan(market.last_price));
    if (orientation === "portrait") dispatchMobileUi({ type: "open-detail", code });
  }

  // 委托面板状态
  const [tradeCode, setTradeCode] = useState<string>(STOCK_LIST[0].code);
  const [orderKind, setOrderKind] = useState<"limit" | "market">("limit");
  const [priceChoice, setPriceChoice] = useState<LimitPriceChoice>("fixed");
  const [priceText, setPriceText] = useState<string>("");
  const [qtyText, setQtyText] = useState<string>("100");
  const [queriedPlayerOrders, setPlayerOrders] = useState<readonly PlayerWorkingOrder[]>([]);
  const protocolPlayerOrders = useSelector((state: RootState) => state.snapshot.playerWorkingOrders);
  const playerOrdersReady = useSelector((state: RootState) => state.snapshot.playerOrdersReady);
  const playerOrders = useMemo(() => playerOrdersReady
    ? Object.values(protocolPlayerOrders).sort((left, right) => left.id - right.id)
    : queriedPlayerOrders, [playerOrdersReady, protocolPlayerOrders, queriedPlayerOrders]);
  const [cancelingOrderIds, setCancelingOrderIds] = useState<ReadonlySet<number>>(new Set());

  const refreshPlayerOrders = useCallback(async () => {
    const host = hostRef.current;
    if (!host) return;
    if (store.getState().snapshot.playerOrdersReady) return;
    const generation = playerOrderRefreshGateRef.current.next();
    try {
      const orders = await host.playerWorkingOrders();
      if (playerOrderRefreshGateRef.current.isCurrent(generation) && host === hostRef.current) {
        setPlayerOrders(orders);
      }
    } catch (refreshError) {
      if (playerOrderRefreshGateRef.current.isCurrent(generation) && host === hostRef.current) {
        setNotice(`活动委托刷新失败：${refreshError instanceof Error ? refreshError.message : String(refreshError)}`);
      }
    }
  }, [setNotice]);

  // 自动单添加表单状态
  const [autoType, setAutoType] = useState<AutoOrderType>("stopProfit");
  const [autoTrigger, setAutoTrigger] = useState<string>("");
  const [autoQty, setAutoQty] = useState<string>("100");

  useEffect(() => {
    let cancelled = false;
    let ownedHost: EngineHost | null = null;
    let unsetIndicatorCalculator: (() => void) | null = null;
    const releaseOwnedHost = () => {
      unsetIndicatorCalculator?.();
      unsetIndicatorCalculator = null;
      if (hostRef.current === ownedHost) hostRef.current = null;
      companyCoordinatorRef.current?.dispose();
      companyCoordinatorRef.current = null;
      protocolCoordinatorRef.current = null;
      autoOrderMgrRef.current?.clear();
      autoOrderMgrRef.current = null;
      const host = ownedHost;
      ownedHost = null;
      host?.dispose();
    };
    stopStartupRef.current = () => {
      cancelled = true;
      saveSelectionGenerationRef.current += 1;
      sessionReplacementGateRef.current.invalidate();
      playerOrderRefreshGateRef.current.invalidate();
      try { ownedHost?.stop(); } finally { releaseOwnedHost(); }
    };
    const playerOrderRefreshGate = playerOrderRefreshGateRef.current;
    if (!pausePreferencesReady || returningToStartupRef.current) return undefined;
    setHostBaselineReady(false);
    (async () => {
      try {
        if (startupTarget.kind === "wasm") assertWasmEnvironment(browserWasmEnvironment());
        const initialSlot = await initialSaveSourceRef.current.read(async () => {
          if (TRADING_E2E_MODE) return null;
          const slot = await getBrowserSaveRepository().load();
          return slot === null ? null : validateDayEndArchive(slot);
        });
        if (cancelled) return;
        const setup = initialSlot === null ? sessionSetup : initialSlot.setup;
        const seed = initialSlot === null ? DEFAULT_SEED : BigInt(initialSlot.seed);
        let host: EngineHost;
        if (startupTarget.kind === "tauri") {
          host = await createTauriHost(setup, seed);
        } else if (startupTarget.kind === "remote") {
          host = await createRemoteHost(setup, seed, { baseUrl: startupTarget.baseUrl });
        } else {
          host = await createWorkerHost(setup, seed, { enableE2EStepping: TRADING_E2E_MODE });
        }
        ownedHost = host;
        if (cancelled) {
          // React StrictMode 会执行一次探测性挂载；异步创建完成后必须停掉该宿主，避免泄漏 Worker/线程池。
          host.dispose();
          return;
        }
        if (initialSlot !== null) await host.load(initialSlot);
        if (cancelled) return;
        setActiveSetup(setup);
        setStartDateDraft(setup.start_date);
        setPriceCageEnabledDraft(setup.config.price_cage_enabled);
        hostRef.current = host;
        unsetIndicatorCalculator = setIndicatorCalculator(host.calculateIndicators);
        companyCoordinatorRef.current = new CompanyQueryCoordinator(host, store.dispatch);
        protocolCoordinatorRef.current = new ProtocolCoordinator({
          onBaseline(protocolState, baseline) {
            saveSelectionGenerationRef.current += 1;
            dayEndPersistenceRef.current.install(baseline.generation);
            setPlayerOrders([]);
            companyCoordinatorRef.current?.installBaseline({ civilDate: baseline.civilDate, revision: baseline.revision, seq: protocolState.snapshot.seq });
            installBaselineRef.current(protocolState);
            setHostBaselineReady(true);
            if (!TRADING_E2E_MODE) void refreshPlayerOrders();
          },
          onApplied(reduction, metadata) {
            const companyCoordinator = companyCoordinatorRef.current;
            if (reduction.update.kind === "tick-batch") {
              for (const frame of reduction.update.frames) companyCoordinator?.acceptFrame(frame, metadata);
            } else {
              companyCoordinator?.acceptCivil(reduction.update, metadata);
              setPlayerOrders([]);
              const target = dayEndFileTargetRef.current;
              const reference = { seq: reduction.update.update.seq_to, settledDate: reduction.update.update.boundary.settled_date };
              void dayEndPersistenceRef.current.completed(
                reduction.state.cursor.generation,
                host.save(reference).then((slot) => validateDayEndCandidate(slot, reference)),
                async (slot, isCurrent) => {
                  return await writeDayEndTargets(slot, isCurrent, [
                    { label: "浏览器快速槽", write: (value, current) => getBrowserSaveRepository().save(value, current) },
                    ...(target === null ? [] : [{ label: "授权文件", write: (value: unknown, current: () => boolean) => target.write(value, () => current() && target === dayEndFileTargetRef.current) }]),
                  ]);
                },
              ).then((saved) => {
                if (saved && host === hostRef.current) setNotice(target === null ? "日终存档已更新（浏览器快速槽）" : "日终存档已更新（快速槽与授权文件）");
              }, (saveError: unknown) => {
                if (host === hostRef.current) setNotice(`日终存档更新失败：${saveError instanceof Error ? saveError.message : String(saveError)}`);
              });
              void refreshPlayerOrders();
            }
            acceptReductionRef.current(reduction);
            const playerOrderChanged = reduction.update.kind === "tick-batch" && reduction.update.frames.some((frame) =>
              frame.facts.some(({ event }) =>
                ("OrderAccepted" in event && event.OrderAccepted.account === 0)
                || ("OrderCanceled" in event && event.OrderCanceled.account === 0)
                || ("IntentRejected" in event && event.IntentRejected.account === 0)
                || ("Trade" in event && (event.Trade.maker === 0 || event.Trade.taker === 0)),
              ),
            );
            if (playerOrderChanged) {
              setCancelingOrderIds(new Set());
            }
            if (reduction.update.kind === "tick-batch") {
              const frames = reduction.update.frames;
              if (frames.some((frame) => playerOrderFactsRequireRefresh(frame.facts))) {
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
            setError(failure);
          },
        });
        const supportedDeliveryModes = host.capabilities.deliveryModes;
        setDeliveryModes(supportedDeliveryModes);
        if (supportedDeliveryModes.length > 0) {
          if (!host.getDeliveryMode || !host.setDeliveryMode) {
            throw new Error("宿主声明支持刷新模式，但没有提供对应的读取或切换接口");
          }
          const supportedDeliveryMode = host.getDeliveryMode();
          if (!supportedDeliveryModes.includes(supportedDeliveryMode)) {
            throw new Error(`宿主当前刷新模式 ${supportedDeliveryMode} 不在其能力声明中`);
          }
          setDeliveryModeState(supportedDeliveryMode);
        } else {
          setDeliveryModeState(null);
        }
        // 初始化 AutoOrderManager
        autoOrderMgrRef.current = new AutoOrderManager(async (intent) => {
          const currentHost = hostRef.current;
          if (!currentHost) throw new Error("游戏引擎尚未就绪");
          await currentHost.submitIntent(intent);
        }, (id) => { if (!cancelled) store.dispatch(markTriggered(id)); }, (_id, submitError) => {
          if (!cancelled) setNotice(`条件单提交失败：${submitError instanceof Error ? submitError.message : String(submitError)}`);
        });
        // 同步 RTK autoOrders → Manager
        host.setSpeed(speed);
        await host.setPausePreferences({ pause_after_close: pauseAfterClose, pause_before_open: pauseBeforeOpen });
        if (cancelled) return;
        host.start(
          (update) => { if (!cancelled && host === hostRef.current) hostUpdateRef.current(update); },
          (failure) => { if (!cancelled && host === hostRef.current) fatalHostErrorRef.current(failure); },
        );
        if (TRADING_E2E_MODE) {
          const controlledHost = host as EngineHost & Partial<WorkerE2EHost>;
          if (typeof controlledHost.stepOnceForE2E !== "function") {
            throw new Error("交易 E2E 模式需要本地 Worker 单步能力");
          }
          let controlledTick = host.tick();
          window.__STOCK_GAME_E2E__ = {
            pause() {
              host.stop();
              store.dispatch(setRunning(false));
            },
            async advanceToTick(target) {
              if (!Number.isSafeInteger(target) || target < controlledTick) {
                throw new Error(`E2E target tick ${target} is before current tick ${controlledTick}`);
              }
              while (controlledTick < target) controlledTick = await controlledHost.stepOnceForE2E!();
              return controlledTick;
            },
            snapshot: () => host.snapshot(),
          };
        }
        if (import.meta.env.DEV && new URLSearchParams(window.location.search).get("protocolFixture") === "malformed") {
          queueMicrotask(() => hostUpdateRef.current(createProtocolUpdate("1", { Malformed: {} })));
        }
        // 初始化是异步的：页面可能已在宿主创建期间转入后台，而当时的
        // visibilitychange 监听器还拿不到 host。就绪后必须补做一次同步，
        // 避免隐藏页持续以 720x/最快占满 CPU。
        if (document.hidden) host.stop();
        if (TRADING_E2E_MODE) {
          host.stop();
          store.dispatch(setRunning(false));
        } else {
          store.dispatch(setRunning(true));
        }
        if (!cancelled) {
          initialSaveSourceRef.current.complete();
          setReady(true);
        }
      } catch (e) {
        if (!cancelled) {
          releaseOwnedHost();
          store.dispatch(setRunning(false));
          setError(startupTarget.kind === "tauri"
            ? fatalDesktopInitializationMessage(e)
            : startupTarget.kind === "remote"
              ? fatalRemoteInitializationMessage(e)
              : fatalWasmInitializationMessage(e));
        }
      }
    })();
    return () => {
      cancelled = true;
      dayEndPersistenceRef.current.invalidate();
      saveSelectionGenerationRef.current += 1;
      sessionReplacementGateRef.current.invalidate();
      if (TRADING_E2E_MODE) delete window.__STOCK_GAME_E2E__;
      releaseOwnedHost();
      playerOrderRefreshGate.invalidate();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionSetup, startupTarget, pausePreferencesReady, refreshPlayerOrders, setIndicatorCalculator]);

  useEffect(() => {
    try { hostRef.current?.setSpeed(speed); } catch (e) { setError(e instanceof Error ? e.message : String(e)); }
    if (hostRef.current) {
      setSpeedMetrics(null);
      setSpeedMetricsError(null);
    }
  }, [speed]);

  useEffect(() => {
    if (pausePreferencesLoadedRef.current || typeof window === "undefined") return;
    try {
      const preferences = loadPausePreferences(window.sessionStorage);
      store.dispatch(setPauseAfterClose(preferences.pause_after_close));
      store.dispatch(setPauseBeforeOpen(preferences.pause_before_open));
      pausePreferencesLoadedRef.current = true;
      setPausePreferencesReady(true);
    } catch (preferenceError) {
      setError(preferenceError instanceof Error ? preferenceError.message : String(preferenceError));
    }
  }, []);

  useEffect(() => {
    if (TRADING_E2E_MODE) return;
    const host = hostRef.current;
    if (host === null) return;
    void host.setPausePreferences({ pause_after_close: pauseAfterClose, pause_before_open: pauseBeforeOpen }).catch((preferenceError) => {
      setError(preferenceError instanceof Error ? preferenceError.message : String(preferenceError));
    });
    if (typeof window !== "undefined" && pausePreferencesLoadedRef.current) {
      try {
        savePausePreferences(window.sessionStorage, { pause_after_close: pauseAfterClose, pause_before_open: pauseBeforeOpen });
      } catch (preferenceError) {
        setError(preferenceError instanceof Error ? preferenceError.message : String(preferenceError));
      }
    }
  }, [pauseAfterClose, pauseBeforeOpen]);

  useEffect(() => {
    setSpeedMetrics(null);
    setSpeedMetricsError(null);
  }, [running]);

  useEffect(() => {
    const host = hostRef.current;
    if (!ready || !host || speedMetricsLoadInProgressRef.current) return;
    const requestGeneration = speedMetricsRequestGateRef.current.capture();
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const poll = async () => {
      try {
        const metrics = await host.readSpeedMetrics();
        if (!cancelled && speedMetricsRequestGateRef.current.isCurrent(requestGeneration)) {
          setSpeedMetrics(metrics);
          setSpeedMetricsError(null);
        }
      } catch (metricsError) {
        if (!cancelled && speedMetricsRequestGateRef.current.isCurrent(requestGeneration)) {
          setSpeedMetricsError(`实际倍速读取失败：${metricsError instanceof Error ? metricsError.message : String(metricsError)}；1 秒后自动重试`);
        }
      } finally {
        if (!cancelled && speedMetricsRequestGateRef.current.isCurrent(requestGeneration)) {
          timer = setTimeout(() => void poll(), 1_000);
        }
      }
    };
    void poll();
    return () => {
      cancelled = true;
      if (timer !== null) clearTimeout(timer);
    };
  }, [ready, speed, running, speedMetricsPollingGeneration]);

  // 一个浏览器中可能同时打开多个游戏页。隐藏页继续以 720x/MAX 运算会与当前页
  // 抢占全部 CPU，并让可见页的 K 线看似停止；隐藏时暂停宿主，重新可见时按 UI
  // 的运行状态恢复。游戏状态仍保留在各自 Worker 中，不会重建或丢失。
  useEffect(() => {
    const syncHostVisibility = () => {
      const host = hostRef.current;
      if (!host) return;
      if (document.hidden) {
        host.stop();
      } else if (runningRef.current) {
        host.start(
          (update) => { if (host === hostRef.current) hostUpdateRef.current(update); },
          (failure) => { if (host === hostRef.current) fatalHostErrorRef.current(failure); },
        );
      }
    };
    document.addEventListener("visibilitychange", syncHostVisibility);
    return () => document.removeEventListener("visibilitychange", syncHostVisibility);
  }, []);

  useEffect(() => {
    refreshDailyChart();
  }, [chartCode, refreshDailyChart]);

  // 存档/读档
  async function recoverFromFile() {
    const recoveryGeneration = sessionReplacementGateRef.current.begin();
    if (recoveryGeneration === null) { setNotice("上一项读档或新局操作尚未结束，请稍后再试"); return; }
    try {
      const slot = await loadFromFile();
      if (!sessionReplacementGateRef.current.isCurrent(recoveryGeneration)) return;
      if (slot === null) { setNotice("已取消读档"); return; }
      validateDayEndArchive(slot);
      dayEndPersistenceRef.current.invalidate();
      saveSelectionGenerationRef.current += 1;
      await dayEndPersistenceRef.current.idle();
      if (!sessionReplacementGateRef.current.isCurrent(recoveryGeneration)) return;
      initialSaveSourceRef.current.select(slot);
      autoOrderMgrRef.current?.clear();
      store.dispatch(clearAutoOrders());
      setPlayerOrders([]);
      setError(null);
      setReady(false);
      setSessionSetup({ ...slot.setup });
    } catch (recoveryError) {
      if (sessionReplacementGateRef.current.isCurrent(recoveryGeneration)) {
        setNotice(`选择日终存档失败：${recoveryError instanceof Error ? recoveryError.message : String(recoveryError)}`);
      }
    } finally {
      sessionReplacementGateRef.current.finish(recoveryGeneration);
    }
  }

  async function handleSave() {
    if (!hostRef.current) return;
    setNotice("已启用日终自动存档；日内不写档，首次完整自然日日结前没有可用日终存档。");
  }
  async function handleLoad() {
    const host = hostRef.current;
    if (host === null) return;
    const loadGeneration = sessionReplacementGateRef.current.begin();
    if (loadGeneration === null) { setNotice("上一项读档或新局操作尚未结束，请稍后再试"); return; }
    saveSelectionGenerationRef.current += 1;
    const isCurrent = () => host === hostRef.current && sessionReplacementGateRef.current.isCurrent(loadGeneration);
    try {
      const slot = await getBrowserSaveRepository().load();
      if (!isCurrent()) return;
      if (!slot) { setNotice("无存档"); return; }
      validateDayEndArchive(slot);
      dayEndPersistenceRef.current.invalidate();
      await dayEndPersistenceRef.current.idle();
      if (!isCurrent()) return;
      playerOrderRefreshGateRef.current.invalidate();
      speedMetricsLoadInProgressRef.current = true;
      speedMetricsRequestGateRef.current.invalidate();
      setSpeedMetricsPollingGeneration(speedMetricsRequestGateRef.current.capture());
      setSpeedMetrics(null);
      setSpeedMetricsError(null);
      try {
        await host.load(slot);
      } finally {
        speedMetricsLoadInProgressRef.current = false;
        speedMetricsRequestGateRef.current.invalidate();
        setSpeedMetricsPollingGeneration(speedMetricsRequestGateRef.current.capture());
      }
      if (!isCurrent()) return;
      const loadedSnapshot = host.snapshot();
      resetMarketHistory(loadedSnapshot);
      playerOrderRefreshGateRef.current.invalidate();
      setPlayerOrders([]);
      setActiveSetup(slot.setup);
      setStartDateDraft(slot.setup.start_date);
      setPriceCageEnabledDraft(slot.setup.config.price_cage_enabled);
      void refreshPlayerOrders();
      autoOrderMgrRef.current?.clear();
      store.dispatch(clearAutoOrders());
      setNotice(`已读档（第 ${host.day() + 1} 个交易日）`);
    } catch (e) {
      const result = await synchronizeCurrentBaseline(() => host.refreshBaseline(), isCurrent);
      if (!isCurrent() || result.kind === "stale") return;
      if (result.kind === "failed") { fatalHostErrorRef.current(`读档后权威基线同步失败：${result.error instanceof Error ? result.error.message : String(result.error)}`); return; }
      setNotice(`读档失败：${e}`);
    } finally {
      sessionReplacementGateRef.current.finish(loadGeneration);
    }
  }

  // 另存为文件（浏览器 File System Access API / 降级下载；Tauri 原生对话框）
  async function handleSaveFile() {
    const host = hostRef.current;
    if (host === null) return;
    const selectionGeneration = ++saveSelectionGenerationRef.current;
    try {
      const target = await selectDayEndFileTarget();
      if (host !== hostRef.current || selectionGeneration !== saveSelectionGenerationRef.current) return;
      if (target === null) { setNotice("已取消选择日终存档文件"); return; }
      dayEndFileTargetRef.current = target;
      setNotice("已授权日终存档文件；仅在后续完整自然日日结更新，日内不写档。");
    } catch (e) {
      if (host === hostRef.current && selectionGeneration === saveSelectionGenerationRef.current) setNotice(`文件存档失败：${e}`);
    }
  }
  // 从文件读档
  async function handleLoadFile() {
    const host = hostRef.current;
    if (host === null) return;
    const loadGeneration = sessionReplacementGateRef.current.begin();
    if (loadGeneration === null) { setNotice("上一项读档或新局操作尚未结束，请稍后再试"); return; }
    saveSelectionGenerationRef.current += 1;
    const isCurrent = () => host === hostRef.current && sessionReplacementGateRef.current.isCurrent(loadGeneration);
    try {
      const slot = await loadFromFile();
      if (!isCurrent()) return;
      if (slot === null) { setNotice("已取消读档"); return; }
      validateDayEndArchive(slot);
      dayEndPersistenceRef.current.invalidate();
      await dayEndPersistenceRef.current.idle();
      if (!isCurrent()) return;
      playerOrderRefreshGateRef.current.invalidate();
      speedMetricsLoadInProgressRef.current = true;
      speedMetricsRequestGateRef.current.invalidate();
      setSpeedMetricsPollingGeneration(speedMetricsRequestGateRef.current.capture());
      setSpeedMetrics(null);
      setSpeedMetricsError(null);
      try {
        await host.load(slot);
      } finally {
        speedMetricsLoadInProgressRef.current = false;
        speedMetricsRequestGateRef.current.invalidate();
        setSpeedMetricsPollingGeneration(speedMetricsRequestGateRef.current.capture());
      }
      if (!isCurrent()) return;
      const loadedSnapshot = host.snapshot();
      resetMarketHistory(loadedSnapshot);
      playerOrderRefreshGateRef.current.invalidate();
      setPlayerOrders([]);
      setActiveSetup(slot.setup);
      setStartDateDraft(slot.setup.start_date);
      setPriceCageEnabledDraft(slot.setup.config.price_cage_enabled);
      void refreshPlayerOrders();
      autoOrderMgrRef.current?.clear();
      store.dispatch(clearAutoOrders());
      setNotice(`已从文件读档（第 ${host.day() + 1} 个交易日）`);
    } catch (e) {
      const result = await synchronizeCurrentBaseline(() => host.refreshBaseline(), isCurrent);
      if (!isCurrent() || result.kind === "stale") return;
      if (result.kind === "failed") { fatalHostErrorRef.current(`文件读档后权威基线同步失败：${result.error instanceof Error ? result.error.message : String(result.error)}`); return; }
      setNotice(`文件读档失败：${e}`);
    } finally {
      sessionReplacementGateRef.current.finish(loadGeneration);
    }
  }

  const handlePauseToggle = useCallback(() => {
    if (!hostRef.current) return;
    if (running) {
      hostRef.current.stop();
      store.dispatch(setRunning(false));
    } else {
      const host = hostRef.current;
      host.start(
        (update) => { if (host === hostRef.current) hostUpdateRef.current(update); },
        (failure) => { if (host === hostRef.current) fatalHostErrorRef.current(failure); },
      );
      store.dispatch(setRunning(true));
      setNotice("已继续模拟");
    }
  }, [running, setNotice]);

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
  }, [setNotice]);

  const handleNewGame = useCallback(async () => {
    const result = parseStartDate(startDateDraft);
    if (result.kind === "invalid") {
      setStartDateError(result.message);
      return;
    }
    setStartDateError(null);
    const newGameGeneration = sessionReplacementGateRef.current.begin();
    if (newGameGeneration === null) { setNotice("上一项读档或新局操作尚未结束，请稍后再试"); return; }
    try {
      dayEndPersistenceRef.current.invalidate();
      saveSelectionGenerationRef.current += 1;
      await dayEndPersistenceRef.current.idle();
      if (!sessionReplacementGateRef.current.isCurrent(newGameGeneration)) return;
      initialSaveSourceRef.current.reset();
      autoOrderMgrRef.current?.clear();
      store.dispatch(clearAutoOrders());
      setPlayerOrders([]);
      setError(null);
      setReady(false);
      setSessionSetup({
        ...setupWithStartDate(activeSetup, result.value),
        config: { ...activeSetup.config, price_cage_enabled: priceCageEnabledDraft },
      });
      setNotice(`已按 ${result.value} 创建新模拟会话`);
    } finally {
      sessionReplacementGateRef.current.finish(newGameGeneration);
    }
  }, [setNotice, startDateDraft, priceCageEnabledDraft, activeSetup, autoOrderMgrRef, initialSaveSourceRef, setSessionSetup, dayEndPersistenceRef]);

  const queryCompanyReports = useCallback((companyId: string, cursor: string | null) => {
    if (TRADING_E2E_MODE) return;
    void companyCoordinatorRef.current?.query({ companyId, cursor });
  }, []);

  const advanceCivilDay = useCallback(async () => {
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
  }, [setNotice]);

  function buildIntent(side: "Buy" | "Sell"): Intent | null {
    try {
      const qty = parseShareQuantity(qtyText);
      const position = playerAccount?.positions[tradeCode];
      const reserved = playerAccount?.reserved_sell_qty[tradeCode] ?? 0;
      const sellable = position ? Math.max(0, position.qty - position.t1_locked - reserved) : 0;
      const stock = activeSetup.stocks.find((candidate) => candidate.code === tradeCode);
      if (!stock) throw new Error(`缺少股票 ${tradeCode} 的 A 股规则配置`);
      validateAShareQuantity(side, qty, sellable, maxAShareOrderQuantity(stock.category, orderKind === "market"));
      return buildPlayerOrderIntent(tradeCode, side, qty, orderKind, priceChoice, priceText);
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error));
      return null;
    }
  }

  async function submit(side: "Buy" | "Sell") {
    const intent = buildIntent(side);
    if (!intent) return;
    try {
      const currentHost = hostRef.current;
      if (!currentHost) throw new Error("游戏引擎尚未就绪");
      await currentHost.submitIntent(intent);
      const kindText = playerOrderDescription(orderKind, priceChoice, priceText);
      setNotice(`已提交${side === "Buy" ? "买入" : "卖出"}${kindText}委托：${tradeCode} ${qtyText} 股`);
    } catch (e) { setNotice(e instanceof Error ? e.message : String(e)); }
  }

  async function cancelPlayerOrder(order: PlayerWorkingOrder) {
    const host = hostRef.current;
    if (!host) {
      setNotice("游戏引擎尚未就绪，无法撤销委托");
      return;
    }
    setCancelingOrderIds((current) => new Set(current).add(order.id));
    try {
      await host.submitIntent({ Cancel: { code: order.code, id: order.id } });
      setNotice(`已提交撤单请求：委托 #${order.id}`);
    } catch (cancelError) {
      setCancelingOrderIds((current) => {
        const next = new Set(current);
        next.delete(order.id);
        return next;
      });
      setNotice(`撤单请求未入队：${cancelError instanceof Error ? cancelError.message : String(cancelError)}`);
    }
  }

  function addAuto() {
    const side: "Buy" | "Sell" = (autoType === "stopProfit" || autoType === "stopLoss" || autoType === "sellTrigger") ? "Sell" : "Buy";
    let tp: number;
    let qty: number;
    try {
      tp = parseYuanPrice(autoTrigger);
      qty = parseShareQuantity(autoQty);
      const position = playerAccount?.positions[tradeCode];
      const reserved = playerAccount?.reserved_sell_qty[tradeCode] ?? 0;
      const sellable = position ? Math.max(0, position.qty - position.t1_locked - reserved) : 0;
      const stock = activeSetup.stocks.find((candidate) => candidate.code === tradeCode);
      if (!stock) throw new Error(`缺少股票 ${tradeCode} 的 A 股规则配置`);
      validateAShareQuantity(side, qty, sellable, maxAShareOrderQuantity(stock.category));
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error));
      return;
    }
    const manager = autoOrderMgrRef.current;
    if (!manager) {
      setNotice("游戏引擎尚未就绪，无法添加条件单");
      return;
    }
    const order = manager.add({ code: tradeCode, type: autoType, triggerPrice: tp, qty, side, enabled: true });
    store.dispatch(addAutoOrder(order));
    setNotice(`已添加条件单：${AUTO_ORDER_LABELS[autoType]} ${tradeCode} @ ${autoTrigger} 元`);
  }

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
          <strong>{mobilePrimaryTitle(mobileTab)}</strong>
          <span className="mobile-head-tools">
            <MobileRunToggle running={running} onToggle={handlePauseToggle} variant="global" />
            <MobileSpeedSelect
              speed={speed}
              measuredSpeed={measuredSpeedText}
              measuredSpeedTitle={measuredSpeedTitle}
              onChange={(value) => store.dispatch(setSpeed(value))}
            />
          </span>
        </div>
        <div className="brand">股票模拟行情终端</div>
        <DesktopAssets />
        <div className="controls">
          <span className="label">速度</span>
          <HTMLSelect className="speed-select" value={speed === Infinity ? "Infinity" : String(speed)} onChange={(e) => {
            const raw = e.target.value;
            const v = raw === "Infinity" ? Infinity : Number(raw);
            store.dispatch(setSpeed(v));
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
          <DeliveryModeControl mode={deliveryMode} modes={deliveryModes} labels={DELIVERY_MODE_LABELS} onChange={handleDeliveryModeChange} />
          <Button className="simulation-button" intent={running ? "danger" : "success"} onClick={handlePauseToggle}>{running ? "暂停" : "继续"}</Button>
          <div className="new-game-control">
            <StartDateInput compact value={startDateDraft} error={startDateError} onChange={(value) => { setStartDateDraft(value); setStartDateError(null); }} />
            <PriceCageInput enabled={priceCageEnabledDraft} onChange={setPriceCageEnabledDraft} />
            <Button onClick={handleNewGame}>新游戏</Button>
          </div>
          <DesktopDayTag />
          <span className={`session-status ${running ? "is-running" : "is-paused"}`} aria-live="polite">
            <i aria-hidden="true" />{running ? "交易中" : "已暂停"}
          </span>
          <fieldset className="pause-preferences" aria-label="自然日暂停偏好">
            <label><input type="checkbox" checked={pauseAfterClose} onChange={(event) => store.dispatch(setPauseAfterClose(event.currentTarget.checked))} />收盘后暂停复盘</label>
            <label><input type="checkbox" checked={pauseBeforeOpen} onChange={(event) => store.dispatch(setPauseBeforeOpen(event.currentTarget.checked))} />开盘前暂停查看资讯</label>
          </fieldset>
          <Button className="theme-toggle" minimal onClick={() => store.dispatch(setTheme(theme === "light" ? "dark" : "light"))} title="切换主题">{theme === "light" ? "🌙" : "☀️"}</Button>
          <div className="save-group" role="group" aria-label="存档读档">
            <Button minimal onClick={handleSave} title="快存到 LocalStorage">💾 存档</Button>
            <Button minimal onClick={handleSaveFile} title="另存为文件">📁 存为文件</Button>
            <Button minimal onClick={handleLoadFile} title="从文件读档">📂 读文件</Button>
            <Button minimal onClick={handleLoad} title="从 LocalStorage 快读">📂 读档</Button>
          </div>
        </div>
      </header>

      <WorkspaceGrid orientation={orientation} data-mobile-tab={mobileTab} data-mobile-detail={mobileDetail ? "1" : "0"}>
        {/* 行情表（AG Grid） */}
        <Card className="panel market-panel" id="section-market">
          <h3 className="panel-title">行情</h3>
          <ConnectedMarketPanel onSelect={selectStock} />
        </Card>

        {/* 分时走势图 + 股票详情头 + 盘口 */}
        <Card className="panel chart-panel" id="section-trade">
          <ConnectedChartPanel chartPeriod={chartPeriod} setChartPeriod={setChartPeriod} klineDays={klineDays} setKlineDays={setKlineDays} />
        </Card>

        <Card className="panel company-panel-shell" id="section-company">
          <h3 className="panel-title">公司信息</h3>
          <ConnectedCompanyPanel initialCivilDate={activeSetup.start_date} onCompanyQuery={queryCompanyReports} onAdvanceCivilDay={advanceCivilDay} />
        </Card>

        {/* 委托面板 + 自动单（移动端为底页弹出） */}
        <Card
          ref={tradeSheetRef}
          className={`panel order-panel ${orientation === "portrait" ? "mobile-sheet" : ""} ${tradeSheetOpen ? "sheet-open" : ""}`}
          id="section-order"
          role={orientation === "portrait" && tradeSheetOpen ? "dialog" : undefined}
          aria-modal={orientation === "portrait" && tradeSheetOpen ? true : undefined}
          aria-hidden={orientation === "portrait" && !tradeSheetOpen ? true : undefined}
          hidden={orientation === "portrait" && !tradeSheetOpen}
          aria-label={orientation === "portrait" ? "交易面板" : undefined}
        >
          <h3 className="panel-title">委托下单</h3>
          <label className="field"><span>股票</span>
            <HTMLSelect value={tradeCode} onChange={(e) => { setTradeCode(e.target.value); const m = store.getState().snapshot.snapshot?.markets[e.target.value]; if (m) setPriceText(yuan(m.last_price)); }}
              options={STOCK_LIST.map((s) => ({ label: `${s.code} ${s.name}`, value: s.code }))} />
          </label>
          <label className="field"><span>委托类型</span>
            <HTMLSelect aria-label="委托类型" value={orderKind} onChange={(event) => setOrderKind(event.target.value as "limit" | "market")}
              options={[{ label: "限价委托", value: "limit" }, { label: "市价委托", value: "market" }]} />
          </label>
          {orderKind === "limit" && <label className="field"><span>限价方式</span>
            <HTMLSelect aria-label="限价方式" value={priceChoice} onChange={(event) => setPriceChoice(event.target.value as LimitPriceChoice)}
              options={[{ label: "指定价格", value: "fixed" }, { label: "最高限价", value: "highest" }, { label: "最低限价", value: "lowest" }]} />
          </label>}
          <label className="field"><span>价格（元）</span><InputGroup value={priceText} onChange={(e) => setPriceText(e.target.value)} {...orderPriceInputState(orderKind, priceChoice)} /></label>
          <label className="field"><span>数量（股）</span><InputGroup value={qtyText} onChange={(e) => setQtyText(e.target.value)} placeholder="买入按手；零股一次卖完" /></label>
          <TradeMarketControls tradeCode={tradeCode} setPriceText={setPriceText} setQtyText={setQtyText} />
          <div className="order-buttons">
            <Button intent="danger" onClick={() => void submit("Buy")}>买入</Button>
            <Button intent="success" onClick={() => void submit("Sell")}>卖出</Button>
          </div>

          <section className="player-orders" aria-labelledby="player-orders-title">
            <h4 id="player-orders-title" className="auto-title">当前活动委托</h4>
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
            <h4 className="auto-title">条件单 / 自动单</h4>
            <div className="auto-form">
              <HTMLSelect value={autoType} onChange={(e) => setAutoType(e.target.value as AutoOrderType)}
                options={(Object.keys(AUTO_ORDER_LABELS) as AutoOrderType[]).map((t) => ({ label: AUTO_ORDER_LABELS[t], value: t }))} />
              <input className="auto-input" type="text" value={autoTrigger} onChange={(e) => setAutoTrigger(e.target.value)} placeholder="触发价（元）" />
              <input className="auto-input" type="text" value={autoQty} onChange={(e) => setAutoQty(e.target.value)} placeholder="数量" />
              <Button small intent="primary" onClick={addAuto}>添加</Button>
            </div>
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
          <PositionsPanel onOpenMarket={() => switchMobileTab("market")} />
        </Card>

        {/* 分时成交 */}
        <Card className="panel trades-panel" id="section-trades">
          <h3 className="panel-title">分时成交</h3>
          <TradesPanel />
        </Card>

        <Card className="panel user-panel" id="section-user">
          <h3 className="panel-title">我的</h3>
          <section className="new-game-panel" aria-label="新游戏">
            <h4>新游戏</h4>
            <StartDateInput value={startDateDraft} error={startDateError} onChange={(value) => { setStartDateDraft(value); setStartDateError(null); }} />
            <PriceCageInput enabled={priceCageEnabledDraft} onChange={setPriceCageEnabledDraft} />
            <Button onClick={handleNewGame}>创建新游戏</Button>
          </section>
          <UserPanel running={running} pauseAfterClose={pauseAfterClose} pauseBeforeOpen={pauseBeforeOpen} deliveryMode={deliveryMode} deliveryModes={deliveryModes} deliveryLabels={DELIVERY_MODE_LABELS} onPauseAfterCloseChange={(value) => store.dispatch(setPauseAfterClose(value))} onPauseBeforeOpenChange={(value) => store.dispatch(setPauseBeforeOpen(value))} onDeliveryModeChange={handleDeliveryModeChange} onSave={() => void handleSave()} onLoad={() => void handleLoad()} onSaveFile={() => void handleSaveFile()} onLoadFile={() => void handleLoadFile()} />
        </Card>
      </WorkspaceGrid>

      {DevNpcInspector !== null && <section>
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
        {mobileTab === "market" && mobileDetail && (
          <div className="mobile-detail-page">
            <ConnectedMobileDetail klineDays={klineDays} setKlineDays={setKlineDays} period={mobileUi.chartPeriod} infoTab={mobileUi.infoTab} speed={speed} measuredSpeed={measuredSpeedText} measuredSpeedTitle={measuredSpeedTitle} running={running} initialCivilDate={activeSetup.start_date} onCompanyQuery={queryCompanyReports} onAdvanceCivilDay={advanceCivilDay} onPeriodChange={(period) => dispatchMobileUi({ type: "select-period", period })} onInfoTabChange={showDetailInfo} onPauseToggle={handlePauseToggle} onBack={() => dispatchMobileUi({ type: "back" })} onSelect={selectStock} />
          </div>
        )}
        <nav className="mobile-tabbar mobile-main-tabbar" aria-label="主导航">
          {MOBILE_PRIMARY_NAV.map(([tab, label]) => (
            <button key={tab} type="button" className={`tab-btn ${mobileTab === tab ? "active" : ""}`} onClick={() => {
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
      {notice && <div className="notice" role="status" aria-live="polite">{notice}</div>}
    </div>
  );
}

function App() {
  const initialSaveSourceRef = useRef(new InitialSaveSource<StrictSaveEnvelope>());
  const dayEndPersistenceRef = useRef(new DayEndPersistence());
  const [sessionSetup, setSessionSetup] = useState<SessionSetup>(INITIAL_SESSION_SETUP);
  const returningToStartupRef = useRef(false);
  const [returningToStartup, setReturningToStartup] = useState(false);
  const [startupReturnError, setStartupReturnError] = useState<string | null>(null);
  const [startupTarget, setStartupTarget] = useState<StartupTarget | null>(() => initialStartupTarget(import.meta.env.MODE));
  const [startupMode, setStartupMode] = useState<StartupMode>(import.meta.env.DEV && import.meta.env.VITE_ENGINE_HOST === "remote" ? "remote" : "local");
  const [remoteAddress, setRemoteAddress] = useState(import.meta.env.DEV ? import.meta.env.VITE_REMOTE_BASE_URL ?? "" : "");
  const [startupError, setStartupError] = useState<string | null>(null);
  const autoOrderMgrRef = useRef<AutoOrderManager | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const returnToStartup = useCallback(async (stopSession: () => void) => {
    if (returningToStartupRef.current) return;
    returningToStartupRef.current = true;
    setReturningToStartup(true);
    setStartupReturnError(null);
    try {
      stopSession();
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
  if (startupTarget === null) {
    return <StartupScreen mode={startupMode} remoteAddress={remoteAddress} error={startupError}
      developmentHint={import.meta.env.DEV && (import.meta.env.VITE_ENGINE_HOST !== undefined || import.meta.env.VITE_REMOTE_BASE_URL !== undefined)}
      onModeChange={(mode) => { setStartupMode(mode); setStartupError(null); }}
      onAddressChange={(address) => { setRemoteAddress(address); setStartupError(null); }}
      onStart={() => {
        try {
          const target = resolveStartupTarget(startupMode, remoteAddress, "__TAURI_INTERNALS__" in window, browserWasmEnvironment);
          setStartupError(null);
          setStartupTarget(target);
        } catch (startupFailure) {
          setStartupError(startupFailure instanceof Error ? startupFailure.message : String(startupFailure));
        }
      }} />;
  }
  return (
    <MarketRuntimeProvider autoOrderManagerRef={autoOrderMgrRef} setNotice={setNotice}>
      <AppShell startupTarget={startupTarget} initialSaveSourceRef={initialSaveSourceRef} onSelectHost={returnToStartup}
        dayEndPersistenceRef={dayEndPersistenceRef} sessionSetup={sessionSetup} setSessionSetup={setSessionSetup}
        returningToStartup={returningToStartup} returningToStartupRef={returningToStartupRef} startupReturnError={startupReturnError}
        autoOrderMgrRef={autoOrderMgrRef} notice={notice} setNotice={setNotice} />
    </MarketRuntimeProvider>
  );
}

export default App;
