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
import { useEffect, useMemo, useRef, useState, useCallback } from "react";
import { Button, Card, InputGroup, HTMLSelect, Switch } from "@blueprintjs/core";
import { useSelector } from "react-redux";
import type { EngineHost } from "./host/engine-host";
import { createTauriHost } from "./host/tauri-host";
import { createRemoteHost } from "./host/remote-host";
import { createWorkerHost } from "./host/worker-host";
import { fatalDesktopInitializationMessage, fatalRemoteInitializationMessage, fatalWasmInitializationMessage } from "./host/startup-policy";
import { DEFAULT_SEED, DEFAULT_SETUP, STOCK_LIST, STOCK_NAMES, TRADING_MINUTES_PER_DAY } from "./config/defaults";
import type { Intent } from "./types/engine";
import {
  setRunning,
  setSnapshot,
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
import { PriceChart } from "./components/PriceChart";
import { MarketGrid } from "./components/MarketGrid";
import { AutoOrderManager, AUTO_ORDER_LABELS, type AutoOrderType } from "./components/auto-order-manager";
import { useOrientation } from "./hooks/useOrientation";
import { saveToFile, loadFromFile } from "./save/save-file";
import { LocalStorageSaveRepository } from "./save/save-repository";
import { MobileStockDetail } from "./mobile/MobileStockDetail";
import { MobileSpeedSelect } from "./mobile/MobileSpeedSelect";
import { MobileGameClock } from "./mobile/MobileGameClock";
import { MobileRunToggle } from "./mobile/MobileRunToggle";
import { marketCodesForView, priceChangePercent } from "./mobile/market-model";
import { MOBILE_PRIMARY_NAV, mobilePrimaryTitle } from "./mobile/mobile-ui-state";
import { colorClass, formatSharesAsLots, formatYuanAmount, yuan } from "./utils/format";
import {
  aSharePriceLimits,
  maxAShareOrderQuantity,
  parseShareQuantity,
  parseYuanPrice,
  validateAShareQuantity,
} from "./utils/trade-input";
import { useMarketChartRuntime } from "./app/useMarketChartRuntime";
import { useMobileUiController } from "./app/useMobileUiController";

const PLAYER_ACCOUNT_KEY = "0";
const MAX_DAILY_CANDLES = 360;
let browserSaveRepository: LocalStorageSaveRepository | null = null;

function getBrowserSaveRepository(): LocalStorageSaveRepository {
  if (typeof window === "undefined") throw new Error("浏览器存储在当前运行环境不可用");
  browserSaveRepository ??= new LocalStorageSaveRepository(window.localStorage);
  return browserSaveRepository;
}

function App() {
  const snapshot = useSelector((s: RootState) => s.snapshot.snapshot);
  const speed = useSelector((s: RootState) => s.settings.speed);
  const running = useSelector((s: RootState) => s.settings.running);
  const trades = useSelector((s: RootState) => s.trades.items);
  const theme = useSelector((s: RootState) => s.settings.theme);
  const autoOrders = useSelector((s: RootState) => s.autoOrders.items);
  const orientation = useOrientation();
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const hostRef = useRef<EngineHost | null>(null);
  const autoOrderMgrRef = useRef<AutoOrderManager | null>(null);
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
  const {
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
  } = useMarketChartRuntime({ autoOrderManagerRef: autoOrderMgrRef, setNotice });
  const acceptRuntimeSnapshotRef = useRef(acceptRuntimeSnapshot);
  acceptRuntimeSnapshotRef.current = acceptRuntimeSnapshot;
  const runningRef = useRef(running);
  runningRef.current = running;
  const [chartPeriod, setChartPeriod] = useState<"分时" | "日K">("分时");
  const [klineDays, setKlineDays] = useState<number>(MAX_DAILY_CANDLES);

  function selectStock(code: string) {
    selectChart(code);
    setTradeCode(code);
    const market = snapshot?.markets[code];
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
          (events) => onEventsRef.current(events),
          acceptRuntimeSnapshot,
          (message) => fatalHostErrorRef.current(message),
        );
        // 初始化是异步的：页面可能已在宿主创建期间转入后台，而当时的
        // visibilitychange 监听器还拿不到 host。就绪后必须补做一次同步，
        // 避免隐藏页持续以 720x/最快占满 CPU。
        if (document.hidden) host.stop();
        const initialSnapshot = host.snapshot();
        syncDailyCandleSnapshot(initialSnapshot);
        store.dispatch(setSnapshot(initialSnapshot));
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
  }, [speed]);

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
          (events) => onEventsRef.current(events),
          (nextSnapshot) => acceptRuntimeSnapshotRef.current(nextSnapshot),
          (message) => fatalHostErrorRef.current(message),
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
      setNotice(`已存档（第 ${snapshot?.day ?? 0} 个交易日）`);
    } catch (e) { setNotice(`存档失败：${e}`); }
  }
  async function handleLoad() {
    if (!hostRef.current) return;
    try {
      const slot = getBrowserSaveRepository().load();
      if (!slot) { setNotice("无存档"); return; }
      await hostRef.current.load(slot);
      const loadedSnapshot = hostRef.current.snapshot();
      resetMarketHistory(loadedSnapshot);
      store.dispatch(setSnapshot(loadedSnapshot));
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
      setNotice(done ? `已另存为文件（第 ${snapshot?.day ?? 0} 个交易日）` : "已取消保存");
    } catch (e) { setNotice(`文件存档失败：${e}`); }
  }
  // 从文件读档
  async function handleLoadFile() {
    if (!hostRef.current) return;
    try {
      const slot = await loadFromFile();
      if (slot === null) { setNotice("已取消读档"); return; }
      await hostRef.current.load(slot);
      const loadedSnapshot = hostRef.current.snapshot();
      resetMarketHistory(loadedSnapshot);
      store.dispatch(setSnapshot(loadedSnapshot));
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
        (events) => onEventsRef.current(events),
        acceptRuntimeSnapshot,
        (message) => fatalHostErrorRef.current(message),
      );
      store.dispatch(setRunning(true));
    }
  }, [running, acceptRuntimeSnapshot, onEventsRef]);

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

  const playerAccount = snapshot?.accounts[PLAYER_ACCOUNT_KEY] ?? null;
  const cash = playerAccount?.cash ?? 0;
  const reservedCash = playerAccount?.reserved_cash ?? 0;
  const availableCash = cash - reservedCash;

  const positionsView = useMemo(() => {
    if (!snapshot || !playerAccount) return [];
    return Object.entries(playerAccount.positions)
      .filter(([, p]) => p.qty > 0)
      .map(([code, p]) => {
        const mkt = snapshot.markets[code];
        const cur = mkt?.last_price ?? 0;
        const netInvested = p.invested_cents - p.recovered_cents;
        const avgCost = p.qty > 0 ? netInvested / p.qty : 0;
        const marketValue = cur * p.qty;
        const pnl = marketValue - netInvested;
        const reservedSellQty = playerAccount.reserved_sell_qty[code] ?? 0;
        return {
          code,
          qty: p.qty,
          sellableQty: Math.max(0, p.qty - p.t1_locked - reservedSellQty),
          avgCost,
          marketValue,
          pnl,
        };
      });
  }, [snapshot, playerAccount]);
  const heldCodes = useMemo(() => new Set(positionsView.map((position) => position.code)), [positionsView]);
  const orderedMarketCodes = useMemo(
    () => marketCodesForView(Object.keys(snapshot?.markets ?? {}), STOCK_LIST.map((stock) => stock.code), "watchlist", heldCodes),
    [heldCodes, snapshot],
  );

  const totalMarketValue = positionsView.reduce((s, x) => s + x.marketValue, 0);
  const totalAssets = cash + totalMarketValue;
  const totalPnl = positionsView.reduce((s, x) => s + x.pnl, 0);
  const latestMinute = chartData.at(-1)?.time;
  const elapsedMinutes = latestMinute === undefined ? 0 : Math.min(TRADING_MINUTES_PER_DAY, Math.floor(latestMinute) + 1);

  if (error) {
    return (
      <div className="app-error" role="alert" aria-live="assertive">
        <h2>游戏已崩溃</h2>
        <p>行情引擎未能启动。请根据下方原因修复运行环境后刷新页面。</p>
        <pre>{error}</pre>
      </div>
    );
  }
  if (!ready || !snapshot) {
    return <div className="app-loading">正在加载行情引擎…</div>;
  }

  return (
    <div
      className={`app-root ${orientation === "portrait" ? "layout-mobile" : "layout-desktop"}`}
      data-theme={theme}
      data-game-day={snapshot.day}
      data-game-tick={snapshot.tick}
    >
      {/* 顶栏 */}
      <header className="top-bar">
        <div className="mobile-brand-bar">
          <button type="button" aria-label="打开我的与存档" onClick={() => switchMobileTab("user")}><span aria-hidden="true">☰</span></button>
          <MobileGameClock day={snapshot.day} tick={snapshot.tick} variant="global" />
          <strong>{mobilePrimaryTitle(mobileTab)}</strong>
          <span className="mobile-head-tools">
            <MobileRunToggle running={running} onToggle={handlePauseToggle} variant="global" />
            <MobileSpeedSelect speed={speed} onChange={(value) => store.dispatch(setSpeed(value))} />
          </span>
        </div>
        <div className="brand">股票模拟行情终端</div>
        <div className="assets">
          <div className="asset"><span className="label">总资产</span><span className="value">{formatYuanAmount(totalAssets / 100)}</span><span className="unit">元</span></div>
          <div className="asset"><span className="label">可用资金</span><span className="value">{formatYuanAmount(availableCash / 100)}</span><span className="unit">元</span></div>
          <div className="asset"><span className="label">总盈亏</span><span className={`value ${colorClass(totalPnl)}`}>{totalPnl >= 0 ? "+" : ""}{formatYuanAmount(totalPnl / 100)}</span><span className="unit">元</span></div>
        </div>
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
          <Button className="simulation-button" intent={running ? "danger" : "success"} onClick={handlePauseToggle}>{running ? "暂停" : "继续"}</Button>
          <span className="day-tag">第 {snapshot.day + 1} 个交易日</span>
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
          <MarketGrid
            snapshot={snapshot}
            selectedCode={chartCode}
            onSelect={selectStock}
            heldCodes={heldCodes}
            priceHistoryByCode={priceHistoryByCodeRef.current}
          />
        </Card>

        {/* 分时走势图 + 股票详情头 + 盘口 */}
        <Card className="panel chart-panel" id="section-trade">
          {/* 分时/日K tab 切换（贴 ref .detail-tabs） */}
          <div className="chart-tabs">
            {(["分时", "日K"] as const).map((p) => (
              <button key={p} className={`chart-tab ${chartPeriod === p ? "active" : ""}`} onClick={() => setChartPeriod(p)}>{p}</button>
            ))}
          </div>
          {/* 股票详情头（大字现价 + 涨跌，贴 ref .detail-info） */}
          {(() => {
            const m = snapshot.markets[chartCode];
            if (!m) return null;
            const diff = m.last_price - m.last_close;
            const pct = priceChangePercent(m.last_price, m.last_close);
            const cls = colorClass(diff);
            return (
              <div className="stock-detail-header">
                <div className="detail-left">
                  <div className="detail-name">{STOCK_NAMES[chartCode] ?? chartCode}</div>
                  <div className="detail-code">{chartCode}</div>
                </div>
                <div className="detail-prices">
                  <span className={`detail-price ${cls}`}>{yuan(m.last_price)}</span>
                  <span className={`detail-change ${cls}`}>{diff >= 0 ? "+" : ""}{yuan(diff)} ({pct >= 0 ? "+" : ""}{pct.toFixed(2)}%)</span>
                </div>
              </div>
            );
          })()}
          <PriceChart data={chartData} dailyCandles={dailyChartData} lastClose={(snapshot.markets[chartCode]?.last_close ?? 0) / 100} chartType={chartPeriod} klineDays={klineDays} />
          {/* 日K 天数选择器 */}
          {chartPeriod === "日K" && (
            <div className="kline-period-bar">
              {[20, 60, 120, 240, MAX_DAILY_CANDLES].map((d) => (
                <button key={d} className={`kline-period-btn ${klineDays === d ? "active" : ""}`} onClick={() => setKlineDays(d)}>{d}日</button>
              ))}
            </div>
          )}
          {/* 五档盘口（贴 ref .quote-panel） */}
          {(() => {
            const m = snapshot.markets[chartCode];
            if (!m) return null;
            const topBids = m.bids.slice(0, 5);
            const topAsks = m.asks.slice(0, 5);
            const lc = m.last_close;
            const rowCls = (p: number) => p > lc ? "up" : p < lc ? "down" : "flat";
            return (
              <div className="order-book">
                <div className="ob-title">五档盘口（手）</div>
                <div className="ob-rows">
                  {topAsks.map((lvl, i) => (
                    <div key={`a${i}`} className="ob-row ob-ask">
                      <span className="ob-label">卖{5 - i}</span>
                      <span className={`ob-price ${rowCls(lvl[0])}`}>{yuan(lvl[0])}</span>
                      <span className="ob-qty">{formatSharesAsLots(lvl[1])}</span>
                    </div>
                  ))}
                  <div className="ob-divider" />
                  {topBids.map((lvl, i) => (
                    <div key={`b${i}`} className="ob-row ob-bid">
                      <span className="ob-label">买{i + 1}</span>
                      <span className={`ob-price ${rowCls(lvl[0])}`}>{yuan(lvl[0])}</span>
                      <span className="ob-qty">{formatSharesAsLots(lvl[1])}</span>
                    </div>
                  ))}
                </div>
              </div>
            );
          })()}
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
            <HTMLSelect value={tradeCode} onChange={(e) => { setTradeCode(e.target.value); const m = snapshot.markets[e.target.value]; if (m) setPriceText(yuan(m.last_price)); }}
              options={STOCK_LIST.map((s) => ({ label: `${s.code} ${s.name}`, value: s.code }))} />
          </label>
          <label className="field"><span>价格（元）</span><InputGroup value={priceText} onChange={(e) => setPriceText(e.target.value)} placeholder="委托价" /></label>
          <label className="field"><span>数量（股）</span><InputGroup value={qtyText} onChange={(e) => setQtyText(e.target.value)} placeholder="买入按手；零股一次卖完" /></label>
          {/* 快速仓位按钮（贴 ref 全仓/1/2/1/3/1/4） */}
          <div className="quick-position">
            {(() => {
              const m = snapshot.markets[tradeCode];
              const price = m ? m.last_price : 0;
              const maxQty = price > 0 ? Math.floor(availableCash / price / 100) * 100 : 0;
              return [
                { label: "全仓", pct: 1 },
                { label: "1/2", pct: 0.5 },
                { label: "1/3", pct: 1 / 3 },
                { label: "1/4", pct: 0.25 },
              ].map((btn) => {
                const q = Math.floor((maxQty * btn.pct) / 100) * 100;
                return (
                  <button key={btn.label} className="qp-btn" onClick={() => setQtyText(String(Math.max(100, q)))} disabled={q < 100}>
                    {btn.label}
                  </button>
                );
              });
            })()}
          </div>
          {/* 涨停/跌停快捷填充（贴 ref .limit-links） */}
          {(() => {
            const m = snapshot.markets[tradeCode];
            if (!m) return null;
            const stock = DEFAULT_SETUP.stocks.find((candidate) => candidate.code === tradeCode);
            if (!stock) return <div className="limit-links" role="alert">缺少 {tradeCode} 的交易规则</div>;
            const { up: upStop, down: downStop } = aSharePriceLimits(m.last_close, stock.category);
            const isUpLimit = m.last_price >= upStop;
            return (
              <div className="limit-links">
                <button className="ll-btn down" onClick={() => setPriceText(yuan(downStop))}>跌停 {yuan(downStop)}</button>
                <button className="ll-btn up" onClick={() => setPriceText(yuan(upStop))} disabled={isUpLimit}>涨停 {yuan(upStop)}</button>
              </div>
            );
          })()}
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
          <section className="mobile-portfolio-summary" aria-label="账户资产概览">
            <div className="mobile-assets-total">
              <span>总资产</span>
              <strong>{formatYuanAmount(totalAssets / 100)}元</strong>
              <small>可用资金 {formatYuanAmount(availableCash / 100)}元</small>
            </div>
            <div><span>持仓市值</span><b>{formatYuanAmount(totalMarketValue / 100)}元</b></div>
            <div><span>浮动盈亏</span><b className={colorClass(totalPnl)}>{totalPnl >= 0 ? "+" : ""}{formatYuanAmount(totalPnl / 100)}元</b></div>
          </section>
          <div className="mobile-position-list-head"><strong>我的持仓</strong><span>{positionsView.length} 只</span></div>
          <div className="mobile-position-table-wrap">
            <table className="grid-table">
              <thead><tr><th>代码</th><th className="num">持仓</th><th className="num">可卖</th><th className="num">成本</th><th className="num">市值</th><th className="num">盈亏</th></tr></thead>
              <tbody>
                {positionsView.length === 0 && <tr><td colSpan={6} className="empty"><div className="mobile-position-empty"><b>暂无持仓</b><span>从行情选择股票，通过“交易”买入后会显示在这里。</span><button type="button" onClick={() => switchMobileTab("market")}>去看行情</button></div></td></tr>}
                {positionsView.map((p) => (
                  <tr key={p.code}>
                    <td className="mono">{p.code} {STOCK_NAMES[p.code]}</td>
                    <td className="num">{p.qty}</td>
                    <td className="num">{p.sellableQty}</td>
                    <td className="num">{yuan(p.avgCost)}</td>
                    <td className="num">{formatYuanAmount(p.marketValue / 100)}元</td>
                    <td className={`num ${colorClass(p.pnl)}`}>{p.pnl >= 0 ? "+" : ""}{formatYuanAmount(p.pnl / 100)}元</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>

        {/* 分时成交 */}
        <Card className="panel trades-panel" id="section-trades">
          <h3 className="panel-title">分时成交</h3>
          <div className="trade-feed">
            <table className="grid-table">
              <thead><tr><th>序号</th><th>代码</th><th className="num">成交价</th><th className="num">成交量（手）</th></tr></thead>
              <tbody>
                {trades.length === 0 && <tr><td colSpan={4} className="empty">等待成交…</td></tr>}
                {trades.map((t) => {
                  const m = snapshot.markets[t.code];
                  const diff = m ? t.price - m.last_close : 0;
                  return (
                    <tr key={t.seq}>
                      <td className="mono">{t.seq}</td>
                      <td className="mono">{t.code}</td>
                      <td className={`num ${colorClass(diff)}`}>{yuan(t.price)}</td>
                      <td className="num">{formatSharesAsLots(t.qty)}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </Card>

        <Card className="panel user-panel" id="section-user">
          <h3 className="panel-title">我的</h3>
          <section className="mobile-user-overview" aria-label="我的账户">
            <span>模拟账户</span>
            <strong>{formatYuanAmount(totalAssets / 100)}元</strong>
            <div>
              <span>可用资金 <b>{formatYuanAmount(availableCash / 100)}元</b></span>
              <span>持仓盈亏 <b className={colorClass(totalPnl)}>{totalPnl >= 0 ? "+" : ""}{formatYuanAmount(totalPnl / 100)}元</b></span>
            </div>
          </section>
          <section className="mobile-game-state" aria-label="游戏状态">
            <div><span>当前进度</span><b>第 {snapshot.day + 1} 个交易日</b></div>
            <div><span>模拟状态</span><b>{running ? "交易中" : "已暂停"}</b></div>
          </section>
          <div className="mobile-user-section">
            <h4>数据管理</h4>
            <button type="button" onClick={handleSave}>保存当前进度</button>
            <button type="button" onClick={handleLoad}>读取本地进度</button>
            <button type="button" onClick={handleSaveFile}>另存为文件</button>
            <button type="button" onClick={handleLoadFile}>从文件读取</button>
          </div>
        </Card>
      </div>

      {/* 移动端浮动交易按钮（贴 ref .ctrl-btn） */}
      {orientation === "portrait" && (
        <>
        {mobileTab === "market" && mobileDetail && snapshot.markets[chartCode] && (
          <div className="mobile-detail-page">
            <MobileStockDetail
              code={chartCode}
              name={STOCK_NAMES[chartCode] ?? chartCode}
              market={snapshot.markets[chartCode]}
              minutePoints={chartData}
              auctionPoints={auctionChartData}
              dailyCandles={dailyChartData}
              activeDailyCandle={activeDailyCandlesRef.current[chartCode]}
              trades={trades.filter((trade) => trade.code === chartCode)}
              elapsedMinutes={Math.min(elapsedMinutes, TRADING_MINUTES_PER_DAY)}
              totalMinutes={TRADING_MINUTES_PER_DAY}
              klineDays={klineDays}
              period={mobileUi.chartPeriod}
              infoTab={mobileUi.infoTab}
              speed={speed}
              running={running}
              gameDay={snapshot.day}
              gameTick={snapshot.tick}
              onKlineDaysChange={setKlineDays}
              onPeriodChange={(period) => dispatchMobileUi({ type: "select-period", period })}
              onInfoTabChange={showDetailInfo}
              onSpeedChange={(value) => store.dispatch(setSpeed(value))}
              onPauseToggle={handlePauseToggle}
              onBack={() => dispatchMobileUi({ type: "back" })}
              onPrevious={() => { const index = orderedMarketCodes.indexOf(chartCode); selectStock(orderedMarketCodes[(index - 1 + orderedMarketCodes.length) % orderedMarketCodes.length]); }}
              onNext={() => { const index = orderedMarketCodes.indexOf(chartCode); selectStock(orderedMarketCodes[(index + 1) % orderedMarketCodes.length]); }}
            />
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
      {notice && <div className="notice" role="status" aria-live="polite">{notice}</div>}
    </div>
  );
}

export default App;
