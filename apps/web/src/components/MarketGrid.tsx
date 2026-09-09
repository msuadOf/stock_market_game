/**
 * AG Grid 行情表（替代简单 HTML 表格）。
 * 密集金融表格：代码/名称/现价/涨跌额/涨跌幅，红涨绿跌单元格渲染。
 * 点击行 → 选股（回调）。
 */
import { AgGridReact } from "ag-grid-react";
import type { ColDef, CellClassParams, GridApi, GridReadyEvent, IRowNode } from "ag-grid-community";
import { ClientSideRowModelModule, enableDevValidations, ModuleRegistry } from "ag-grid-community";
import { useMemo, useCallback, useEffect, useRef, useState } from "react";
import type { Cents, MarketSnap } from "../types/engine";
import type { PricePoint } from "./PriceChart";
import { STOCK_LIST } from "../config/defaults";
import { MOBILE_LAYOUT } from "../mobile/mobile-layout-spec";
import { marketCodesForView, sparklineGeometry, type MobileMarketView } from "../mobile/market-model";
import { buildMarketRows, diffMarketRows, type MarketGridRow as RowData } from "./market-grid-rows.ts";

ModuleRegistry.registerModules([ClientSideRowModelModule]);

if (import.meta.env.DEV) {
  enableDevValidations();
}

interface Props {
  markets: Readonly<Record<string, MarketSnap>>;
  selectedCode: string | null;
  onSelect: (code: string) => void;
  heldCodes: ReadonlySet<string>;
  priceHistoryByCode: Readonly<Record<string, PricePoint[]>>;
}

function yuan(cents: Cents): number {
  return cents / 100;
}

export function MarketGrid({ markets, selectedCode, onSelect, heldCodes, priceHistoryByCode }: Props) {
  const [mobileTab, setMobileTab] = useState<MobileMarketView>("watchlist");
  const [sortDescending, setSortDescending] = useState(false);
  const builtRowsRef = useRef<RowData[]>([]);
  const allRowData = useMemo<RowData[]>(() => {
    const codes = marketCodesForView(Object.keys(markets), STOCK_LIST.map((stock) => stock.code), "watchlist", heldCodes);
    const rows = buildMarketRows(markets, codes, builtRowsRef.current);
    builtRowsRef.current = rows;
    return rows;
  }, [heldCodes, markets]);
  const latestRowsRef = useRef(allRowData);
  latestRowsRef.current = allRowData;
  const initialRowsRef = useRef(allRowData);
  const appliedRowsRef = useRef(initialRowsRef.current);
  const gridApiRef = useRef<GridApi<RowData> | null>(null);

  const applyLatestRows = useCallback((api: GridApi<RowData>, rows: RowData[]) => {
    const transaction = diffMarketRows(appliedRowsRef.current, rows);
    if (transaction.add.length > 0 || transaction.update.length > 0 || transaction.remove.length > 0) {
      api.applyTransactionAsync(transaction);
    }
    appliedRowsRef.current = rows;
  }, []);

  useEffect(() => {
    if (gridApiRef.current) applyLatestRows(gridApiRef.current, allRowData);
  }, [allRowData, applyLatestRows]);

  const onGridReady = useCallback((event: GridReadyEvent<RowData>) => {
    gridApiRef.current = event.api;
    applyLatestRows(event.api, latestRowsRef.current);
  }, [applyLatestRows]);
  const getRowId = useCallback((params: { data: RowData }) => params.data.code, []);

  const mobileRowData = useMemo(() => {
    const rows = mobileTab === "holdings" ? allRowData.filter((row) => heldCodes.has(row.code)) : allRowData;
    return sortDescending ? [...rows].sort((a, b) => b.changePct - a.changePct) : rows;
  }, [allRowData, heldCodes, mobileTab, sortDescending]);

  const marketIndex = useMemo(() => {
    if (allRowData.length === 0) return { value: 0, change: 0 };
    return {
      value: allRowData.reduce((sum, stock) => sum + stock.lastPrice, 0) / allRowData.length * 100,
      change: allRowData.reduce((sum, stock) => sum + stock.changePct, 0) / allRowData.length,
    };
  }, [allRowData]);

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
        width: 80,
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
        valueFormatter: (p) => (p.value as number).toFixed(2),
        cellClass: (p: CellClassParams<RowData>) =>
          p.data ? colorClass(p.data._rawLastPrice - p.data._rawLastClose) : "",
      },
      {
        headerName: "涨跌额",
        field: "changeAbs",
        width: 80,
        type: "numericColumn",
        valueFormatter: (p) => {
          const v = p.value as number;
          return (v >= 0 ? "+" : "") + v.toFixed(2);
        },
        cellClass: (p: CellClassParams<RowData>) =>
          p.data ? colorClass(p.data.changeAbs) : "",
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
    ],
    [colorClass],
  );

  const defaultColDef = useMemo<ColDef>(
    () => ({
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

  const getRowClass = useCallback(
    (params: { data: RowData | undefined }) => {
      if (params.data && params.data.code === selectedCode) return "row-selected";
      return "";
    },
    [selectedCode],
  );

  return (
    <>
      <div className="ag-theme-alpine market-grid-container" style={{ width: "100%", height: "100%", minHeight: 180 }}>
        <AgGridReact<RowData>
          theme="legacy"
          rowData={initialRowsRef.current}
          getRowId={getRowId}
          onGridReady={onGridReady}
          columnDefs={columnDefs}
          defaultColDef={defaultColDef}
          onRowClicked={onRowClicked}
          getRowClass={getRowClass}
          rowHeight={28}
          headerHeight={28}
          suppressCellFocus={true}
        />
      </div>
      <div className="mobile-market-dashboard">
        <section className="mobile-index-strip" aria-label="市场指数与快捷入口">
          <div className={`mobile-index-quote ${marketIndex.change > 0 ? "up" : marketIndex.change < 0 ? "down" : "flat"}`}><strong>{marketIndex.value.toFixed(2)} <small>{marketIndex.change >= 0 ? "+" : ""}{marketIndex.change.toFixed(2)}</small></strong><span>模拟指数　<b>{marketIndex.change >= 0 ? "+" : ""}{marketIndex.change.toFixed(2)}%</b>⌄</span></div>
          {[["⌁", "资金"], ["▤", "资讯"], ["▣", "资产"], ["⌁", "分析"]].map(([icon, label]) => <button type="button" key={label} title={`${label}尚未开放`} disabled><i>{icon}</i><span>{label}</span></button>)}
        </section>
        <nav className="mobile-market-tabs" aria-label="行情分类">
          {MOBILE_LAYOUT.watchlistTabs.map((label, index) => {
            const tab: MobileMarketView = index === 0 ? "watchlist" : "holdings";
            return (
            <button key={tab} type="button" className={mobileTab === tab ? "active" : ""} onClick={() => setMobileTab(tab)}>{label}</button>
          );})}
          <span className="mobile-market-tabs-spacer" aria-hidden="true" /><button type="button" aria-label="更多分类（尚未开放）" title="更多分类尚未开放" disabled>☰</button>
        </nav>
        <div className="mobile-market-toolbar" aria-label="行情列表工具栏">
          <span>✎　　☷</span><b>▦ 多股同列</b>
          <button type="button" aria-pressed={sortDescending} onClick={() => setSortDescending((value) => !value)}>涨幅　{sortDescending ? "↓" : "↕"}</button>
        </div>
      <div className="mobile-market-list" aria-label="股票行情列表">
        {mobileTab === "holdings" && mobileRowData.length === 0 && <p className="mobile-market-empty">暂无持仓</p>}
        {mobileRowData.map((stock) => {
          const trend = stock.changePct > 0 ? "up" : stock.changePct < 0 ? "down" : "flat";
          const geometry = sparklineGeometry(priceHistoryByCode[stock.code] ?? [], yuan(stock._rawLastClose), 64, 48);
          const gradientId = `mobile-market-fill-${stock.code.replace(/[^a-zA-Z0-9_-]/g, "-")}`;
          return (
            <button
              key={stock.code}
              type="button"
              className={`mobile-market-row ${selectedCode === stock.code ? "selected" : ""}`}
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
                <small>{stock.lastPrice < 10 ? stock.lastPrice.toFixed(3) : stock.lastPrice.toFixed(2)}</small>
              </span>
            </button>
          );
        })}
      </div>
      </div>
    </>
  );
}
