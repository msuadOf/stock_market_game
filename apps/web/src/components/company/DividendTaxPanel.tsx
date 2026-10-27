/* oxlint-disable react/only-export-components -- 精确金额文案与面板共用。 */
import type { AccountDividendTaxStatusView, DividendTaxOutstandingView } from "../../host/dividend-tax.ts";
import { useCompanyQuery } from "./use-company-query.ts";

export interface DividendTaxQueryResult {
  readonly status: AccountDividendTaxStatusView;
  readonly outstanding: readonly DividendTaxOutstandingView[];
}
interface DividendTaxPanelProps {
  /** 只返回本人事实；owner 或 generation 改变时同步更新 refreshKey。 */
  readonly onQuery: (() => Promise<DividendTaxQueryResult>) | undefined;
  readonly refreshKey: string;
}
const TAX_MODE_LABELS: Record<AccountDividendTaxStatusView["mode"], string> = {
  FlatWithholding: "简税（分红付款日按开局比例代扣，机构同样代扣，不按持股期补税）",
  AShareIndividual: "大 A 方式（个人差别化计税）",
  Exempt: "不扣税（连卖出印花税也免，佣金与过户费照付）",
};

function exactTaxText(amount: DividendTaxOutstandingView["outstanding"]): string {
  if (amount.denominator !== "1") return `${amount.numerator}/${amount.denominator} 分`;
  const cents = BigInt(amount.numerator);
  return `${cents / 100n}.${(cents % 100n).toString().padStart(2, "0")} 元（${amount.numerator} 分）`;
}
export function outstandingText(view: DividendTaxOutstandingView): string { return exactTaxText(view.outstanding); }
export function stockStatusLabel(status: AccountDividendTaxStatusView, stock: string): string {
  const row = status.stocks.find((item) => item.stock === stock);
  if (row === undefined) return "无股东名册";
  if (row.status === "FlatWithholding") return "简税代扣（付款日按开局比例直接扣，机构同样代扣）";
  if (row.status === "IndividualPublicMarket") return "个人差别化税账已配置";
  return status.mode === "Exempt" ? "不扣税模式" : "未配置个人税账（机构/企业差别计税未实现）";
}

/** 本人股息税查询只读投影；最近回执只描述发生当时，不能代替当前余额状态。 */
export function DividendTaxPanel({ onQuery, refreshKey }: DividendTaxPanelProps) {
  const [data, error, loading, refresh] = useCompanyQuery(onQuery, refreshKey);
  return <section className="company-dividend-tax" aria-label="股息税状态">
    <h4>股息税状态</h4>
    {onQuery === undefined ? <p>当前宿主不支持股息税状态查询。</p> : <>
      {loading && <p role="status">正在查询本人股息税状态…</p>}
      {error !== null && <p role="alert">股息税状态查询失败：{error}</p>}
      {!loading && error === null && data === null && <p>尚未查询股息税状态。</p>}
      {data !== null && <>
        <p>本局税务模式：{TAX_MODE_LABELS[data.status.mode]}；本人身份：{data.status.identity === "Personal" ? "个人" : "机构/企业（差别计税未实现）"}。</p>
        {data.status.stocks.length === 0 ? <p>当前没有已配置完整股东名册的证券。</p> : <ul>{data.status.stocks.map((row) => <li key={row.stock}>{row.stock} {stockStatusLabel(data.status, row.stock)}</li>)}</ul>}
        {data.outstanding.length === 0 ? <p>查询未返回本人个人股息税账记录。</p> : <table className="grid-table">
          <thead><tr><th>证券</th><th>当前未划收税额</th><th>最近实际划收回执（历史时点）</th></tr></thead>
          <tbody>{data.outstanding.map((view) => <tr key={JSON.stringify([view.account, view.stock])}>
            <td>{view.stock}</td><td>{outstandingText(view)}</td>
            <td>{view.latest_collection === null ? "尚无实际划收回执；本查询不触发划收。" : <>
              {view.latest_collection.day} · 操作序号 {view.latest_collection.operation_seq} · 事件 {view.latest_collection.event_id}：当时可用现金 {view.latest_collection.available_cash} 分，实收 {view.latest_collection.collected} 分，余款 {view.latest_collection.remaining_cash} 分，回执时未划收 {exactTaxText(view.latest_collection.outstanding)}。
              {view.latest_collection.needs_funds ? "该回执时资金不足。" : "该回执未标记资金不足。"}历史回执不表示当前余额均已尝试划收。
            </>}</td>
          </tr>)}</tbody>
        </table>}
      </>}
      <button type="button" disabled={loading} onClick={refresh}>刷新股息税状态</button>
    </>}
  </section>;
}
