/**
 * AG Grid 行情表（替代简单 HTML 表格）。
 * 密集金融表格：代码/名称/现价/涨跌额/涨跌幅，红涨绿跌单元格渲染。
 * 点击行 → 选股（回调）。
 */
import { AgGridReact } from "ag-grid-react";
import type { ColDef, CellClassParams, IRowNode } from "ag-grid-community";
import { ModuleRegistry, AllCommunityModule } from "ag-grid-community";
import { useMemo, useCallback, useState } from "react";
import type { Snapshot, Cents } from "../types/engine";
import { STOCK_NAMES } from "../config/defaults";

ModuleRegistry.registerModules([AllCommunityModule]);

interface RowData {
  code: string;
  name: string;
  lastPrice: number; // 元
  changeAbs: number; // 元
  changePct: number; // %
  _rawLastPrice: Cents;
  _rawLastClose: Cents;
}

interface Props {
  snapshot: Snapshot;
  selectedCode: string | null;
  onSelect: (code: string) => void;
}

function yuan(cents: Cents): number {
  return cents / 100;
}

export function MarketGrid({ snapshot, selectedCode, onSelect }: Props) {
  const [mobileTab, setMobileTab] = useState<"overview" | "ranking" | "dragon">("overview");
  const rowData = useMemo<RowData[]>(() => {
    return Object.entries(snapshot.markets).map(([code, m]) => {
      const diff = m.last_price - m.last_close;
      return {
        code,
        name: STOCK_NAMES[code] ?? code,
        lastPrice: yuan(m.last_price),
        changeAbs: yuan(diff),
        changePct: m.last_close !== 0 ? (diff / m.last_close) * 100 : 0,
        _rawLastPrice: m.last_price,
        _rawLastClose: m.last_close,
      };
    });
  }, [snapshot]);

  const marketSummary = useMemo(() => {
    const changes = rowData.map((stock) => stock.changePct);
    const up = changes.filter((value) => value > 0).length;
    const down = changes.filter((value) => value < 0).length;
    const flat = changes.length - up - down;
    const indexFor = (codes: string[]) => {
      const source = rowData.filter((stock) => codes.includes(stock.code));
      const values = source.length > 0 ? source : rowData;
      const averagePrice = values.reduce((total, stock) => total + stock.lastPrice, 0) / Math.max(values.length, 1);
      const averageChange = values.reduce((total, stock) => total + stock.changePct, 0) / Math.max(values.length, 1);
      return { value: averagePrice * 100, change: averageChange };
    };
    return {
      up,
      down,
      flat,
      indices: [
        { name: "综合指数", ...indexFor(rowData.map((stock) => stock.code)) },
        { name: "创业板", ...indexFor(["300260"]) },
        { name: "中小板", ...indexFor(["002156", "000812"]) },
      ],
    };
  }, [rowData]);

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
          rowData={rowData}
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
        <section className="mobile-index-strip" aria-label="市场指数">
          {marketSummary.indices.map((index) => {
            const tone = index.change > 0 ? "up" : index.change < 0 ? "down" : "flat";
            return (
              <div key={index.name} className={`mobile-index-card ${tone}`}>
                <span>{index.name}</span>
                <strong>{index.value.toFixed(2)}</strong>
                <small>{index.change >= 0 ? "+" : ""}{index.change.toFixed(2)}%</small>
              </div>
            );
          })}
        </section>
        <section className="mobile-market-breadth" aria-label="市场涨跌统计">
          <div className="breadth-labels"><span className="down">下跌 {marketSummary.down}</span><span className="flat">平 {marketSummary.flat}</span><span className="up">上涨 {marketSummary.up}</span></div>
          <div className="breadth-bar"><i className="down" style={{ flex: marketSummary.down }} /><i className="flat" style={{ flex: marketSummary.flat }} /><i className="up" style={{ flex: marketSummary.up }} /></div>
        </section>
        <nav className="mobile-market-tabs" aria-label="行情分类">
          {([ ["overview", "市场概览"], ["ranking", "个股排行"], ["dragon", "龙虎榜"]] as const).map(([tab, label]) => (
            <button key={tab} type="button" className={mobileTab === tab ? "active" : ""} onClick={() => setMobileTab(tab)}>{label}</button>
          ))}
        </nav>
        {mobileTab === "dragon" ? (
          <p className="mobile-market-empty">当前交易日暂无龙虎榜数据</p>
        ) : (
      <div className="mobile-market-list" aria-label="股票行情列表">
        {(mobileTab === "ranking" ? [...rowData].sort((a, b) => b.changePct - a.changePct) : rowData).map((stock) => {
          const trend = stock.changePct > 0 ? "up" : stock.changePct < 0 ? "down" : "flat";
          const endY = stock.changePct > 0 ? 15 : stock.changePct < 0 ? 33 : 24;
          const points = `0,26 16,${25 - stock.changePct} 32,${28 + stock.changePct} 48,${21 - stock.changePct} 64,${endY}`;
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
              <svg className={`mobile-market-trend ${trend}`} viewBox="0 0 64 48" aria-hidden="true">
                <polyline points={points} fill="none" vectorEffect="non-scaling-stroke" />
              </svg>
              <span className={`mobile-market-price ${trend}`}>
                <strong>{stock.lastPrice.toFixed(2)}</strong>
                <small>{stock.changePct >= 0 ? "+" : ""}{stock.changePct.toFixed(2)}%</small>
              </span>
            </button>
          );
        })}
      </div>
        )}
      </div>
    </>
  );
}
