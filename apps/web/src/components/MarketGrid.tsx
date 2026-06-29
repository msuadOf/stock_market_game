/**
 * AG Grid 行情表（替代简单 HTML 表格）。
 * 密集金融表格：代码/名称/现价/涨跌额/涨跌幅，红涨绿跌单元格渲染。
 * 点击行 → 选股（回调）。
 */
import { AgGridReact } from "ag-grid-react";
import type { ColDef, CellClassParams, IRowNode } from "ag-grid-community";
import { ModuleRegistry, AllCommunityModule } from "ag-grid-community";
import { useMemo, useCallback } from "react";
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
  );
}
