import { centsToYuanText } from "../utils/money.ts";

export interface TradeConfirmation {
  readonly receipt_id: string;
  readonly civil_date: string;
  readonly code: string;
  readonly side: "Buy" | "Sell";
  readonly price: string;
  readonly quantity_shares: number;
  readonly gross: string;
  readonly actual_fees: {
    readonly commission: string;
    readonly stamp_tax: string;
    readonly transfer_fee: string;
  };
}

export interface TradeConfirmationTableProps {
  readonly rows: readonly TradeConfirmation[];
  readonly loading: boolean;
  readonly error: string | null;
  readonly hasQueried?: boolean;
  readonly onRefresh: () => void;
  readonly onOlder?: () => void;
}

function yuan(cents: string): string {
  return `${centsToYuanText(cents)}元`;
}

export function TradeConfirmationTable({ rows, loading, error, hasQueried = true, onRefresh, onOlder }: TradeConfirmationTableProps) {
  if (error !== null) {
    return <section aria-label="本人交割单"><p role="alert">查询本人交割单失败：{error}</p><button type="button" onClick={onRefresh}>重试</button></section>;
  }
  if (loading) return <section aria-label="本人交割单" aria-busy="true">正在查询本人交割单…</section>;
  if (!hasQueried) return <section aria-label="本人交割单"><p>交割单按需查询，不会使用公开成交事件补造。</p><button type="button" onClick={onRefresh}>查询本人交割单</button></section>;
  if (rows.length === 0) return <section aria-label="本人交割单"><p>暂无本人真实成交</p><button type="button" onClick={onRefresh}>刷新</button></section>;
  return <section aria-label="本人交割单">
    <button type="button" onClick={onRefresh}>刷新</button>
    {rows.length === 100 && onOlder !== undefined && <button type="button" onClick={onOlder}>更早成交</button>}
    <table>
      <thead><tr><th>日期</th><th>证券</th><th>方向</th><th>成交价</th><th>数量</th><th>成交额</th><th>佣金</th><th>印花税</th><th>过户费</th></tr></thead>
      <tbody>{rows.map((row) => <tr key={row.receipt_id}>
        <td>{row.civil_date}</td><td>{row.code}</td><td>{row.side === "Buy" ? "买入" : "卖出"}</td>
        <td>{yuan(row.price)}</td><td>{row.quantity_shares}股</td><td>{yuan(row.gross)}</td>
        <td>{yuan(row.actual_fees.commission)}</td><td>{yuan(row.actual_fees.stamp_tax)}</td><td>{yuan(row.actual_fees.transfer_fee)}</td>
      </tr>)}</tbody>
    </table>
  </section>;
}
