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
import { createWasmHost, ensureWasmReady, type EngineHost } from "./host/wasm-host";
import { createTauriHost } from "./host/tauri-host";
import { createWorkerHost } from "./host/worker-host";
import { DEFAULT_SEED, DEFAULT_SETUP, STOCK_LIST, STOCK_NAMES } from "./config/defaults";
import type { Cents, Intent, IntentRejectedEvent, SettlementErrorEvent } from "./types/engine";
import {
  appendTrades,
  applyEvents,
  setRunning,
  setSnapshot,
  setSpeed,
  setTheme,
  store,
  addAutoOrder,
  removeAutoOrder,
  toggleAutoOrder,
  clearTriggeredOrders,
  type RootState,
} from "./store/store";
import "./App.css";
import "ag-grid-community/styles/ag-grid.css";
import "ag-grid-community/styles/ag-theme-alpine.css";
import { PriceChart, type KlinePoint, type PricePoint } from "./components/PriceChart";
import { MarketGrid } from "./components/MarketGrid";
import { AutoOrderManager, AUTO_ORDER_LABELS, type AutoOrderType } from "./components/AutoOrders";
import { useOrientation } from "./hooks/useOrientation";
import { saveToFile, loadFromFile } from "./save/save-file";

const PLAYER_ACCOUNT_KEY = "0";
const MAX_DAILY_CANDLES = 360;

function yuan(cents: Cents): string {
  return (cents / 100).toFixed(2);
}

/** 盘口为空时没有可展示的买卖报价；该状态不是价格 0。 */
function quoteLevel(cents: Cents | null | undefined): string {
  return typeof cents === "number" ? yuan(cents) : "暂无";
}

/** 大额格式化：≥1亿显示"X.XX亿"，≥1万显示"X.XX万"，否则正常元。 */
export function bigYuan(cents: Cents): string {
  const v = cents / 100;
  if (Math.abs(v) >= 1e8) return (v / 1e8).toFixed(2) + "亿";
  if (Math.abs(v) >= 1e4) return (v / 1e4).toFixed(2) + "万";
  return v.toFixed(2);
}

/** 手数格式化。 */
export function lots(qty: number): string {
  const l = Math.round(qty / 100);
  if (l >= 10000) return (l / 10000).toFixed(2) + "万手";
  return String(l);
}
function colorClass(diff: number): string {
  if (diff > 0) return "up";
  if (diff < 0) return "down";
  return "flat";
}
function rejectionText(reason: IntentRejectedEvent["reason"]): string {
  switch (reason) {
    case "InsufficientCash": return "资金不足";
    case "InsufficientShares": return "持仓不足";
    case "LimitExceeded": return "超出涨跌停限制";
    case "UnknownStock": return "未知股票";
    default: return String(reason);
  }
}

function App() {
  const snapshot = useSelector((s: RootState) => s.snapshot.snapshot);
  const speed = useSelector((s: RootState) => s.settings.speed);
  const running = useSelector((s: RootState) => s.settings.running);
  const trades = useSelector((s: RootState) => s.trades.items);
  const theme = useSelector((s: RootState) => s.settings.theme);
  const autoOrders = useSelector((s: RootState) => s.autoOrders.items);
  const orientation = useOrientation();
  const [mobileTab, setMobileTab] = useState<"market" | "watchlist" | "positions" | "trades" | "user">("market");
  const [tradeSheetOpen, setTradeSheetOpen] = useState(false);
  const [mobileDetail, setMobileDetail] = useState(false); // 竖屏：列表 → 详情页切换

  // 移动端底页是一个真正的临时操作层：按 Esc 能安全退出，避免遮挡行情后无返回路径。
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") setTradeSheetOpen(false);
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  /** 移动端 tab → 切面板。 */
  function switchMobileTab(tab: "market" | "watchlist" | "positions" | "trades" | "user") {
    setMobileTab(tab);
    setMobileDetail(false); // 切 tab 时回到列表视角
  }

  /** 点击股票 → 选股 + 竖屏进入详情页。 */
  function selectStock(code: string) {
    setChartCode(code);
    setDailyChartData([...(dailyCandlesByCodeRef.current[code] ?? [])]);
    setTradeCode(code);
    const m = snapshot?.markets[code];
    if (m) setPriceText(yuan(m.last_price));
    if (orientation === "portrait") {
      setMobileDetail(true); // 竖屏：进入详情页（复刻同花顺）
    }
  }

  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  // 分时图：选中股票 + 价格历史
  const [chartCode, setChartCode] = useState<string>(STOCK_LIST[0].code);
  // 图表缓存必须按证券代码隔离，避免切股时把前一只股票的价格曲线绘入当前图表。
  const priceHistoryByCodeRef = useRef<Record<string, PricePoint[]>>({});
  const [chartData, setChartData] = useState<PricePoint[]>([]);
  // 已收盘日 K 与分时缓存分离：后者可在日界清空，前者保留完整的新局窗口。
  const dailyCandlesByCodeRef = useRef<Record<string, KlinePoint[]>>({});
  const activeDailyCandlesRef = useRef<Record<string, KlinePoint>>({});
  const [dailyChartData, setDailyChartData] = useState<KlinePoint[]>([]);
  const [chartPeriod, setChartPeriod] = useState<"分时" | "日K">("分时");
  const [klineDays, setKlineDays] = useState<number>(MAX_DAILY_CANDLES);

  /** 已收盘 K 加上盘中正在形成的 K；首个交易日也必须可见。 */
  function chartCandlesFor(code: string): KlinePoint[] {
    const completed = dailyCandlesByCodeRef.current[code] ?? [];
    const active = activeDailyCandlesRef.current[code];
    return active ? [...completed, active] : [...completed];
  }

  const hostRef = useRef<EngineHost | null>(null);
  const autoOrderMgrRef = useRef<AutoOrderManager | null>(null);

  // 稳定的事件回调
  const onEventsRef = useRef<(events: import("./types/engine").EngineEvent[]) => void>(() => {});

  // 初始化 AutoOrderManager（一次）
  if (!autoOrderMgrRef.current && hostRef.current) {
    autoOrderMgrRef.current = new AutoOrderManager((intent) => {
      try { hostRef.current?.submitIntent(intent); } catch { /* 忽略自动单提交失败 */ }
    });
  }

  // Worker 已在内部每 1 秒通知一次（合并事件），主线程直接处理即可，不需要额外节流。
  onEventsRef.current = (events) => {
    const fills: import("./types/engine").TradeEvent[] = [];
    let dayChanged = false;
    let selectedTick: PricePoint | null = null;
    let selectedDailyChanged = false;
    const currentDay = store.getState().snapshot.snapshot?.day ?? 0;
    for (const e of events) {
      if ("Trade" in e) fills.push(e.Trade);
      if ("PriceTick" in e) {
        const tick = e.PriceTick;
        const price = tick.last_price / 100;
        const existing = activeDailyCandlesRef.current[tick.code];
        if (existing) {
          existing.high = Math.max(existing.high, price);
          existing.low = Math.min(existing.low, price);
          existing.close = price;
        } else {
          activeDailyCandlesRef.current[tick.code] = {
            time: currentDay as KlinePoint["time"],
            open: price,
            high: price,
            low: price,
            close: price,
          };
        }
        if (tick.code === chartCode) {
          // Worker 已按刷新率合并同一股票的事件；直接消费 PriceTick，不能等待日界快照。
          selectedTick = { time: tick.seq, value: price };
          selectedDailyChanged = true;
        }
      }
      if ("DayBoundary" in e) {
        dayChanged = true;
        for (const [code, candle] of Object.entries(activeDailyCandlesRef.current)) {
          const history = dailyCandlesByCodeRef.current[code] ?? [];
          history.push(candle);
          dailyCandlesByCodeRef.current[code] = history.slice(-MAX_DAILY_CANDLES);
        }
        activeDailyCandlesRef.current = {};
      }
    }
    if (fills.length > 0) store.dispatch(appendTrades(fills));

    // 日界 → 重置分时图（新交易日 = 新的分时线）
    if (dayChanged) {
      priceHistoryByCodeRef.current = {};
      setChartData([]);
      setDailyChartData(chartCandlesFor(chartCode));
    }

    // 分时图须由盘中 PriceTick 驱动；Worker 只在日界推送快照，依赖快照会使折线整日停住。
    if (selectedTick) {
      const history = priceHistoryByCodeRef.current[chartCode] ?? [];
      history.push(selectedTick);
      if (history.length > 300) history.shift();
      priceHistoryByCodeRef.current[chartCode] = history;
      setChartData([...history]);
    }
    // 日K 同时展示盘中蜡烛，避免新游戏的第一个交易日切换后出现空白图。
    if (selectedDailyChanged && !dayChanged) {
      setDailyChartData(chartCandlesFor(chartCode));
    }

    for (const e of events) {
      if ("IntentRejected" in e) {
        const r = (e as { IntentRejected: IntentRejectedEvent }).IntentRejected;
        setNotice(`委托被拒：${r.code} — ${rejectionText(r.reason)}`);
      } else if ("SettlementError" in e) {
        const r = (e as { SettlementError: SettlementErrorEvent }).SettlementError;
        setNotice(`结算错误：${r.code} — ${r.reason}`);
      } else if ("VError" in e) {
        const r = (e as { VError: { code: string; reason: string } }).VError;
        setNotice(`估值错误：${r.code} — ${r.reason}`);
      }
    }

    const snap = store.getState().snapshot.snapshot;
    if (autoOrderMgrRef.current && snap) {
      autoOrderMgrRef.current.checkEvents(events, snap);
    }

    store.dispatch(applyEvents(events));
  };

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
    (async () => {
      try {
        const useTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
        let host: EngineHost;
        if (useTauri) {
          host = createTauriHost(DEFAULT_SETUP, DEFAULT_SEED);
        } else {
          // 优先用 Web Worker（不阻塞 UI）；失败则回退主线程
          try {
            host = await createWorkerHost(DEFAULT_SETUP, DEFAULT_SEED);
          } catch {
            await ensureWasmReady();
            host = createWasmHost(DEFAULT_SETUP, DEFAULT_SEED);
          }
        }
        if (cancelled) return;
        hostRef.current = host;
        // 初始化 AutoOrderManager
        autoOrderMgrRef.current = new AutoOrderManager((intent) => {
          try { hostRef.current?.submitIntent(intent); } catch { /* 忽略 */ }
        });
        // 同步 RTK autoOrders → Manager
        host.setSpeed(speed);
        host.start((events) => onEventsRef.current(events));
        store.dispatch(setSnapshot(host.snapshot()));
        store.dispatch(setRunning(true));
        if (!cancelled) setReady(true);
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? `${e.name}: ${e.message}` : String(e));
      }
    })();
    return () => { cancelled = true; hostRef.current?.stop(); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    try { hostRef.current?.setSpeed(speed); } catch (e) { setError(e instanceof Error ? e.message : String(e)); }
  }, [speed]);

  // 首帧以快照价格初始化。盘中后续点由上面的 PriceTick 事件直接累积。
  useEffect(() => {
    if (!snapshot) return;
    const m = snapshot.markets[chartCode];
    const history = priceHistoryByCodeRef.current[chartCode] ?? [];
    if (m && history.length === 0) {
      history.push({ time: 0, value: m.last_price / 100 });
      priceHistoryByCodeRef.current[chartCode] = history;
      setChartData([...history]);
    }
  }, [snapshot, chartCode]);

  useEffect(() => {
    setDailyChartData([...(dailyCandlesByCodeRef.current[chartCode] ?? [])]);
  }, [chartCode]);

  // 同步 RTK autoOrders → AutoOrderManager（仅在增删时触发）
  useEffect(() => {
    if (!autoOrderMgrRef.current) return;
    // RTK 是 UI 真源；Manager 的事件检查独立运行
  }, [autoOrders]);

  // 存档/读档
  const SAVE_KEY = "stock-game-save";
  async function handleSave() {
    if (!hostRef.current) return;
    try {
      const slot = await hostRef.current.save();
      localStorage.setItem(SAVE_KEY, JSON.stringify(slot));
      setNotice(`已存档（第 ${snapshot?.day ?? 0} 个交易日）`);
    } catch (e) { setNotice(`存档失败：${e}`); }
  }
  async function handleLoad() {
    if (!hostRef.current) return;
    try {
      const raw = localStorage.getItem(SAVE_KEY);
      if (!raw) { setNotice("无存档"); return; }
      const slot = JSON.parse(raw);
      // 清价格历史（加载后从头累积）
      priceHistoryByCodeRef.current = {};
      dailyCandlesByCodeRef.current = {};
      activeDailyCandlesRef.current = {};
      setChartData([]);
      setDailyChartData([]);
      await hostRef.current.load(slot);
      store.dispatch(setSnapshot(hostRef.current.snapshot()));
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
      // 清价格历史（加载后从头累积）
      priceHistoryByCodeRef.current = {};
      dailyCandlesByCodeRef.current = {};
      activeDailyCandlesRef.current = {};
      setChartData([]);
      setDailyChartData([]);
      await hostRef.current.load(slot);
      store.dispatch(setSnapshot(hostRef.current.snapshot()));
      setNotice(`已从文件读档（第 ${hostRef.current.day() + 1} 个交易日）`);
    } catch (e) { setNotice(`文件读档失败：${e}`); }
  }

  const handlePauseToggle = useCallback(() => {
    if (!hostRef.current) return;
    if (running) {
      hostRef.current.stop();
      store.dispatch(setRunning(false));
    } else {
      hostRef.current.start((events) => onEventsRef.current(events));
      store.dispatch(setRunning(true));
    }
  }, [running]);

  function buildIntent(side: "Buy" | "Sell"): Intent | null {
    const price = Math.round(Number(priceText) * 100);
    const qty = Math.round(Number(qtyText));
    if (!Number.isFinite(price) || price <= 0) { setNotice(`价格非法：${priceText}`); return null; }
    if (!Number.isFinite(qty) || qty <= 0 || qty % 100 !== 0) { setNotice(`数量必须为 100 的正整数倍`); return null; }
    return { PlaceLimit: { code: tradeCode, side, price, qty } };
  }

  function submit(side: "Buy" | "Sell") {
    const intent = buildIntent(side);
    if (!intent) return;
    try {
      hostRef.current?.submitIntent(intent);
      setNotice(`已提交${side === "Buy" ? "买入" : "卖出"}委托：${tradeCode} ${qtyText} 股 @ ${priceText} 元`);
    } catch (e) { setNotice(e instanceof Error ? e.message : String(e)); }
  }

  function addAuto() {
    const tp = Math.round(Number(autoTrigger) * 100);
    const qty = Math.round(Number(autoQty));
    if (!Number.isFinite(tp) || tp <= 0) { setNotice("触发价非法"); return; }
    if (!Number.isFinite(qty) || qty <= 0 || qty % 100 !== 0) { setNotice("数量须为 100 倍数"); return; }
    const side: "Buy" | "Sell" = (autoType === "stopProfit" || autoType === "stopLoss" || autoType === "sellTrigger") ? "Sell" : "Buy";
    const id = `auto-${Date.now()}`;
    store.dispatch(addAutoOrder({ id, code: tradeCode, type: autoType, triggerPrice: tp, qty, side, enabled: true, triggered: false }));
    autoOrderMgrRef.current?.add({ code: tradeCode, type: autoType, triggerPrice: tp, qty, side, enabled: true });
    setNotice(`已添加条件单：${AUTO_ORDER_LABELS[autoType]} ${tradeCode} @ ${autoTrigger} 元`);
  }

  const playerAccount = snapshot?.accounts[PLAYER_ACCOUNT_KEY] ?? null;
  const cash = playerAccount?.cash ?? 0;

  const positionsView = useMemo(() => {
    if (!snapshot || !playerAccount) return [];
    return Object.entries(playerAccount.positions)
      .filter(([, p]) => p.qty > 0)
      .map(([code, p]) => {
        const mkt = snapshot.markets[code];
        const cur = mkt?.last_price ?? 0;
        const avgCost = p.qty > 0 ? p.invested_cents / p.qty : 0;
        const marketValue = cur * p.qty;
        const pnl = marketValue - p.invested_cents;
        return { code, qty: p.qty, avgCost, marketValue, pnl };
      });
  }, [snapshot, playerAccount]);

  const totalMarketValue = positionsView.reduce((s, x) => s + x.marketValue, 0);
  const totalAssets = cash + totalMarketValue;
  const totalPnl = positionsView.reduce((s, x) => s + x.pnl, 0);
  const marketBreadth = Object.values(snapshot?.markets ?? {}).reduce(
    (summary, market) => {
      const change = market.last_price - market.last_close;
      if (change > 0) summary.up += 1;
      else if (change < 0) summary.down += 1;
      else summary.flat += 1;
      return summary;
    },
    { up: 0, down: 0, flat: 0 },
  );
  const remainingTicks = Math.max(0, DEFAULT_SETUP.ticks_per_day - (snapshot?.tick ?? 0));

  if (error) {
    return (
      <div className="app-error">
        <h2>引擎初始化失败</h2>
        <pre>{error}</pre>
      </div>
    );
  }
  if (!ready || !snapshot) {
    return <div className="app-loading">正在加载行情引擎…</div>;
  }

  return (
    <div className={`app-root ${orientation === "portrait" ? "layout-mobile" : "layout-desktop"}`} data-theme={theme}>
      {/* 顶栏 */}
      <header className="top-bar">
        <div className="brand">股票模拟行情终端</div>
        <div className="assets">
          <div className="asset"><span className="label">总资产</span><span className="value">{yuan(totalAssets)}</span><span className="unit">元</span></div>
          <div className="asset"><span className="label">可用资金</span><span className="value">{yuan(cash)}</span><span className="unit">元</span></div>
          <div className="asset"><span className="label">总盈亏</span><span className={`value ${colorClass(totalPnl)}`}>{totalPnl >= 0 ? "+" : ""}{yuan(totalPnl)}</span><span className="unit">元</span></div>
        </div>
        <div className="mobile-session-summary">
          <div className="mobile-speed-row" aria-label="模拟速度">
            {[1, 1.5, 2].map((value) => (
              <button key={value} type="button" className={speed === value ? "active" : ""} onClick={() => store.dispatch(setSpeed(value))}>{value}倍速</button>
            ))}
          </div>
          <div className="mobile-day-row">
            <span>第 {snapshot.day + 1} 个交易日</span>
            <strong>剩余 {remainingTicks} 分钟</strong>
          </div>
          <div className="mobile-state-row">
            <span>{running ? "交易中" : "已暂停"}</span>
            <span>上涨 {marketBreadth.up} · 下跌 {marketBreadth.down} · 平 {marketBreadth.flat}</span>
          </div>
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
              { label: "5x", value: "5" },
              { label: "10x", value: "10" },
              { label: "30x", value: "30" },
              { label: "180x", value: "180" },
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
          <MarketGrid snapshot={snapshot} selectedCode={chartCode} onSelect={selectStock} />
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
            const pct = m.last_close !== 0 ? (diff / m.last_close) * 100 : 0;
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
                <div className="ob-title">五档盘口</div>
                <div className="ob-rows">
                  {topAsks.map((lvl, i) => (
                    <div key={`a${i}`} className="ob-row ob-ask">
                      <span className="ob-label">卖{5 - i}</span>
                      <span className={`ob-price ${rowCls(lvl[0])}`}>{yuan(lvl[0])}</span>
                      <span className="ob-qty">{lvl[1]}</span>
                    </div>
                  ))}
                  <div className="ob-divider" />
                  {topBids.map((lvl, i) => (
                    <div key={`b${i}`} className="ob-row ob-bid">
                      <span className="ob-label">买{i + 1}</span>
                      <span className={`ob-price ${rowCls(lvl[0])}`}>{yuan(lvl[0])}</span>
                      <span className="ob-qty">{lvl[1]}</span>
                    </div>
                  ))}
                </div>
              </div>
            );
          })()}
        </Card>

        {/* 委托面板 + 自动单（移动端为底页弹出） */}
        <Card
          className={`panel order-panel ${orientation === "portrait" ? "mobile-sheet" : ""} ${tradeSheetOpen ? "sheet-open" : ""}`}
          id="section-order"
          role={orientation === "portrait" ? "dialog" : undefined}
          aria-modal={orientation === "portrait" ? true : undefined}
          aria-label={orientation === "portrait" ? "交易面板" : undefined}
        >
          <h3 className="panel-title">委托下单</h3>
          <label className="field"><span>股票</span>
            <HTMLSelect value={tradeCode} onChange={(e) => { setTradeCode(e.target.value); const m = snapshot.markets[e.target.value]; if (m) setPriceText(yuan(m.last_price)); }}
              options={STOCK_LIST.map((s) => ({ label: `${s.code} ${s.name}`, value: s.code }))} />
          </label>
          <label className="field"><span>价格（元）</span><InputGroup value={priceText} onChange={(e) => setPriceText(e.target.value)} placeholder="委托价" /></label>
          <label className="field"><span>数量（股）</span><InputGroup value={qtyText} onChange={(e) => setQtyText(e.target.value)} placeholder="100 的倍数" /></label>
          {/* 快速仓位按钮（贴 ref 全仓/1/2/1/3/1/4） */}
          <div className="quick-position">
            {(() => {
              const m = snapshot.markets[tradeCode];
              const price = m ? m.last_price : 0;
              const maxQty = price > 0 ? Math.floor(cash / price / 100) * 100 : 0;
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
            const upStop = Math.ceil(m.last_close * 1.1);
            const downStop = Math.floor(m.last_close * 0.9);
            const limitPct = m.last_close !== 0 ? (m.last_price - m.last_close) / m.last_close : 0;
            const isUpLimit = limitPct >= 0.099;
            return (
              <div className="limit-links">
                <button className="ll-btn down" onClick={() => setPriceText(yuan(downStop))}>跌停 {yuan(downStop)}</button>
                <button className="ll-btn up" onClick={() => setPriceText(yuan(upStop))} disabled={isUpLimit}>涨停 {yuan(upStop)}</button>
              </div>
            );
          })()}
          <div className="order-buttons">
            <Button intent="danger" onClick={() => submit("Buy")}>买入</Button>
            <Button intent="success" onClick={() => submit("Sell")}>卖出</Button>
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
              <Button small minimal onClick={() => store.dispatch(clearTriggeredOrders())}>清除已触发</Button>
            )}
          </div>

          {notice && <div className="notice" role="status" aria-live="polite">{notice}</div>}
          {/* 移动端底页关闭按钮 */}
          {orientation === "portrait" && (
            <button className="sheet-close" type="button" onClick={() => setTradeSheetOpen(false)}>收起交易面板</button>
          )}
        </Card>

        {/* 持仓 */}
        <Card className="panel pos-panel" id="section-positions">
          <h3 className="panel-title">持仓</h3>
          <table className="grid-table">
            <thead><tr><th>代码</th><th className="num">持仓</th><th className="num">成本</th><th className="num">市值</th><th className="num">盈亏</th></tr></thead>
            <tbody>
              {positionsView.length === 0 && <tr><td colSpan={5} className="empty">暂无持仓</td></tr>}
              {positionsView.map((p) => (
                <tr key={p.code}>
                  <td className="mono">{p.code} {STOCK_NAMES[p.code]}</td>
                  <td className="num">{p.qty}</td>
                  <td className="num">{yuan(p.avgCost)}</td>
                  <td className="num">{yuan(p.marketValue)}</td>
                  <td className={`num ${colorClass(p.pnl)}`}>{p.pnl >= 0 ? "+" : ""}{yuan(p.pnl)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>

        {/* 分时成交 */}
        <Card className="panel trades-panel" id="section-trades">
          <h3 className="panel-title">分时成交</h3>
          <div className="trade-feed">
            <table className="grid-table">
              <thead><tr><th>序号</th><th>代码</th><th className="num">成交价</th><th className="num">成交量</th></tr></thead>
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
                      <td className="num">{t.qty}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </Card>

        <Card className="panel user-panel" id="section-user">
          <h3 className="panel-title">我的</h3>
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
        {/* 竖屏：列表 → 详情页 切换（复刻同花顺手机版） */}
        {mobileTab === "market" && mobileDetail && (
          <div className="mobile-detail-page">
            {/* 红色头部 + 返回按钮（贴 ref .detail-head） */}
            <div className="mobile-detail-head">
              <button className="mobile-back-btn" type="button" aria-label="返回行情列表" onClick={() => setMobileDetail(false)}>‹</button>
              <div className="mobile-detail-title">
                <div className="detail-name" style={{ color: "#fff" }}>{STOCK_NAMES[chartCode] ?? chartCode}</div>
                <div className="detail-code" style={{ color: "rgba(255,255,255,0.8)" }}>{chartCode}</div>
              </div>
            </div>
            {/* 详情内容：大字现价 + 图表 + 盘口 + 成交（同一套组件，纵向排列） */}
            {(() => {
              const m = snapshot.markets[chartCode];
              if (!m) return null;
              const diff = m.last_price - m.last_close;
              const pct = m.last_close !== 0 ? (diff / m.last_close) * 100 : 0;
              const cls = colorClass(diff);
              return (
                <div className="mobile-detail-info">
                  <div className="mobile-detail-quote">
                    <div className={`mobile-detail-price ${cls}`}>{yuan(m.last_price)}</div>
                    <div className={`mobile-detail-change ${cls}`}>{diff >= 0 ? "+" : ""}{yuan(diff)} ({pct >= 0 ? "+" : ""}{pct.toFixed(2)}%)</div>
                  </div>
                  <div className="mobile-detail-stats" aria-label="股票报价摘要">
                    <span>昨收<b>{yuan(m.last_close)}</b></span>
                    <span>买一<b>{quoteLevel(m.best_bid)}</b></span>
                    <span>卖一<b>{quoteLevel(m.best_ask)}</b></span>
                    <span>涨停<b className="up">{yuan(Math.ceil(m.last_close * 1.1))}</b></span>
                    <span>跌停<b className="down">{yuan(Math.floor(m.last_close * 0.9))}</b></span>
                    <span>估值<b>{yuan(m.fundamental_value)}</b></span>
                  </div>
                </div>
              );
            })()}
            {/* 图表 tab（同一组件） */}
            <div className="chart-tabs">
              {(["分时", "日K"] as const).map((p) => (
                <button key={p} className={`chart-tab ${chartPeriod === p ? "active" : ""}`} onClick={() => setChartPeriod(p)}>{p}</button>
              ))}
            </div>
            <PriceChart data={chartData} dailyCandles={dailyChartData} lastClose={(snapshot.markets[chartCode]?.last_close ?? 0) / 100} chartType={chartPeriod} klineDays={klineDays} />
            {chartPeriod === "日K" && (
              <div className="kline-period-bar">
                {[20, 60, 120, 240, MAX_DAILY_CANDLES].map((d) => (
                  <button key={d} className={`kline-period-btn ${klineDays === d ? "active" : ""}`} onClick={() => setKlineDays(d)}>{d}日</button>
                ))}
              </div>
            )}
            {/* 五档盘口 + 分时成交（同一组件，纵向） */}
            {(() => {
              const m = snapshot.markets[chartCode];
              if (!m) return null;
              const topBids = m.bids.slice(0, 5);
              const topAsks = m.asks.slice(0, 5);
              const lc = m.last_close;
              const rowCls = (p: number) => p > lc ? "up" : p < lc ? "down" : "flat";
              return (
                <div className="mobile-detail-book">
                  <div className="order-book">
                    <div className="ob-title">五档盘口 — {STOCK_NAMES[chartCode] ?? chartCode}</div>
                    <div className="ob-rows">
                      {topAsks.map((lvl, i) => (
                        <div key={`a${i}`} className="ob-row ob-ask">
                          <span className="ob-label">卖{5 - i}</span>
                          <span className={`ob-price ${rowCls(lvl[0])}`}>{yuan(lvl[0])}</span>
                          <span className="ob-qty">{lvl[1]}</span>
                        </div>
                      ))}
                      <div className="ob-divider" />
                      {topBids.map((lvl, i) => (
                        <div key={`b${i}`} className="ob-row ob-bid">
                          <span className="ob-label">买{i + 1}</span>
                          <span className={`ob-price ${rowCls(lvl[0])}`}>{yuan(lvl[0])}</span>
                          <span className="ob-qty">{lvl[1]}</span>
                        </div>
                      ))}
                    </div>
                  </div>
                </div>
              );
            })()}
            {/* 分时成交（同一组件） */}
            <div className="mobile-detail-trades">
              <h4 className="auto-title">分时成交</h4>
              <div className="trade-feed">
                <table className="grid-table">
                  <thead><tr><th>序号</th><th>代码</th><th className="num">成交价</th><th className="num">成交量</th></tr></thead>
                  <tbody>
                    {trades.filter((t) => t.code === chartCode).slice(0, 30).map((t) => {
                      const m = snapshot.markets[t.code];
                      const diff = m ? t.price - m.last_close : 0;
                      return (
                        <tr key={t.seq}>
                          <td className="mono">{t.seq}</td>
                          <td className="mono">{t.code}</td>
                          <td className={`num ${colorClass(diff)}`}>{yuan(t.price)}</td>
                          <td className="num">{t.qty}</td>
                        </tr>
                      );
                    })}
                    {trades.filter((t) => t.code === chartCode).length === 0 && (
                      <tr><td colSpan={4} className="empty">等待成交…</td></tr>
                    )}
                  </tbody>
                </table>
              </div>
            </div>
          </div>
        )}
          <button className="float-trade-btn" type="button" aria-haspopup="dialog" aria-expanded={tradeSheetOpen} onClick={() => setTradeSheetOpen(true)}>交易</button>
        <nav className="mobile-tabbar" aria-label="主导航">
          {([["market", "行情"], ["watchlist", "自选"], ["trades", "交易"], ["positions", "持仓"], ["user", "我的"]] as const).map(([tab, label]) => (
            <button key={tab} className={`tab-btn ${mobileTab === tab ? "active" : ""}`} onClick={() => {
              if (tab === "trades") setTradeSheetOpen(true);
              else switchMobileTab(tab);
            }}>
              <span className={`tab-icon tab-icon-${tab}`} aria-hidden="true" />
              <span>{label}</span>
            </button>
          ))}
        </nav>
        {/* 底页遮罩 */}
        {tradeSheetOpen && <div className="sheet-mask" onClick={() => setTradeSheetOpen(false)} />}
        </>
      )}
    </div>
  );
}

export default App;
