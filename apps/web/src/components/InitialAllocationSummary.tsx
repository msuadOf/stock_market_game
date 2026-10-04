import type { InitialAllocation } from "../host/initial-allocation.ts";
import "./FloatAllocationInput.css";

const CATEGORY_LABELS = { Retail: "散户", Inst: "机构", Hot: "游资" };

export function InitialAllocationSummary({ allocation }: { readonly allocation: InitialAllocation | null }) {
  if (allocation === null) return <p className="float-allocation-summary">本次读档不展示开局分配；现有持仓不能冒充初始持仓。</p>;
  return <details className="float-allocation-details" open>
    <summary>本局实际初始持仓分配</summary>
    <p>由本局 Engine 首次开跑前的账户持仓汇总，不是预计比例；后续交易不改变此开局记录。设置只影响下一次新局。</p>
    {allocation.stocks.map((stock) => <section key={stock.code} aria-label={`${stock.code}实际初始分配`}>
      <h4>{stock.code} · 流通盘 {stock.float_shares.toLocaleString("zh-CN")} 股</h4>
      <div className="float-allocation-table-wrap"><table className="float-allocation-summary">
        <thead><tr><th>NPC 类别</th><th>实际股数（股）</th><th>占流通盘</th><th>账户数（人）</th><th>零持股（人）</th></tr></thead>
        <tbody>{stock.categories.map((category) => <tr key={category.kind}>
          <th scope="row">{CATEGORY_LABELS[category.kind]}</th>
          <td>{category.shares.toLocaleString("zh-CN")}</td>
          <td>{stock.float_shares === 0 ? "—" : `${(category.shares / stock.float_shares * 100).toFixed(2)}%`}</td>
          <td>{category.account_count.toLocaleString("zh-CN")}</td>
          <td>{category.zero_holders.toLocaleString("zh-CN")}</td>
        </tr>)}</tbody>
      </table></div>
      <p>已分配 {stock.categories.reduce((sum, category) => sum + category.shares, 0).toLocaleString("zh-CN")} 股
        ＋ 未分配 {stock.unallocated_shares.toLocaleString("zh-CN")} 股 ＝ 流通盘 {stock.float_shares.toLocaleString("zh-CN")} 股（整数对账一致）</p>
      {stock.unallocated_shares > 0 && <p role="status">没有 NPC，流通盘尚未分配，不会凭空给玩家股份。</p>}
    </section>)}
  </details>;
}
