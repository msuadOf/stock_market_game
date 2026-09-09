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
import { useEffect, useRef, useState, useCallback, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import { Button, Card, InputGroup, HTMLSelect, Switch } from "@blueprintjs/core";
import { useSelector } from "react-redux";
import type { DeliveryMode, EngineHost, SpeedMetrics } from "./host/engine-host";
import type { HostUpdate } from "./host/host-update.ts";
import { createTauriHost } from "./host/tauri-host";
import { createRemoteHost } from "./host/remote-host";
import { createWorkerHost } from "./host/worker-host";
import { SpeedMetricsRequestGate, speedMetricsMatchesUiState } from "./host/speed";
import { fatalDesktopInitializationMessage, fatalRemoteInitializationMessage, fatalWasmInitializationMessage } from "./host/startup-policy";
import { DEFAULT_SEED, DEFAULT_SETUP, STOCK_LIST } from "./config/defaults";
import type { Intent } from "./types/engine";
import {
  setRunning,
  setSpeed,
  setTheme,
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
import { saveToFile, loadFromFile } from "./save/save-file";
import { LocalStorageSaveRepository } from "./save/save-repository";
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
import { useMobileUiController } from "./app/useMobileUiController";
import { MarketRuntimeProvider, useMarketRuntimeActions, useMarketRuntimeSelection } from "./app/MarketRuntimeProvider.tsx";
import {
  ClockMarker,
  ConnectedChartPanel,
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
const MAX_DAILY_CANDLES = 360;
const DELIVERY_MODE_LABELS: Record<DeliveryMode, string> = {
  push: "服务端推送 60Hz",
  pull: "客户端拉取 60Hz",
};
let browserSaveRepository: LocalStorageSaveRepository | null = null;

function getBrowserSaveRepository(): LocalStorageSaveRepository {
  if (typeof window === "undefined") throw new Error("浏览器存储在当前运行环境不可用");
  browserSaveRepository ??= new LocalStorageSaveRepository(window.localStorage);
  return browserSaveRepository;
}

interface AppShellProps {
  autoOrderMgrRef: MutableRefObject<AutoOrderManager | null>;
  notice: string | null;
  setNotice: Dispatch<SetStateAction<string | null>>;
}

function AppShell({ autoOrderMgrRef, notice, setNotice }: AppShellProps) {
  const hasSnapshot = useSelector((s: RootState) => s.snapshot.snapshot !== null);
  const playerAccount = useSelector((s: RootState) => s.snapshot.snapshot?.accounts[PLAYER_ACCOUNT_KEY] ?? null);
  const speed = useSelector((s: RootState) => s.settings.speed);
  const running = useSelector((s: RootState) => s.settings.running);
  const theme = useSelector((s: RootState) => s.settings.theme);
  const autoOrders = useSelector((s: RootState) => s.autoOrders.items);
  const orientation = useOrientation();
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [speedMetrics, setSpeedMetrics] = useState<SpeedMetrics | null>(null);
  const [speedMetricsError, setSpeedMetricsError] = useState<string | null>(null);
  const [speedMetricsPollingGeneration, setSpeedMetricsPollingGeneration] = useState(0);
  const [deliveryMode, setDeliveryModeState] = useState<DeliveryMode | null>(null);
  const [deliveryModes, setDeliveryModes] = useState<readonly DeliveryMode[]>([]);
  const speedMetricsRequestGateRef = useRef(new SpeedMetricsRequestGate());
  const speedMetricsLoadInProgressRef = useRef(false);
  const hostRef = useRef<EngineHost | null>(null);
  const fatalHostErrorRef = useRef<(message: string) => void>(() => {});
  fatalHostErrorRef.current = (message) => {
    store.dispatch(setRunning(false));
    setError(`游戏引擎已崩溃：${message}`);
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
  const { onEventsRef, acceptRuntimeSnapshot, selectChart, resetMarketHistory, refreshDailyChart } = useMarketRuntimeActions();
  const acceptRuntimeSnapshotRef = useRef(acceptRuntimeSnapshot);
  acceptRuntimeSnapshotRef.current = acceptRuntimeSnapshot;
  const hostUpdateRef = useRef<(update: HostUpdate) => void>(() => {});
  hostUpdateRef.current = (update) => {
    if (update.type === "baseline") {
      acceptRuntimeSnapshotRef.current(update.snapshot);
      return;
    }
    onEventsRef.current(update.events);
    if (update.runtimeSnapshot) acceptRuntimeSnapshotRef.current(update.runtimeSnapshot);
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
  const [priceText, setPriceText] = useState<string>("");
  const [qtyText, setQtyText] = useState<string>("100");

  // 自动单添加表单状态
  const [autoType, setAutoType] = useState<AutoOrderType>("stopProfit");
  const [autoTrigger, setAutoTrigger] = useState<string>("");
  const [autoQty, setAutoQty] = useState<string>("100");

  useEffect(() => {
    let cancelled = false;
    let ownedHost: EngineHost | null = null;
    (async () => {
      const detectedMode = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window
        ? "tauri"
        : "wasm";
      const deploymentMode = import.meta.env.VITE_ENGINE_HOST ?? detectedMode;
      try {
        let host: EngineHost;
        if (deploymentMode === "tauri") {
          host = await createTauriHost(DEFAULT_SETUP, DEFAULT_SEED);
        } else if (deploymentMode === "remote") {
          host = await createRemoteHost(DEFAULT_SETUP, DEFAULT_SEED);
        } else if (deploymentMode === "wasm") {
          // Web 版必须使用多线程 WASM。初始化失败属于致命配置错误，禁止以单线程
          // fallback 掩盖问题，否则高倍速会表现为“能运行但不可用”。
          host = await createWorkerHost(DEFAULT_SETUP, DEFAULT_SEED);
        } else {
          throw new Error(`未知引擎宿主模式：${deploymentMode}`);
        }
        ownedHost = host;
        if (cancelled) {
          // React StrictMode 会执行一次探测性挂载；异步创建完成后必须停掉该宿主，避免泄漏 Worker/线程池。
          host.dispose();
          return;
        }
        hostRef.current = host;
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
        }, (id) => store.dispatch(markTriggered(id)), (_id, submitError) => {
          setNotice(`条件单提交失败：${submitError instanceof Error ? submitError.message : String(submitError)}`);
        });
        // 同步 RTK autoOrders → Manager
        host.setSpeed(speed);
        host.start(
          (update) => hostUpdateRef.current(update),
          (failure) => fatalHostErrorRef.current(`${failure.code}: ${failure.message}`),
        );
        // 初始化是异步的：页面可能已在宿主创建期间转入后台，而当时的
        // visibilitychange 监听器还拿不到 host。就绪后必须补做一次同步，
        // 避免隐藏页持续以 720x/最快占满 CPU。
        if (document.hidden) host.stop();
        store.dispatch(setRunning(true));
        if (!cancelled) setReady(true);
      } catch (e) {
        if (!cancelled) {
          setError(deploymentMode === "tauri"
            ? fatalDesktopInitializationMessage(e)
            : deploymentMode === "remote"
              ? fatalRemoteInitializationMessage(e)
              : fatalWasmInitializationMessage(e));
        }
      }
    })();
    return () => {
      cancelled = true;
      ownedHost?.dispose();
      if (hostRef.current === ownedHost) hostRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    try { hostRef.current?.setSpeed(speed); } catch (e) { setError(e instanceof Error ? e.message : String(e)); }
    if (hostRef.current) {
      setSpeedMetrics(null);
      setSpeedMetricsError(null);
    }
  }, [speed]);

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
          (update) => hostUpdateRef.current(update),
          (failure) => fatalHostErrorRef.current(`${failure.code}: ${failure.message}`),
        );
      }
    };
    document.addEventListener("visibilitychange", syncHostVisibility);
    return () => document.removeEventListener("visibilitychange", syncHostVisibility);
  }, [onEventsRef]);

  useEffect(() => {
    refreshDailyChart();
  }, [chartCode, refreshDailyChart]);

  // 存档/读档
  async function handleSave() {
    if (!hostRef.current) return;
    try {
      const slot = await hostRef.current.save();
      getBrowserSaveRepository().save(slot);
      setNotice(`已存档（第 ${hostRef.current.day() + 1} 个交易日）`);
    } catch (e) { setNotice(`存档失败：${e}`); }
  }
  async function handleLoad() {
    if (!hostRef.current) return;
    try {
      const slot = getBrowserSaveRepository().load();
      if (!slot) { setNotice("无存档"); return; }
      speedMetricsLoadInProgressRef.current = true;
      speedMetricsRequestGateRef.current.invalidate();
      setSpeedMetricsPollingGeneration(speedMetricsRequestGateRef.current.capture());
      setSpeedMetrics(null);
      setSpeedMetricsError(null);
      try {
        await hostRef.current.load(slot);
      } finally {
        speedMetricsLoadInProgressRef.current = false;
        speedMetricsRequestGateRef.current.invalidate();
        setSpeedMetricsPollingGeneration(speedMetricsRequestGateRef.current.capture());
      }
      const loadedSnapshot = hostRef.current.snapshot();
      resetMarketHistory(loadedSnapshot);
      autoOrderMgrRef.current?.clear();
      store.dispatch(clearAutoOrders());
      setNotice(`已读档（第 ${hostRef.current.day() + 1} 个交易日）`);
    } catch (e) { setNotice(`读档失败：${e}`); }
  }

  // 另存为文件（浏览器 File System Access API / 降级下载；Tauri 原生对话框）
  async function handleSaveFile() {
    if (!hostRef.current) return;
    try {
      const slot = await hostRef.current.save();
      const done = await saveToFile(slot);
      setNotice(done ? `已另存为文件（第 ${hostRef.current.day() + 1} 个交易日）` : "已取消保存");
    } catch (e) { setNotice(`文件存档失败：${e}`); }
  }
  // 从文件读档
  async function handleLoadFile() {
    if (!hostRef.current) return;
    try {
      const slot = await loadFromFile();
      if (slot === null) { setNotice("已取消读档"); return; }
      speedMetricsLoadInProgressRef.current = true;
      speedMetricsRequestGateRef.current.invalidate();
      setSpeedMetricsPollingGeneration(speedMetricsRequestGateRef.current.capture());
      setSpeedMetrics(null);
      setSpeedMetricsError(null);
      try {
        await hostRef.current.load(slot);
      } finally {
        speedMetricsLoadInProgressRef.current = false;
        speedMetricsRequestGateRef.current.invalidate();
        setSpeedMetricsPollingGeneration(speedMetricsRequestGateRef.current.capture());
      }
      const loadedSnapshot = hostRef.current.snapshot();
      resetMarketHistory(loadedSnapshot);
      autoOrderMgrRef.current?.clear();
      store.dispatch(clearAutoOrders());
      setNotice(`已从文件读档（第 ${hostRef.current.day() + 1} 个交易日）`);
    } catch (e) { setNotice(`文件读档失败：${e}`); }
  }

  const handlePauseToggle = useCallback(() => {
    if (!hostRef.current) return;
    if (running) {
      hostRef.current.stop();
      store.dispatch(setRunning(false));
    } else {
      hostRef.current.start(
        (update) => hostUpdateRef.current(update),
        (failure) => fatalHostErrorRef.current(`${failure.code}: ${failure.message}`),
      );
      store.dispatch(setRunning(true));
    }
  }, [running]);

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

  function buildIntent(side: "Buy" | "Sell"): Intent | null {
    try {
      const price = parseYuanPrice(priceText);
      const qty = parseShareQuantity(qtyText);
      const position = playerAccount?.positions[tradeCode];
      const reserved = playerAccount?.reserved_sell_qty[tradeCode] ?? 0;
      const sellable = position ? Math.max(0, position.qty - position.t1_locked - reserved) : 0;
      const stock = DEFAULT_SETUP.stocks.find((candidate) => candidate.code === tradeCode);
      if (!stock) throw new Error(`缺少股票 ${tradeCode} 的 A 股规则配置`);
      validateAShareQuantity(side, qty, sellable, maxAShareOrderQuantity(stock.category));
      return { PlaceLimit: { code: tradeCode, side, price, qty } };
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
      setNotice(`已提交${side === "Buy" ? "买入" : "卖出"}委托：${tradeCode} ${qtyText} 股 @ ${priceText} 元`);
    } catch (e) { setNotice(e instanceof Error ? e.message : String(e)); }
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
      const stock = DEFAULT_SETUP.stocks.find((candidate) => candidate.code === tradeCode);
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

  if (error) {
    return (
      <div className="app-error" role="alert" aria-live="assertive">
        <h2>游戏已崩溃</h2>
        <p>行情引擎未能启动。请根据下方原因修复运行环境后刷新页面。</p>
        <pre>{error}</pre>
      </div>
    );
  }
  if (!ready || !hasSnapshot) {
    return <div className="app-loading">正在加载行情引擎…</div>;
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
          {deliveryMode !== null && deliveryModes.length > 0 && (
            <label className="delivery-mode-control">
              <span>刷新</span>
              <HTMLSelect
                className="delivery-select"
                aria-label="客户端 Publisher 刷新模式"
                value={deliveryMode}
                onChange={(event) => handleDeliveryModeChange(event.target.value as DeliveryMode)}
                options={deliveryModes.map((mode) => ({ label: DELIVERY_MODE_LABELS[mode], value: mode }))}
              />
            </label>
          )}
          <Button className="simulation-button" intent={running ? "danger" : "success"} onClick={handlePauseToggle}>{running ? "暂停" : "继续"}</Button>
          <DesktopDayTag />
          <span className={`session-status ${running ? "is-running" : "is-paused"}`} aria-live="polite">
            <i aria-hidden="true" />{running ? "交易中" : "已暂停"}
          </span>
          <Button className="theme-toggle" minimal onClick={() => store.dispatch(setTheme(theme === "light" ? "dark" : "light"))} title="切换主题">{theme === "light" ? "🌙" : "☀️"}</Button>
          <div className="save-group" role="group" aria-label="存档读档">
            <Button minimal onClick={handleSave} title="快存到 LocalStorage">💾 存档</Button>
            <Button minimal onClick={handleSaveFile} title="另存为文件">📁 存为文件</Button>
            <Button minimal onClick={handleLoadFile} title="从文件读档">📂 读文件</Button>
            <Button minimal onClick={handleLoad} title="从 LocalStorage 快读">📂 读档</Button>
          </div>
        </div>
      </header>

      <div className="app-grid" data-mobile-tab={mobileTab} data-mobile-detail={mobileDetail ? "1" : "0"}>
        {/* 行情表（AG Grid） */}
        <Card className="panel market-panel" id="section-market">
          <h3 className="panel-title">行情</h3>
          <ConnectedMarketPanel onSelect={selectStock} />
        </Card>

        {/* 分时走势图 + 股票详情头 + 盘口 */}
        <Card className="panel chart-panel" id="section-trade">
          <ConnectedChartPanel chartPeriod={chartPeriod} setChartPeriod={setChartPeriod} klineDays={klineDays} setKlineDays={setKlineDays} />
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
          <label className="field"><span>价格（元）</span><InputGroup value={priceText} onChange={(e) => setPriceText(e.target.value)} placeholder="委托价" /></label>
          <label className="field"><span>数量（股）</span><InputGroup value={qtyText} onChange={(e) => setQtyText(e.target.value)} placeholder="买入按手；零股一次卖完" /></label>
          <TradeMarketControls tradeCode={tradeCode} setPriceText={setPriceText} setQtyText={setQtyText} />
          <div className="order-buttons">
            <Button intent="danger" onClick={() => void submit("Buy")}>买入</Button>
            <Button intent="success" onClick={() => void submit("Sell")}>卖出</Button>
          </div>

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
          <UserPanel running={running} deliveryMode={deliveryMode} deliveryModes={deliveryModes} deliveryLabels={DELIVERY_MODE_LABELS} onDeliveryModeChange={handleDeliveryModeChange} onSave={() => void handleSave()} onLoad={() => void handleLoad()} onSaveFile={() => void handleSaveFile()} onLoadFile={() => void handleLoadFile()} />
        </Card>
      </div>

      {/* 移动端浮动交易按钮（贴 ref .ctrl-btn） */}
      {orientation === "portrait" && (
        <>
        {mobileTab === "market" && mobileDetail && (
          <div className="mobile-detail-page">
            <ConnectedMobileDetail klineDays={klineDays} setKlineDays={setKlineDays} period={mobileUi.chartPeriod} infoTab={mobileUi.infoTab} speed={speed} measuredSpeed={measuredSpeedText} measuredSpeedTitle={measuredSpeedTitle} running={running} onPeriodChange={(period) => dispatchMobileUi({ type: "select-period", period })} onInfoTabChange={showDetailInfo} onPauseToggle={handlePauseToggle} onBack={() => dispatchMobileUi({ type: "back" })} onSelect={selectStock} />
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
      {speedMetricsError && <div className="speed-metrics-error" role="alert">{speedMetricsError}</div>}
      {notice && <div className="notice" role="status" aria-live="polite">{notice}</div>}
    </div>
  );
}

function App() {
  const autoOrderMgrRef = useRef<AutoOrderManager | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  return (
    <MarketRuntimeProvider autoOrderManagerRef={autoOrderMgrRef} setNotice={setNotice}>
      <AppShell autoOrderMgrRef={autoOrderMgrRef} notice={notice} setNotice={setNotice} />
    </MarketRuntimeProvider>
  );
}

export default App;
