import type { SecurityBrowser } from "./useSecurityBrowser.ts";
import { sortSecurityCodes, mobileSecuritySort } from "./security-sort-model.ts";
import { filterSecurityCodes, adjacentSecurityCode, securityListEmptyMessage, securityListKeyboardTarget } from "./security-browser-model.ts";
import { SecurityListControls, WatchlistToggle } from "../components/SecurityListControls.tsx";
import { MarketKlinePanel } from "../components/MarketKlinePanel.tsx";
import { ChartPeriodTabs } from "../components/ChartPeriodTabs.tsx";
import { MarketQuotePanel } from "../components/MarketQuotePanel.tsx";
import { addMoney, subtractMoney, compareMoney } from "../utils/money.ts";
import { useEffect, useId, useLayoutEffect, useMemo, useRef, type Dispatch, type SetStateAction } from "react";
import { useSelector } from "react-redux";
import { CompanyPanel } from "../components/company/CompanyPanel.tsx";
import { publicCompanyForStock } from "../components/company/company-catalog.ts";
import { MarketGrid } from "../components/MarketGrid.tsx";
import { PriceChart } from "../components/PriceChart.tsx";
import { STOCK_LIST, STOCK_NAMES, TRADING_MINUTES_PER_DAY } from "../config/defaults.ts";
import { MobileGameClock } from "../mobile/MobileGameClock.tsx";
import { MobileStockDetail } from "../mobile/MobileStockDetail.tsx";
import { marketCodesForView, priceChangePercent, MobileIntradayProjection } from "../mobile/market-model.ts";
import type { MobileChartPeriod, MobileInfoTab } from "../mobile/mobile-ui-state.ts";
import { store, type RootState } from "../store/store.ts";
import { selectCompany, updateCompanyReading, type CompanyReading } from "../store/company-slice.ts";
import type { DeliveryMode } from "../host/engine-host.ts";
import { colorClass, formatSharesAsLots, formatCentsAmount, yuan } from "../utils/format.ts";
import { useTradingTimeline } from "../components/TradingTimelineContext.tsx";
import { useMarketRuntimeActions, useMarketRuntimeData, useMarketRuntimeSelection } from "./MarketRuntimeProvider.tsx";
import { portfolioInputEqual, selectPortfolioInput } from "./portfolio-selector.ts";
import { valueHeldPosition } from "./position-valuation.ts";

const PLAYER_ACCOUNT_KEY = "0";

function usePortfolio() {
  const { account, heldPrices } = useSelector(selectPortfolioInput, portfolioInputEqual);
  return useMemo(() => {
    const cash = account?.cash ?? "0";
    const availableCash = subtractMoney(cash, account?.reserved_cash ?? "0");
    const positions = !account ? [] : Object.entries(account.positions)
      .filter(([, position]) => position.qty > 0)
      .map(([code, position]) => {
        const currentPrice = heldPrices[code];
        if (currentPrice === undefined) throw new Error(`持仓 ${code} 缺少行情，不能估值`);
        const valuation = valueHeldPosition(position, currentPrice);
        return {
          code,
          qty: position.qty,
          sellableQty: Math.max(0, position.qty - position.t1_locked - (account.reserved_sell_qty[code] ?? 0)),
          ...valuation,
        };
      });
    const totalMarketValue = positions.reduce((sum, position) => addMoney(sum, position.marketValue), "0");
    const totalPnl = positions.reduce((sum, position) => addMoney(sum, position.pnl), "0");
    return { positions, availableCash, totalMarketValue, totalAssets: addMoney(cash, totalMarketValue), totalPnl };
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
  const timeline = useTradingTimeline();
  const day = useSelector((state: RootState) => state.snapshot.snapshot?.day ?? 0);
  const tick = useSelector((state: RootState) => state.snapshot.snapshot?.tick ?? 0);
  return <span className="day-tag" title={timeline.displayNote ?? undefined}>第 {day + 1} 日 <time>{timeline.clock(tick)}</time>{timeline.customized && <small>（时间简化）</small>}</span>;
}

export function DesktopAssets() {
  const { totalAssets, availableCash, totalPnl } = usePortfolio();
  return <div className="assets">
    <div className="asset"><span className="label">总资产</span><span className="value">{formatCentsAmount(totalAssets)}</span><span className="unit">元</span></div>
    <div className="asset"><span className="label">可用资金</span><span className="value">{formatCentsAmount(availableCash)}</span><span className="unit">元</span></div>
    <div className="asset"><span className="label">总盈亏</span><span className={`value ${colorClass(compareMoney(totalPnl, "0"))}`}>{compareMoney(totalPnl, "0") >= 0 ? "+" : ""}{formatCentsAmount(totalPnl)}</span><span className="unit">元</span></div>
  </div>;
}

interface MarketPanelProps { browser: SecurityBrowser; onSelect: (code: string) => void; onOpen?: (code: string) => void }
export function ConnectedMarketPanel({ onSelect, onOpen, browser }: MarketPanelProps) {
  const markets = useSelector((state: RootState) => state.snapshot.snapshot?.markets ?? {});
  const account = useSelector((state: RootState) => state.snapshot.snapshot?.accounts[PLAYER_ACCOUNT_KEY] ?? null);
  const chartCode = useMarketRuntimeSelection();
  const { getPriceHistory } = useMarketRuntimeActions();
  const heldCodes = useMemo(() => new Set(Object.entries(account?.positions ?? {}).filter(([, position]) => position.qty > 0).map(([code]) => code)), [account]);
  return <MarketGrid browser={browser} markets={markets} selectedCode={chartCode} onSelect={onSelect} onOpen={onOpen} heldCodes={heldCodes} priceHistoryByCode={getPriceHistory()} />;
}

export function ConnectedTerminalStockList({ onSelect, browser }: MarketPanelProps) {
  const markets = useSelector((state: RootState) => state.snapshot.snapshot?.markets ?? {});
  const selectedCode = useMarketRuntimeSelection();
  const account = useSelector((state: RootState) => state.snapshot.snapshot?.accounts[PLAYER_ACCOUNT_KEY]);
  const heldCodes = new Set(Object.entries(account?.positions ?? {}).filter(([, position]) => position.qty > 0).map(([code]) => code));
  const codes = sortSecurityCodes(filterSecurityCodes({ codes: marketCodesForView(Object.keys(markets), STOCK_LIST.map(stock => stock.code), "watchlist", heldCodes), names: STOCK_NAMES, favorites: browser.favorites, heldCodes, query: browser.query, view: browser.view }), markets, browser.sortRules);
  return <nav aria-label="个股列表"><SecurityListControls browser={browser} codes={codes} onOpen={onSelect} />
    {codes.length === 0 && <p className="security-list-empty">{securityListEmptyMessage(browser.view, browser.query, browser.ready)}</p>}
    {codes.map(code => {
      const market = markets[code]!;
      const pct = priceChangePercent(market.last_price, market.last_close);
      return <button type="button" key={code} className="terminal-stock-row" data-security-code={code} onKeyDown={event => {
        const target = securityListKeyboardTarget(codes, code, event.key);
        if (target === null) return;
        event.preventDefault();
        onSelect(target);
        Array.from(event.currentTarget.parentElement!.querySelectorAll<HTMLButtonElement>(".terminal-stock-row")).find(button => button.dataset.securityCode === target)?.focus();
      }} aria-current={code === selectedCode ? "true" : undefined} onClick={() => onSelect(code)}><span><strong>{STOCK_NAMES[code] ?? code}</strong><small>{code}</small></span><span className={colorClass(pct)}><strong>{yuan(market.last_price)}</strong><small>{pct >= 0 ? "+" : ""}{pct.toFixed(2)}%</small></span></button>;
    })}
  </nav>;
}

interface ChartPanelProps {
  browser: SecurityBrowser;
  chartPeriod: MobileChartPeriod;
  setChartPeriod: (period: MobileChartPeriod) => void;
  klineDays: number;
  onTrade?: (side: "Buy" | "Sell") => void;
}
export function ConnectedChartPanel({ chartPeriod, setChartPeriod, klineDays, onTrade, browser }: ChartPanelProps) {
  const panelId = useId();
  const timeline = useTradingTimeline();
  const { getActiveDailyCandles } = useMarketRuntimeActions();
  const chartCode = useMarketRuntimeSelection();
  const { queryChartHistory } = useMarketRuntimeActions();
  useEffect(() => {
    if (chartPeriod === "日K") void queryChartHistory(chartCode);
  }, [chartCode, chartPeriod, klineDays, queryChartHistory]);
  const market = useSelector((state: RootState) => state.snapshot.snapshot?.markets[chartCode]);
  const trades = useSelector((state: RootState) => state.trades.items);
  const { chartData, auctionChartData, dailyChartData, indicatorCalculator } = useMarketRuntimeData();
  const day = useSelector((state: RootState) => state.snapshot.snapshot?.day ?? 0);
  const tick = useSelector((state: RootState) => state.snapshot.snapshot?.tick ?? 0);
  if (!market) return null;
  const diff = subtractMoney(market.last_price, market.last_close);
  const pct = priceChangePercent(market.last_price, market.last_close);
  const cls = colorClass(compareMoney(diff, "0"));
  return <>
    <div className="chart-toolbar"><ChartPeriodTabs period={chartPeriod} onChange={setChartPeriod} panelId={panelId} variant="terminal" /></div>
    <div className="stock-detail-header"><div className="detail-left"><div className="detail-name">{STOCK_NAMES[chartCode] ?? chartCode}</div><div className="detail-code">{chartCode}</div></div><div className="detail-prices"><span className={`detail-price ${cls}`}>{yuan(market.last_price)}</span><span className={`detail-change ${cls}`}>{compareMoney(diff, "0") >= 0 ? "+" : ""}{yuan(diff)} ({pct >= 0 ? "+" : ""}{pct.toFixed(2)}%)</span></div>{onTrade && <div className="terminal-quote-actions"><WatchlistToggle browser={browser} code={chartCode} /><button type="button" className="terminal-buy" aria-label="买入此股票" onClick={() => onTrade("Buy")}>买入</button><button type="button" className="terminal-sell" aria-label="卖出此股票" onClick={() => onTrade("Sell")}>卖出</button></div>}</div>
    <div className="market-chart-slot" id={panelId} role="tabpanel" aria-label={`${chartPeriod}图表`}><div hidden={chartPeriod === "分时"} className="shared-kline-host"><MarketKlinePanel code={chartCode} key={chartCode} dailyCandles={dailyChartData} period={chartPeriod} indicatorCalculator={indicatorCalculator} /></div><div hidden={chartPeriod !== "分时"} className="shared-intraday-host"><PriceChart dayRange={getActiveDailyCandles()[chartCode]} intraday={MobileIntradayProjection.fromInputs({ timeline, market, minutePoints: chartData, auctionPoints: auctionChartData, trades: [], elapsedMinutes: chartData.length, totalMinutes: TRADING_MINUTES_PER_DAY, gameDay: day, gameTick: tick })} data={chartData} dailyCandles={dailyChartData} lastClose={market.last_close} chartType="分时" klineDays={klineDays} indicatorCalculator={indicatorCalculator} /></div></div>
    <div className="order-book"><MarketQuotePanel code={chartCode} market={market} candle={getActiveDailyCandles()[chartCode]} trades={trades} /></div>
  </>;
}

export function PositionsPanel({ onOpenMarket }: { onOpenMarket: () => void }) {
  const { positions, availableCash, totalMarketValue, totalAssets, totalPnl } = usePortfolio();
  return <><section className="mobile-portfolio-summary" aria-label="账户资产概览"><div className="mobile-assets-total"><span>总资产</span><strong>{formatCentsAmount(totalAssets)}元</strong><small>可用资金 {formatCentsAmount(availableCash)}元</small></div><div><span>持仓市值</span><b>{formatCentsAmount(totalMarketValue)}元</b></div><div><span>浮动盈亏</span><b className={colorClass(compareMoney(totalPnl, "0"))}>{compareMoney(totalPnl, "0") >= 0 ? "+" : ""}{formatCentsAmount(totalPnl)}元</b></div></section><div className="mobile-position-list-head"><strong>我的持仓</strong><span>{positions.length} 只</span></div><div className="mobile-position-table-wrap"><table className="grid-table"><thead><tr><th>代码</th><th className="num">持仓</th><th className="num">可卖</th><th className="num">成本</th><th className="num">市值</th><th className="num">盈亏</th></tr></thead><tbody>
    {positions.length === 0 && <tr><td colSpan={6} className="empty"><div className="mobile-position-empty"><b>暂无持仓</b><span>从行情选择股票，通过“交易”买入后会显示在这里。</span><button type="button" onClick={onOpenMarket}>去看行情</button></div></td></tr>}
    {positions.map((position) => <tr key={position.code}><td className="mono">{position.code} {STOCK_NAMES[position.code]}</td><td className="num">{position.qty}</td><td className="num">{position.sellableQty}</td><td className="num">{yuan(position.avgCost)}</td><td className="num">{formatCentsAmount(position.marketValue)}元</td><td className={`num ${colorClass(compareMoney(position.pnl, "0"))}`}>{compareMoney(position.pnl, "0") >= 0 ? "+" : ""}{formatCentsAmount(position.pnl)}元</td></tr>)}
  </tbody></table></div></>;
}

export function TradesPanel() {
  const trades = useSelector((state: RootState) => state.trades.items);
  const lastCloses = useSelector(
    (state: RootState) => Object.fromEntries(Object.entries(state.snapshot.snapshot?.markets ?? {}).map(([code, market]) => [code, market.last_close])),
    (left, right) => Object.keys(left).length === Object.keys(right).length && Object.entries(left).every(([code, value]) => right[code] === value),
  );
  return <div className="trade-feed"><table className="grid-table"><thead><tr><th>序号</th><th>代码</th><th className="num">成交价</th><th className="num">成交量（手）</th></tr></thead><tbody>{trades.length === 0 && <tr><td colSpan={4} className="empty">等待成交…</td></tr>}{trades.map((trade) => { const diff = subtractMoney(trade.price, lastCloses[trade.code] ?? trade.price); return <tr key={trade.seq}><td className="mono">{trade.seq}</td><td className="mono">{trade.code}</td><td className={`num ${colorClass(compareMoney(diff, "0"))}`}>{yuan(trade.price)}</td><td className="num">{formatSharesAsLots(trade.qty)}</td></tr>; })}</tbody></table></div>;
}

interface UserPanelProps { running: boolean; pauseAfterClose: boolean; pauseBeforeOpen: boolean; pausePreferencesPending: boolean; deliveryMode: DeliveryMode | null; deliveryModes: readonly DeliveryMode[]; deliveryLabels: Record<DeliveryMode, string>; onPauseAfterCloseChange: (value: boolean) => void; onPauseBeforeOpenChange: (value: boolean) => void; onDeliveryModeChange: (mode: DeliveryMode) => void; onSave: () => void; onLoad: () => void; onSaveFile: () => void; onLoadFile: () => void }
export function UserPanel(props: UserPanelProps) {
  const savePolicyId = useId();
  const day = useSelector((state: RootState) => state.snapshot.snapshot?.day ?? 0);
  const { totalAssets, availableCash, totalPnl } = usePortfolio();
  return <><section className="mobile-user-overview" aria-label="我的账户"><span>模拟账户</span><strong>{formatCentsAmount(totalAssets)}元</strong><div><span>可用资金 <b>{formatCentsAmount(availableCash)}元</b></span><span>持仓盈亏 <b className={colorClass(compareMoney(totalPnl, "0"))}>{compareMoney(totalPnl, "0") >= 0 ? "+" : ""}{formatCentsAmount(totalPnl)}元</b></span></div></section><section className="mobile-game-state" aria-label="游戏状态"><div><span>当前进度</span><b>第 {day + 1} 个交易日</b></div><div><span>模拟状态</span><b>{props.running ? "交易中" : "已暂停"}</b></div><label><input type="checkbox" checked={props.pauseAfterClose} disabled={props.pausePreferencesPending} onChange={(event) => props.onPauseAfterCloseChange(event.currentTarget.checked)} />收盘后暂停复盘</label><label><input type="checkbox" checked={props.pauseBeforeOpen} disabled={props.pausePreferencesPending} onChange={(event) => props.onPauseBeforeOpenChange(event.currentTarget.checked)} />开盘前暂停查看资讯</label>{props.deliveryMode !== null && props.deliveryModes.length > 0 && <div><label htmlFor="mobile-delivery-mode">刷新方式</label><select id="mobile-delivery-mode" value={props.deliveryMode} onChange={(event) => props.onDeliveryModeChange(event.target.value as DeliveryMode)}>{props.deliveryModes.map((mode) => <option key={mode} value={mode}>{props.deliveryLabels[mode]}</option>)}</select></div>}</section><div className="mobile-user-section"><h4>数据管理</h4><p id={savePolicyId} className="save-policy-description">完整自然日结束后自动存档，日内不保存；设置的文件仅在后续日终更新。首次日终前没有可读档案。</p><button type="button" aria-describedby={savePolicyId} onClick={props.onSave}>日终存档说明</button><button type="button" onClick={props.onLoad}>读取本地进度</button><button type="button" aria-describedby={savePolicyId} onClick={props.onSaveFile}>设置日终存档文件</button><button type="button" onClick={props.onLoadFile}>从文件读取</button></div><details className="chart-attribution"><summary>关于图表</summary><p>TradingView Lightweight Charts™<br />Copyright (с) 2025 TradingView, Inc. <a href="https://www.tradingview.com/" target="_blank" rel="noreferrer">TradingView</a></p></details></>;
}

interface CompanyPanelActions { initialCivilDate: string; onCompanyQuery: (companyId: string, cursor: string | null) => void; onAdvanceCivilDay: () => Promise<void> }
interface MobileDetailProps extends CompanyPanelActions { browser: SecurityBrowser; klineDays: number; setKlineDays: Dispatch<SetStateAction<number>>; period: MobileChartPeriod; infoTab: MobileInfoTab; speed: number; measuredSpeed: string; measuredSpeedTitle: string; running: boolean; onPeriodChange: (period: MobileChartPeriod) => void; onInfoTabChange: (tab: MobileInfoTab) => void; onSpeedChange: (speed: number) => void; onPauseToggle: () => void; onBack: () => void; onSelect: (code: string) => void }

export function ConnectedCompanyPanel(props: CompanyPanelActions & { stockContext?: boolean }) {
  const chartCode = useMarketRuntimeSelection();
  const companyState = useSelector((state: RootState) => state.company);
  const companyId = props.stockContext ? publicCompanyForStock(chartCode)?.id ?? null : companyState.selectedCompanyId ?? publicCompanyForStock(chartCode)?.id ?? null;
  const onCompanyChange = (nextCompanyId: string) => {
    store.dispatch(selectCompany(nextCompanyId));
  };
  const onReadingChange = (changes: Partial<CompanyReading>) => {
    if (companyId !== null) store.dispatch(updateCompanyReading({ generation: companyState.generation, companyId, changes }));
  };
  return <CompanyPanel allowCompanySelection={!props.stockContext} companyId={companyId} companyState={companyState} initialCivilDate={props.initialCivilDate} onCompanyChange={onCompanyChange} onReadingChange={onReadingChange} onQuery={props.onCompanyQuery} onAdvanceCivilDay={props.onAdvanceCivilDay} />;
}

export function ConnectedMobileDetail(props: MobileDetailProps) {
  const chartCode = useMarketRuntimeSelection();
  const { queryChartHistory } = useMarketRuntimeActions();
  useEffect(() => {
    if (props.period !== "分时") void queryChartHistory(chartCode);
  }, [chartCode, props.klineDays, props.period, queryChartHistory]);
  const { chartData, auctionChartData, dailyChartData, indicatorCalculator } = useMarketRuntimeData();
  const { getActiveDailyCandles } = useMarketRuntimeActions();
  const markets = useSelector((state: RootState) => state.snapshot.snapshot?.markets ?? {});
  const market = markets[chartCode];
  const marketCodes = Object.keys(markets);
  const account = useSelector((state: RootState) => state.snapshot.snapshot?.accounts[PLAYER_ACCOUNT_KEY]);
  const allTrades = useSelector((state: RootState) => state.trades.items);
  const trades = useMemo(() => allTrades.filter((trade) => trade.code === chartCode), [allTrades, chartCode]);
  const day = useSelector((state: RootState) => state.snapshot.snapshot?.day ?? 0);
  const tick = useSelector((state: RootState) => state.snapshot.snapshot?.tick ?? 0);
  if (!market) return null;
  const heldCodes = new Set(Object.entries(account?.positions ?? {}).filter(([, position]) => position.qty > 0).map(([code]) => code));
  const orderedCodes = sortSecurityCodes(filterSecurityCodes({ codes: marketCodesForView(marketCodes, STOCK_LIST.map(stock => stock.code), "watchlist", heldCodes), names: STOCK_NAMES, favorites: props.browser.favorites, heldCodes, query: props.browser.query, view: props.browser.view }), markets, mobileSecuritySort(props.browser.sortRules));
  const previous = adjacentSecurityCode(orderedCodes, chartCode, -1);
  const next = adjacentSecurityCode(orderedCodes, chartCode, 1);
  const latestMinute = chartData.at(-1)?.time;
  const elapsedMinutes = latestMinute === undefined ? 0 : Math.min(TRADING_MINUTES_PER_DAY, Math.floor(latestMinute) + 1);
  return <MobileStockDetail code={chartCode} name={STOCK_NAMES[chartCode] ?? chartCode} market={market} minutePoints={chartData} auctionPoints={auctionChartData} dailyCandles={dailyChartData} activeDailyCandle={getActiveDailyCandles()[chartCode]} indicatorCalculator={indicatorCalculator} trades={trades} elapsedMinutes={elapsedMinutes} totalMinutes={TRADING_MINUTES_PER_DAY} klineDays={props.klineDays} period={props.period} infoTab={props.infoTab} speed={props.speed} measuredSpeed={props.measuredSpeed} measuredSpeedTitle={props.measuredSpeedTitle} running={props.running} gameDay={day} gameTick={tick} onKlineDaysChange={props.setKlineDays} onPeriodChange={props.onPeriodChange} onInfoTabChange={props.onInfoTabChange} onSpeedChange={props.onSpeedChange} onPauseToggle={props.onPauseToggle} onBack={props.onBack} canSwitchStock={previous !== null && next !== null} onPrevious={() => { if (previous !== null) props.onSelect(previous); }} onNext={() => { if (next !== null) props.onSelect(next); }} watchlistControl={<WatchlistToggle browser={props.browser} code={chartCode} />} companyContent={<ConnectedCompanyPanel initialCivilDate={props.initialCivilDate} onCompanyQuery={props.onCompanyQuery} onAdvanceCivilDay={props.onAdvanceCivilDay} />} />;
}
