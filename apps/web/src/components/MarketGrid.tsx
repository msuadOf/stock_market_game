import { compareMoney, centsToYuanText, moneyToChartNumber } from "../utils/money.ts";
/**
 * AG Grid 行情表（替代简单 HTML 表格）。
 * 密集金融表格：代码/名称/现价/涨跌额/涨跌幅，红涨绿跌单元格渲染。
 * 点击行 → 选股（回调）。
 */
import { AgGridReact } from "ag-grid-react";
import type { GridApi, ColDef, CellClassParams, GridReadyEvent, IRowNode, CellKeyDownEvent, FullWidthCellKeyDownEvent } from "ag-grid-community";
import { RowApiModule, CellStyleModule, ClientSideRowModelApiModule, ClientSideRowModelModule, enableDevValidations, LocaleModule, ModuleRegistry, RowStyleModule } from "ag-grid-community";
import { useMemo, useCallback, useEffect, useRef, useState } from "react";
import type { Cents, MarketSnap } from "../types/engine";
import type { PricePoint } from "./PriceChart";
import { STOCK_LIST } from "../config/defaults";
import { marketCodesForView, sparklineGeometry } from "../mobile/market-model";
import { MarketGridRowSynchronizer } from "./market-grid-row-synchronizer.ts";
import { buildMarketRows, type MarketGridRow as RowData } from "./market-grid-rows.ts";
import { SecurityListControls } from "./SecurityListControls.tsx";
import type { SecurityBrowser } from "../app/useSecurityBrowser.ts";
import { filterSecurityCodes, securityListEmptyMessage } from "../app/security-browser-model.ts";
import { STOCK_NAMES } from "../config/defaults";
import { MARKET_GRID_LOCALE, selectMarketByKeyboard } from "./market-grid-accessibility.ts";

ModuleRegistry.registerModules([RowApiModule, ClientSideRowModelModule, ClientSideRowModelApiModule, RowStyleModule, CellStyleModule, LocaleModule]);

if (import.meta.env.DEV) {
  enableDevValidations();
}

interface Props {
  browser: SecurityBrowser;
  markets: Readonly<Record<string, MarketSnap>>;
  selectedCode: string | null;
  onSelect: (code: string) => void;
  onOpen?: (code: string) => void;
  heldCodes: ReadonlySet<string>;
  priceHistoryByCode: Readonly<Record<string, readonly PricePoint[]>>;
}

function yuan(cents: Cents): number {
  return moneyToChartNumber(cents) / 100;
}

export function MarketGrid({ markets, selectedCode, onSelect, onOpen, heldCodes, priceHistoryByCode, browser }: Props) {
  const [sortDescending, setSortDescending] = useState(false);
  const builtRowsRef = useRef<RowData[]>([]);
  const gridApiRef = useRef<GridApi<RowData> | null>(null);
  const gridContainerRef = useRef<HTMLDivElement | null>(null);
  const allMarketRowData = useMemo<RowData[]>(() => {
    const codes = marketCodesForView(Object.keys(markets), STOCK_LIST.map((stock) => stock.code), "watchlist", heldCodes);
    const rows = buildMarketRows(markets, codes, builtRowsRef.current);
    builtRowsRef.current = rows;
    return rows;
  }, [heldCodes, markets]);
  const visibleCodes = useMemo(() => filterSecurityCodes({ codes: allMarketRowData.map(row => row.code), names: STOCK_NAMES, favorites: browser.favorites, heldCodes, query: browser.query, view: browser.view }), [allMarketRowData, browser.favorites, browser.query, browser.view, heldCodes]);
  const allRowData = useMemo(() => { const available = new Set(visibleCodes); return allMarketRowData.filter(row => available.has(row.code)); }, [allMarketRowData, visibleCodes]);
  const emptyMessage = securityListEmptyMessage(browser.view, browser.query, browser.ready);
  const initialRowsRef = useRef(allRowData);
  const [rowSynchronizer] = useState(() => new MarketGridRowSynchronizer(initialRowsRef.current));
  rowSynchronizer.recordLatest(allRowData);

  // ready 可能在本组件的 effect 前到达；先登记本次渲染的最新目标。
  useEffect(() => {
    rowSynchronizer.updateLatest(allRowData);
  }, [allRowData, rowSynchronizer]);

  const onGridReady = useCallback((event: GridReadyEvent<RowData>) => {
    gridApiRef.current = event.api;
    rowSynchronizer.attach(event.api, initialRowsRef.current);
  }, [rowSynchronizer]);
  const onGridPreDestroyed = useCallback(() => { gridApiRef.current = null; rowSynchronizer.dispose(); }, [rowSynchronizer]);
  const getRowId = useCallback((params: { data: RowData }) => params.data.code, []);

  const mobileRowData = useMemo(() => sortDescending ? [...allRowData].sort((a, b) => b.changePct - a.changePct) : allRowData, [allRowData, sortDescending]);

  const marketIndex = useMemo(() => {
    if (allMarketRowData.length === 0) return { value: 0, change: 0 };
    return {
      value: allMarketRowData.reduce((sum, stock) => sum + yuan(stock.lastPrice), 0) / allMarketRowData.length * 100,
      change: allMarketRowData.reduce((sum, stock) => sum + stock.changePct, 0) / allMarketRowData.length,
    };
  }, [allMarketRowData]);

  const colorClass = useCallback((diff: number) => {
    if (diff > 0) return "cell-up";
    if (diff < 0) return "cell-down";
    return "cell-flat";
  }, []);

  const columnDefs = useMemo<ColDef<RowData>[]>(
    () => [
      {
        headerName: "代码",
        field: "code",
        width: 110,
        minWidth: 110,
        cellClass: "grid-mono",
        pinned: "left",
      },
      {
        headerName: "名称",
        field: "name",
        width: 90,
      },
      {
        headerName: "现价",
        field: "lastPrice",
        width: 80,
        type: "numericColumn",
        valueFormatter: (p) => centsToYuanText(p.value as Cents),
        comparator: compareMoney,
        cellClass: (p: CellClassParams<RowData>) =>
          p.data ? colorClass(compareMoney(p.data._rawLastPrice, p.data._rawLastClose)) : "",
      },
      {
        headerName: "涨跌额",
        field: "changeAbs",
        comparator: compareMoney,
        width: 80,
        type: "numericColumn",
        valueFormatter: (p) => {
          const value = p.value as Cents;
          return (compareMoney(value, "0") >= 0 ? "+" : "") + centsToYuanText(value);
        },
        cellClass: (p: CellClassParams<RowData>) =>
          p.data ? colorClass(compareMoney(p.data.changeAbs, "0")) : "",
      },
      {
        headerName: "涨跌幅",
        field: "changePct",
        width: 80,
        type: "numericColumn",
        valueFormatter: (p) => {
          const v = p.value as number;
          return (v >= 0 ? "+" : "") + v.toFixed(2) + "%";
        },
        cellClass: (p: CellClassParams<RowData>) =>
          p.data ? colorClass(p.data.changePct) : "",
      },
      {
        headerName: "昨收", colId: "lastClose", minWidth: 85, type: "numericColumn",
        valueGetter: (p) => p.data?._rawLastClose,
        valueFormatter: (p) => p.value === undefined ? "—" : centsToYuanText(p.value as Cents), comparator: compareMoney,
      },
      ...([ ["best_bid", "买一"], ["best_ask", "卖一"] ] as const).map(([field, headerName]): ColDef<RowData> => ({
        headerName, colId: field, minWidth: 85, type: "numericColumn",
        valueGetter: (p) => p.data?._source[field],
        valueFormatter: (p) => p.value == null ? "—" : centsToYuanText(p.value as Cents),
        comparator: (a: Cents | null, b: Cents | null) => a === null ? (b === null ? 0 : -1) : b === null ? 1 : compareMoney(a, b),
      })),
    ],
    [colorClass],
  );

  const defaultColDef = useMemo<ColDef>(
    () => ({
      flex: 1,
      minWidth: 80,
      resizable: true,
      sortable: true,
    }),
    [],
  );

  const onRowClicked = useCallback(
    (e: { data?: RowData; node?: IRowNode<RowData> }) => {
      if (e.data) onSelect(e.data.code);
    },
    [onSelect],
  );

  const onCellKeyDown = useCallback((event: CellKeyDownEvent<RowData> | FullWidthCellKeyDownEvent<RowData>) => {
    const keyboardEvent = event.event;
    if (keyboardEvent instanceof KeyboardEvent && keyboardEvent.key === "Enter" && event.data && onOpen) {
      keyboardEvent.preventDefault();
      onOpen(event.data.code);
      return;
    }
    if (keyboardEvent instanceof KeyboardEvent && selectMarketByKeyboard(keyboardEvent.key, event.data?.code, onSelect)) keyboardEvent.preventDefault();
  }, [onSelect, onOpen]);

  const getRowClass = useCallback(
    (params: { data: RowData | undefined }) => {
      if (params.data && params.data.code === selectedCode) return "row-selected";
      return "";
    },
    [selectedCode],
  );

  return (
    <>
      <SecurityListControls browser={browser} codes={visibleCodes} onOpen={code => {
        if (gridContainerRef.current?.getClientRects().length === 0) {
          const first = mobileRowData[0];
          if (first) (onOpen ?? onSelect)(first.code);
          return;
        }
        const api = gridApiRef.current;
        if (api === null) { (onOpen ?? onSelect)(code); return; }
        api.flushAsyncTransactions();
        const first = api.getDisplayedRowAtIndex(0);
        if (first?.data) (onOpen ?? onSelect)(first.data.code);
      }} />
      <div ref={gridContainerRef} className="ag-theme-alpine market-grid-container" role="region" aria-label="股票行情" aria-describedby="market-grid-keyboard-help" style={{ width: "100%", height: "100%", minHeight: 180 }}>
        <p className="sr-only" id="market-grid-keyboard-help">用方向键浏览行情，空格预览当前股票，Enter 进入个股。</p>
        <AgGridReact<RowData>
          theme="legacy"
          overlayNoRowsTemplate={`<span class="security-list-empty">${emptyMessage}</span>`}
          rowData={initialRowsRef.current}
          getRowId={getRowId}
          onGridReady={onGridReady}
          onGridPreDestroyed={onGridPreDestroyed}
          columnDefs={columnDefs}
          defaultColDef={defaultColDef}
          onRowClicked={onRowClicked}
          onRowDoubleClicked={(event) => { if (event.data && onOpen) onOpen(event.data.code); }}
          onCellKeyDown={onCellKeyDown}
          localeText={MARKET_GRID_LOCALE}
          getRowClass={getRowClass}
          rowHeight={28}
          headerHeight={28}
          suppressCellFocus={false}
        />
      </div>
      <div className="mobile-market-dashboard">
        <section className="mobile-index-strip" aria-label="市场指数与快捷入口">
          <div className={`mobile-index-quote ${marketIndex.change > 0 ? "up" : marketIndex.change < 0 ? "down" : "flat"}`}><strong>{marketIndex.value.toFixed(2)} <small>{marketIndex.change >= 0 ? "+" : ""}{marketIndex.change.toFixed(2)}</small></strong><span>模拟指数　<b>{marketIndex.change >= 0 ? "+" : ""}{marketIndex.change.toFixed(2)}%</b>⌄</span></div>
          {[["⌁", "资金"], ["▤", "资讯"], ["▣", "资产"], ["⌁", "分析"]].map(([icon, label]) => <button type="button" key={label} title={`${label}尚未开放`} disabled><i>{icon}</i><span>{label}</span></button>)}
        </section>
        <div className="mobile-market-toolbar" aria-label="行情列表工具栏">
          <span>✎　　☷</span><b>▦ 多股同列</b>
          <button type="button" aria-pressed={sortDescending} onClick={() => setSortDescending((value) => !value)}>涨幅　{sortDescending ? "↓" : "↕"}</button>
        </div>
      <div className="mobile-market-list" aria-label="股票行情列表">
        {mobileRowData.length === 0 && <p className="security-list-empty">{emptyMessage}</p>}
        {mobileRowData.map((stock) => {
          const trend = stock.changePct > 0 ? "up" : stock.changePct < 0 ? "down" : "flat";
          const geometry = sparklineGeometry(priceHistoryByCode[stock.code] ?? [], yuan(stock._rawLastClose), 64, 48);
          const gradientId = `mobile-market-fill-${stock.code.replace(/[^a-zA-Z0-9_-]/g, "-")}`;
          return (
            <button
              key={stock.code}
              type="button"
              className={`mobile-market-row ${selectedCode === stock.code ? "selected" : ""}`}
              aria-current={selectedCode === stock.code ? "true" : undefined}
              onClick={() => onSelect(stock.code)}
            >
              <span className="mobile-market-name">
                <strong>{stock.name}</strong>
                <small>{stock.code}</small>
              </span>
              <svg className={`mobile-market-trend ${trend}`} viewBox="0 0 64 48" preserveAspectRatio="none" aria-hidden="true">
                <defs>
                  <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" stopColor="currentColor" stopOpacity={trend === "down" ? .02 : .22} />
                    <stop offset="100%" stopColor="currentColor" stopOpacity={trend === "down" ? .22 : .02} />
                  </linearGradient>
                </defs>
                {geometry.areaPoints && <polygon className="mobile-market-area" points={geometry.areaPoints} fill={`url(#${gradientId})`} />}
                <line className="mobile-market-axis" x1="0" y1={geometry.axisY} x2="64" y2={geometry.axisY} />
                {geometry.linePoints && <polyline points={geometry.linePoints} fill="none" vectorEffect="non-scaling-stroke" />}
              </svg>
              <span className={`mobile-market-price ${trend}`}>
                <strong>{stock.changePct >= 0 ? "+" : ""}{stock.changePct.toFixed(2)}%</strong>
                <small>{centsToYuanText(stock.lastPrice)}</small>
              </span>
            </button>
          );
        })}
      </div>
      </div>
    </>
  );
}
