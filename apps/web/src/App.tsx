/**
 * 应用根组件。
 *
 * 挂载时：加载 wasm + 创建会话 + 启动 host，把首张快照写入 store。
 * 渲染：顶栏（资产/速度/暂停）/ 行情表 / 分时成交 / 委托面板 / 持仓。
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { Button, Card, InputGroup, HTMLSelect } from "@blueprintjs/core";
import { useSelector } from "react-redux";
import { createWasmHost, ensureWasmReady, type EngineHost } from "./host/wasm-host";
import { DEFAULT_SEED, DEFAULT_SETUP, STOCK_LIST, STOCK_NAMES } from "./config/defaults";
import type { Cents, Intent, IntentRejectedEvent, SettlementErrorEvent } from "./types/engine";
import {
  appendTrades,
  applyEvents,
  setRunning,
  setSnapshot,
  setSpeed,
  store,
  type RootState,
} from "./store/store";
import "./App.css";

const PLAYER_ACCOUNT_KEY = "0"; // AccountId(0)

/** 分 → 元（保留 2 位），用于金额显示。 */
function yuan(cents: Cents): string {
  return (cents / 100).toFixed(2);
}

/** 涨跌方向 → 颜色 class。 */
function colorClass(diff: number): string {
  if (diff > 0) return "up";
  if (diff < 0) return "down";
  return "flat";
}

function App() {
  const snapshot = useSelector((s: RootState) => s.snapshot.snapshot);
  const speed = useSelector((s: RootState) => s.settings.speed);
  const running = useSelector((s: RootState) => s.settings.running);
  const trades = useSelector((s: RootState) => s.trades.items);

  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const hostRef = useRef<EngineHost | null>(null);
  // 稳定的事件回调：保证「继续」时 start() 注册的是同一份处理逻辑。
  const onEventsRef = useRef<(events: import("./types/engine").EngineEvent[]) => void>(() => {});

  onEventsRef.current = (events) => {
    const fills: import("./types/engine").TradeEvent[] = [];
    for (const e of events) {
      if ("Trade" in e) fills.push(e.Trade);
    }
    if (fills.length > 0) store.dispatch(appendTrades(fills));

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

    store.dispatch(applyEvents(events));
  };

  // 委托面板状态
  const [tradeCode, setTradeCode] = useState<string>(STOCK_LIST[0].code);
  const [priceText, setPriceText] = useState<string>("");
  const [qtyText, setQtyText] = useState<string>("100");

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        await ensureWasmReady();
        if (cancelled) return;
        const host = createWasmHost(DEFAULT_SETUP, DEFAULT_SEED);
        hostRef.current = host;
        host.setSpeed(speed);
        host.start((events) => onEventsRef.current(events));
        // 写入首张快照
        store.dispatch(setSnapshot(host.snapshot()));
        store.dispatch(setRunning(true));
        if (!cancelled) setReady(true);
      } catch (e) {
        if (!cancelled) {
          setError(e instanceof Error ? `${e.name}: ${e.message}` : String(e));
        }
      }
    })();
    return () => {
      cancelled = true;
      hostRef.current?.stop();
    };
    // 仅在挂载时执行一次
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 速度变化 → 同步 host
  useEffect(() => {
    try {
      hostRef.current?.setSpeed(speed);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [speed]);

  function handlePauseToggle() {
    if (!hostRef.current) return;
    if (running) {
      hostRef.current.stop();
      store.dispatch(setRunning(false));
    } else {
      // 继续：复用同一份事件处理回调，重启定时器。
      hostRef.current.start((events) => onEventsRef.current(events));
      store.dispatch(setRunning(true));
    }
  }

  function buildIntent(side: "Buy" | "Sell"): Intent | null {
    const code = tradeCode;
    const price = Math.round(Number(priceText) * 100);
    const qty = Math.round(Number(qtyText));
    if (!Number.isFinite(price) || price <= 0) {
      setNotice(`价格非法：${priceText}`);
      return null;
    }
    if (!Number.isFinite(qty) || qty <= 0 || qty % 100 !== 0) {
      setNotice(`数量必须为 100 的正整数倍，当前：${qtyText}`);
      return null;
    }
    return { PlaceLimit: { code, side, price, qty } };
  }

  function submit(side: "Buy" | "Sell") {
    const intent = buildIntent(side);
    if (!intent) return;
    try {
      hostRef.current?.submitIntent(intent);
      setNotice(`已提交${side === "Buy" ? "买入" : "卖出"}委托：${tradeCode} ${qtyText} 股 @ ${priceText || "(市价)"} 元`);
    } catch (e) {
      setNotice(e instanceof Error ? e.message : String(e));
    }
  }

  const playerAccount = snapshot?.accounts[PLAYER_ACCOUNT_KEY] ?? null;
  const cash = playerAccount?.cash ?? 0;

  // 持仓 + 总市值 / 盈亏
  const positionsView = useMemo(() => {
    if (!snapshot || !playerAccount) return [];
    return Object.entries(playerAccount.positions)
      .filter(([, p]) => p.qty > 0)
      .map(([code, p]) => {
        const mkt = snapshot.markets[code];
        const cur = mkt?.last_price ?? 0;
        // 成本均价：投入/持仓（cents）；当前市值；盈亏 = (现价-均价)*qty
        const avgCost = p.qty > 0 ? p.invested_cents / p.qty : 0;
        const marketValue = cur * p.qty;
        const pnl = marketValue - p.invested_cents;
        return { code, qty: p.qty, avgCost, marketValue, pnl };
      });
  }, [snapshot, playerAccount]);

  const totalMarketValue = positionsView.reduce((s, x) => s + x.marketValue, 0);
  const totalAssets = cash + totalMarketValue;
  const totalPnl = positionsView.reduce((s, x) => s + x.pnl, 0);

  if (error) {
    return (
      <div className="app-error">
        <h2>引擎初始化失败</h2>
        <pre>{error}</pre>
        <p>请刷新页面重试；若持续失败请检查 wasm-pkg 是否就位。</p>
      </div>
    );
  }

  if (!ready || !snapshot) {
    return <div className="app-loading">正在加载行情引擎…</div>;
  }

  return (
    <div className="app-root">
      {/* 顶栏 */}
      <header className="top-bar">
        <div className="brand">股票模拟行情终端</div>
        <div className="assets">
          <div className="asset">
            <span className="label">总资产</span>
            <span className="value">{yuan(totalAssets)}</span>
            <span className="unit">元</span>
          </div>
          <div className="asset">
            <span className="label">可用资金</span>
            <span className="value">{yuan(cash)}</span>
            <span className="unit">元</span>
          </div>
          <div className="asset">
            <span className="label">总盈亏</span>
            <span className={`value ${colorClass(totalPnl)}`}>
              {totalPnl >= 0 ? "+" : ""}
              {yuan(totalPnl)}
            </span>
            <span className="unit">元</span>
          </div>
        </div>
        <div className="controls">
          <span className="label">速度</span>
          <HTMLSelect
            value={String(speed)}
            onChange={(e) => store.dispatch(setSpeed(Number(e.target.value)))}
            options={[
              { label: "1x", value: "1" },
              { label: "2x", value: "2" },
              { label: "5x", value: "5" },
            ]}
          />
          <Button intent={running ? "danger" : "success"} onClick={handlePauseToggle}>
            {running ? "暂停" : "继续"}
          </Button>
          <span className="day-tag">第 {snapshot.day + 1} 个交易日</span>
        </div>
      </header>

      <div className="app-grid">
        {/* 行情表 */}
        <Card className="panel market-panel">
          <h3 className="panel-title">行情</h3>
          <table className="grid-table">
            <thead>
              <tr>
                <th>代码</th>
                <th>名称</th>
                <th className="num">现价</th>
                <th className="num">涨跌额</th>
                <th className="num">涨跌幅</th>
              </tr>
            </thead>
            <tbody>
              {STOCK_LIST.map((s) => {
                const m = snapshot.markets[s.code];
                if (!m) return null;
                const diff = m.last_price - m.last_close;
                const pct = m.last_close !== 0 ? (diff / m.last_close) * 100 : 0;
                const cls = colorClass(diff);
                return (
                  <tr key={s.code}>
                    <td className="mono">{s.code}</td>
                    <td>{s.name}</td>
                    <td className={`num ${cls}`}>{yuan(m.last_price)}</td>
                    <td className={`num ${cls}`}>
                      {diff >= 0 ? "+" : ""}
                      {yuan(diff)}
                    </td>
                    <td className={`num ${cls}`}>
                      {pct >= 0 ? "+" : ""}
                      {pct.toFixed(2)}%
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </Card>

        {/* 委托面板 */}
        <Card className="panel order-panel">
          <h3 className="panel-title">委托下单</h3>
          <label className="field">
            <span>股票</span>
            <HTMLSelect
              value={tradeCode}
              onChange={(e) => {
                setTradeCode(e.target.value);
                const m = snapshot.markets[e.target.value];
                if (m) setPriceText(yuan(m.last_price));
              }}
              options={STOCK_LIST.map((s) => ({ label: `${s.code} ${s.name}`, value: s.code }))}
            />
          </label>
          <label className="field">
            <span>价格（元）</span>
            <InputGroup
              value={priceText}
              onChange={(e) => setPriceText(e.target.value)}
              placeholder="委托价"
            />
          </label>
          <label className="field">
            <span>数量（股）</span>
            <InputGroup
              value={qtyText}
              onChange={(e) => setQtyText(e.target.value)}
              placeholder="100 的倍数"
            />
          </label>
          <div className="order-buttons">
            <Button intent="danger" onClick={() => submit("Buy")}>
              买入
            </Button>
            <Button intent="success" onClick={() => submit("Sell")}>
              卖出
            </Button>
          </div>
          {notice && <div className="notice">{notice}</div>}
        </Card>

        {/* 持仓 */}
        <Card className="panel pos-panel">
          <h3 className="panel-title">持仓</h3>
          <table className="grid-table">
            <thead>
              <tr>
                <th>代码</th>
                <th className="num">持仓</th>
                <th className="num">成本</th>
                <th className="num">市值</th>
                <th className="num">盈亏</th>
              </tr>
            </thead>
            <tbody>
              {positionsView.length === 0 && (
                <tr>
                  <td colSpan={5} className="empty">
                    暂无持仓
                  </td>
                </tr>
              )}
              {positionsView.map((p) => (
                <tr key={p.code}>
                  <td className="mono">
                    {p.code} {STOCK_NAMES[p.code]}
                  </td>
                  <td className="num">{p.qty}</td>
                  <td className="num">{yuan(p.avgCost)}</td>
                  <td className="num">{yuan(p.marketValue)}</td>
                  <td className={`num ${colorClass(p.pnl)}`}>
                    {p.pnl >= 0 ? "+" : ""}
                    {yuan(p.pnl)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>

        {/* 分时成交 */}
        <Card className="panel trades-panel">
          <h3 className="panel-title">分时成交</h3>
          <div className="trade-feed">
            <table className="grid-table">
              <thead>
                <tr>
                  <th>序号</th>
                  <th>代码</th>
                  <th className="num">成交价</th>
                  <th className="num">成交量</th>
                </tr>
              </thead>
              <tbody>
                {trades.length === 0 && (
                  <tr>
                    <td colSpan={4} className="empty">
                      等待成交…
                    </td>
                  </tr>
                )}
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
      </div>
    </div>
  );
}

/** 把引擎的拒单原因枚举翻成中文提示。 */
function rejectionText(reason: IntentRejectedEvent["reason"]): string {
  switch (reason) {
    case "InsufficientCash":
      return "资金不足";
    case "InsufficientShares":
      return "持仓不足";
    case "LimitExceeded":
      return "超出涨跌停限制";
    case "UnknownStock":
      return "未知股票";
    default:
      return String(reason);
  }
}

export default App;
