import { useLayoutEffect, useMemo, useRef, type Dispatch, type SetStateAction } from "react";
import { useSelector } from "react-redux";
import { MarketGrid } from "../components/MarketGrid.tsx";
import { PriceChart } from "../components/PriceChart.tsx";
import { DEFAULT_SETUP, STOCK_LIST, STOCK_NAMES, TRADING_MINUTES_PER_DAY } from "../config/defaults.ts";
import { MobileGameClock } from "../mobile/MobileGameClock.tsx";
import { MobileStockDetail } from "../mobile/MobileStockDetail.tsx";
import { marketCodesForView, priceChangePercent } from "../mobile/market-model.ts";
import type { MobileChartPeriod, MobileInfoTab } from "../mobile/mobile-ui-state.ts";
import { setSpeed, store, type RootState } from "../store/store.ts";
import type { DeliveryMode } from "../host/engine-host.ts";
import { aSharePriceLimits } from "../utils/trade-input.ts";
import { colorClass, formatSharesAsLots, formatYuanAmount, yuan } from "../utils/format.ts";
import { useMarketRuntimeActions, useMarketRuntimeData, useMarketRuntimeSelection } from "./MarketRuntimeProvider.tsx";
import { portfolioInputEqual, selectPortfolioInput } from "./portfolio-selector.ts";

const PLAYER_ACCOUNT_KEY = "0";
const MAX_DAILY_CANDLES = 360;

function usePortfolio() {
  const { account, heldPrices } = useSelector(selectPortfolioInput, portfolioInputEqual);
  return useMemo(() => {
    const cash = account?.cash ?? 0;
    const availableCash = cash - (account?.reserved_cash ?? 0);
    const positions = !account ? [] : Object.entries(account.positions)
      .filter(([, position]) => position.qty > 0)
      .map(([code, position]) => {
        const currentPrice = heldPrices[code] ?? 0;
        const netInvested = position.invested_cents - position.recovered_cents;
        const marketValue = currentPrice * position.qty;
        return {
          code,
          qty: position.qty,
          sellableQty: Math.max(0, position.qty - position.t1_locked - (account.reserved_sell_qty[code] ?? 0)),
          avgCost: position.qty > 0 ? netInvested / position.qty : 0,
          marketValue,
          pnl: marketValue - netInvested,
        };
      });
    const totalMarketValue = positions.reduce((sum, position) => sum + position.marketValue, 0);
    const totalPnl = positions.reduce((sum, position) => sum + position.pnl, 0);
    return { positions, availableCash, totalMarketValue, totalAssets: cash + totalMarketValue, totalPnl };
  }, [account, heldPrices]);
}

export function ClockMarker() {
  const day = useSelector((state: RootState) => state.snapshot.snapshot?.day ?? 0);
  const tick = useSelector((state: RootState) => state.snapshot.snapshot?.tick ?? 0);
  const markerRef = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    const root = markerRef.current?.parentElement;
    if (!root) throw new Error("游戏时钟诊断标记必须直接挂载在应用根节点");
    root.dataset.gameDay = String(day);
    root.dataset.gameTick = String(tick);
  }, [day, tick]);
  return <span ref={markerRef} hidden data-game-day={day} data-game-tick={tick} />;
}

export function ConnectedMobileGameClock() {
  const day = useSelector((state: RootState) => state.snapshot.snapshot?.day ?? 0);
  const tick = useSelector((state: RootState) => state.snapshot.snapshot?.tick ?? 0);
  return <MobileGameClock day={day} tick={tick} variant="global" />;
}

export function DesktopDayTag() {
  const day = useSelector((state: RootState) => state.snapshot.snapshot?.day ?? 0);
  return <span className="day-tag">第 {day + 1} 个交易日</span>;
}

export function DesktopAssets() {
  const { totalAssets, availableCash, totalPnl } = usePortfolio();
  return <div className="assets">
    <div className="asset"><span className="label">总资产</span><span className="value">{formatYuanAmount(totalAssets / 100)}</span><span className="unit">元</span></div>
    <div className="asset"><span className="label">可用资金</span><span className="value">{formatYuanAmount(availableCash / 100)}</span><span className="unit">元</span></div>
    <div className="asset"><span className="label">总盈亏</span><span className={`value ${colorClass(totalPnl)}`}>{totalPnl >= 0 ? "+" : ""}{formatYuanAmount(totalPnl / 100)}</span><span className="unit">元</span></div>
  </div>;
}

interface MarketPanelProps { onSelect: (code: string) => void }
export function ConnectedMarketPanel({ onSelect }: MarketPanelProps) {
  const markets = useSelector((state: RootState) => state.snapshot.snapshot?.markets ?? {});
  const account = useSelector((state: RootState) => state.snapshot.snapshot?.accounts[PLAYER_ACCOUNT_KEY] ?? null);
  const chartCode = useMarketRuntimeSelection();
  const { priceHistoryByCodeRef } = useMarketRuntimeActions();
  const heldCodes = useMemo(() => new Set(Object.entries(account?.positions ?? {}).filter(([, position]) => position.qty > 0).map(([code]) => code)), [account]);
  return <MarketGrid markets={markets} selectedCode={chartCode} onSelect={onSelect} heldCodes={heldCodes} priceHistoryByCode={priceHistoryByCodeRef.current} />;
}

interface ChartPanelProps {
  chartPeriod: "分时" | "日K";
  setChartPeriod: Dispatch<SetStateAction<"分时" | "日K">>;
  klineDays: number;
  setKlineDays: Dispatch<SetStateAction<number>>;
}
export function ConnectedChartPanel({ chartPeriod, setChartPeriod, klineDays, setKlineDays }: ChartPanelProps) {
  const chartCode = useMarketRuntimeSelection();
  const market = useSelector((state: RootState) => state.snapshot.snapshot?.markets[chartCode]);
  const { chartData, dailyChartData } = useMarketRuntimeData();
  if (!market) return null;
  const diff = market.last_price - market.last_close;
  const pct = priceChangePercent(market.last_price, market.last_close);
  const cls = colorClass(diff);
  const rowCls = (price: number) => price > market.last_close ? "up" : price < market.last_close ? "down" : "flat";
  return <>
    <div className="chart-tabs">{(["分时", "日K"] as const).map((period) => <button key={period} className={`chart-tab ${chartPeriod === period ? "active" : ""}`} onClick={() => setChartPeriod(period)}>{period}</button>)}</div>
    <div className="stock-detail-header"><div className="detail-left"><div className="detail-name">{STOCK_NAMES[chartCode] ?? chartCode}</div><div className="detail-code">{chartCode}</div></div><div className="detail-prices"><span className={`detail-price ${cls}`}>{yuan(market.last_price)}</span><span className={`detail-change ${cls}`}>{diff >= 0 ? "+" : ""}{yuan(diff)} ({pct >= 0 ? "+" : ""}{pct.toFixed(2)}%)</span></div></div>
    <PriceChart data={chartData} dailyCandles={dailyChartData} lastClose={market.last_close / 100} chartType={chartPeriod} klineDays={klineDays} />
    {chartPeriod === "日K" && <div className="kline-period-bar">{[20, 60, 120, 240, MAX_DAILY_CANDLES].map((days) => <button key={days} className={`kline-period-btn ${klineDays === days ? "active" : ""}`} onClick={() => setKlineDays(days)}>{days}日</button>)}</div>}
    <div className="order-book"><div className="ob-title">五档盘口（手）</div><div className="ob-rows">
      {market.asks.slice(0, 5).map((level, index) => <div key={`a${index}`} className="ob-row ob-ask"><span className="ob-label">卖{5 - index}</span><span className={`ob-price ${rowCls(level[0])}`}>{yuan(level[0])}</span><span className="ob-qty">{formatSharesAsLots(level[1])}</span></div>)}
      <div className="ob-divider" />
      {market.bids.slice(0, 5).map((level, index) => <div key={`b${index}`} className="ob-row ob-bid"><span className="ob-label">买{index + 1}</span><span className={`ob-price ${rowCls(level[0])}`}>{yuan(level[0])}</span><span className="ob-qty">{formatSharesAsLots(level[1])}</span></div>)}
    </div></div>
  </>;
}

interface TradeMarketControlsProps { tradeCode: string; setPriceText: Dispatch<SetStateAction<string>>; setQtyText: Dispatch<SetStateAction<string>> }
export function TradeMarketControls({ tradeCode, setPriceText, setQtyText }: TradeMarketControlsProps) {
  const market = useSelector((state: RootState) => state.snapshot.snapshot?.markets[tradeCode]);
  const account = useSelector((state: RootState) => state.snapshot.snapshot?.accounts[PLAYER_ACCOUNT_KEY]);
  const availableCash = (account?.cash ?? 0) - (account?.reserved_cash ?? 0);
  const price = market?.last_price ?? 0;
  const maxQty = price > 0 ? Math.floor(availableCash / price / 100) * 100 : 0;
  return <>
    <div className="quick-position">{[{ label: "全仓", pct: 1 }, { label: "1/2", pct: .5 }, { label: "1/3", pct: 1 / 3 }, { label: "1/4", pct: .25 }].map((button) => { const quantity = Math.floor((maxQty * button.pct) / 100) * 100; return <button key={button.label} className="qp-btn" onClick={() => setQtyText(String(Math.max(100, quantity)))} disabled={quantity < 100}>{button.label}</button>; })}</div>
    {market && (() => { const stock = DEFAULT_SETUP.stocks.find((candidate) => candidate.code === tradeCode); if (!stock) return <div className="limit-links" role="alert">缺少 {tradeCode} 的交易规则</div>; const { up, down } = aSharePriceLimits(market.last_close, stock.category); return <div className="limit-links"><button className="ll-btn down" onClick={() => setPriceText(yuan(down))}>跌停 {yuan(down)}</button><button className="ll-btn up" onClick={() => setPriceText(yuan(up))} disabled={market.last_price >= up}>涨停 {yuan(up)}</button></div>; })()}
  </>;
}

export function PositionsPanel({ onOpenMarket }: { onOpenMarket: () => void }) {
  const { positions, availableCash, totalMarketValue, totalAssets, totalPnl } = usePortfolio();
  return <><section className="mobile-portfolio-summary" aria-label="账户资产概览"><div className="mobile-assets-total"><span>总资产</span><strong>{formatYuanAmount(totalAssets / 100)}元</strong><small>可用资金 {formatYuanAmount(availableCash / 100)}元</small></div><div><span>持仓市值</span><b>{formatYuanAmount(totalMarketValue / 100)}元</b></div><div><span>浮动盈亏</span><b className={colorClass(totalPnl)}>{totalPnl >= 0 ? "+" : ""}{formatYuanAmount(totalPnl / 100)}元</b></div></section><div className="mobile-position-list-head"><strong>我的持仓</strong><span>{positions.length} 只</span></div><div className="mobile-position-table-wrap"><table className="grid-table"><thead><tr><th>代码</th><th className="num">持仓</th><th className="num">可卖</th><th className="num">成本</th><th className="num">市值</th><th className="num">盈亏</th></tr></thead><tbody>
    {positions.length === 0 && <tr><td colSpan={6} className="empty"><div className="mobile-position-empty"><b>暂无持仓</b><span>从行情选择股票，通过“交易”买入后会显示在这里。</span><button type="button" onClick={onOpenMarket}>去看行情</button></div></td></tr>}
    {positions.map((position) => <tr key={position.code}><td className="mono">{position.code} {STOCK_NAMES[position.code]}</td><td className="num">{position.qty}</td><td className="num">{position.sellableQty}</td><td className="num">{yuan(position.avgCost)}</td><td className="num">{formatYuanAmount(position.marketValue / 100)}元</td><td className={`num ${colorClass(position.pnl)}`}>{position.pnl >= 0 ? "+" : ""}{formatYuanAmount(position.pnl / 100)}元</td></tr>)}
  </tbody></table></div></>;
}

export function TradesPanel() {
  const trades = useSelector((state: RootState) => state.trades.items);
  const lastCloses = useSelector(
    (state: RootState) => Object.fromEntries(Object.entries(state.snapshot.snapshot?.markets ?? {}).map(([code, market]) => [code, market.last_close])),
    (left, right) => Object.keys(left).length === Object.keys(right).length && Object.entries(left).every(([code, value]) => right[code] === value),
  );
  return <div className="trade-feed"><table className="grid-table"><thead><tr><th>序号</th><th>代码</th><th className="num">成交价</th><th className="num">成交量（手）</th></tr></thead><tbody>{trades.length === 0 && <tr><td colSpan={4} className="empty">等待成交…</td></tr>}{trades.map((trade) => { const diff = trade.price - (lastCloses[trade.code] ?? trade.price); return <tr key={trade.seq}><td className="mono">{trade.seq}</td><td className="mono">{trade.code}</td><td className={`num ${colorClass(diff)}`}>{yuan(trade.price)}</td><td className="num">{formatSharesAsLots(trade.qty)}</td></tr>; })}</tbody></table></div>;
}

interface UserPanelProps { running: boolean; deliveryMode: DeliveryMode | null; deliveryModes: readonly DeliveryMode[]; deliveryLabels: Record<DeliveryMode, string>; onDeliveryModeChange: (mode: DeliveryMode) => void; onSave: () => void; onLoad: () => void; onSaveFile: () => void; onLoadFile: () => void }
export function UserPanel(props: UserPanelProps) {
  const day = useSelector((state: RootState) => state.snapshot.snapshot?.day ?? 0);
  const { totalAssets, availableCash, totalPnl } = usePortfolio();
  return <><section className="mobile-user-overview" aria-label="我的账户"><span>模拟账户</span><strong>{formatYuanAmount(totalAssets / 100)}元</strong><div><span>可用资金 <b>{formatYuanAmount(availableCash / 100)}元</b></span><span>持仓盈亏 <b className={colorClass(totalPnl)}>{totalPnl >= 0 ? "+" : ""}{formatYuanAmount(totalPnl / 100)}元</b></span></div></section><section className="mobile-game-state" aria-label="游戏状态"><div><span>当前进度</span><b>第 {day + 1} 个交易日</b></div><div><span>模拟状态</span><b>{props.running ? "交易中" : "已暂停"}</b></div>{props.deliveryMode !== null && props.deliveryModes.length > 0 && <div><label htmlFor="mobile-delivery-mode">刷新方式</label><select id="mobile-delivery-mode" value={props.deliveryMode} onChange={(event) => props.onDeliveryModeChange(event.target.value as DeliveryMode)}>{props.deliveryModes.map((mode) => <option key={mode} value={mode}>{props.deliveryLabels[mode]}</option>)}</select></div>}</section><div className="mobile-user-section"><h4>数据管理</h4><button type="button" onClick={props.onSave}>保存当前进度</button><button type="button" onClick={props.onLoad}>读取本地进度</button><button type="button" onClick={props.onSaveFile}>另存为文件</button><button type="button" onClick={props.onLoadFile}>从文件读取</button></div></>;
}

interface MobileDetailProps { klineDays: number; setKlineDays: Dispatch<SetStateAction<number>>; period: MobileChartPeriod; infoTab: MobileInfoTab; speed: number; measuredSpeed: string; measuredSpeedTitle: string; running: boolean; onPeriodChange: (period: MobileChartPeriod) => void; onInfoTabChange: (tab: MobileInfoTab) => void; onPauseToggle: () => void; onBack: () => void; onSelect: (code: string) => void }
export function ConnectedMobileDetail(props: MobileDetailProps) {
  const chartCode = useMarketRuntimeSelection();
  const { chartData, auctionChartData, dailyChartData } = useMarketRuntimeData();
  const { activeDailyCandlesRef } = useMarketRuntimeActions();
  const market = useSelector((state: RootState) => state.snapshot.snapshot?.markets[chartCode]);
  const marketCodes = useSelector((state: RootState) => Object.keys(state.snapshot.snapshot?.markets ?? {}));
  const account = useSelector((state: RootState) => state.snapshot.snapshot?.accounts[PLAYER_ACCOUNT_KEY]);
  const allTrades = useSelector((state: RootState) => state.trades.items);
  const trades = useMemo(() => allTrades.filter((trade) => trade.code === chartCode), [allTrades, chartCode]);
  const day = useSelector((state: RootState) => state.snapshot.snapshot?.day ?? 0);
  const tick = useSelector((state: RootState) => state.snapshot.snapshot?.tick ?? 0);
  if (!market) return null;
  const heldCodes = new Set(Object.entries(account?.positions ?? {}).filter(([, position]) => position.qty > 0).map(([code]) => code));
  const orderedCodes = marketCodesForView(marketCodes, STOCK_LIST.map((stock) => stock.code), "watchlist", heldCodes);
  const index = orderedCodes.indexOf(chartCode);
  const latestMinute = chartData.at(-1)?.time;
  const elapsedMinutes = latestMinute === undefined ? 0 : Math.min(TRADING_MINUTES_PER_DAY, Math.floor(latestMinute) + 1);
  return <MobileStockDetail code={chartCode} name={STOCK_NAMES[chartCode] ?? chartCode} market={market} minutePoints={chartData} auctionPoints={auctionChartData} dailyCandles={dailyChartData} activeDailyCandle={activeDailyCandlesRef.current[chartCode]} trades={trades} elapsedMinutes={elapsedMinutes} totalMinutes={TRADING_MINUTES_PER_DAY} klineDays={props.klineDays} period={props.period} infoTab={props.infoTab} speed={props.speed} measuredSpeed={props.measuredSpeed} measuredSpeedTitle={props.measuredSpeedTitle} running={props.running} gameDay={day} gameTick={tick} onKlineDaysChange={props.setKlineDays} onPeriodChange={props.onPeriodChange} onInfoTabChange={props.onInfoTabChange} onSpeedChange={(value) => store.dispatch(setSpeed(value))} onPauseToggle={props.onPauseToggle} onBack={props.onBack} onPrevious={() => props.onSelect(orderedCodes[(index - 1 + orderedCodes.length) % orderedCodes.length])} onNext={() => props.onSelect(orderedCodes[(index + 1) % orderedCodes.length])} />;
}
