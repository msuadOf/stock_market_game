import { useEffect, useRef, useState } from "react";
import type { AccountDividendTaxStatusView, DividendTaxOutstandingView } from "../../host/engine-host.ts";
import type { Cents } from "../../types/engine.ts";
import { yuan } from "../../utils/format.ts";

export interface DividendTaxQueryResult {
  readonly status: AccountDividendTaxStatusView;
  readonly outstanding: readonly DividendTaxOutstandingView[];
}

interface DividendTaxPanelProps {
  /** 宿主查询入口（owner 隔离：只返回本人股息税状态）；undefined 表示宿主明确不支持。 */
  readonly onQuery: (() => Promise<DividendTaxQueryResult>) | undefined;
  /** 刷新键：自然日或会话 generation 变化时重查（税事实只在日终变化）。 */
  readonly refreshKey: string;
}

const TAX_MODE_LABELS: Record<AccountDividendTaxStatusView["mode"], string> = {
  FlatWithholding: "简税（分红到账时按比例直接代扣）",
  AShareIndividual: "大 A 方式（个人差别化计税）",
  Exempt: "不扣税（连印花税也免）",
};

const IDENTITY_LABELS: Record<AccountDividendTaxStatusView["identity"], string> = {
  Personal: "个人",
  NonIndividualPending: "机构/企业（计税未实现，保持不计税）",
};

/** 未清税额恒为整数分（分母 1）；非整数分数按原样显式展示，不静默取整。 */
export function outstandingText(view: DividendTaxOutstandingView): string {
  if (view.outstanding.denominator === "1") {
    return `${yuan(view.outstanding.numerator as Cents)} 元`;
  }
  return `${view.outstanding.numerator}/${view.outstanding.denominator} 分`;
}

export function stockStatusLabel(status: AccountDividendTaxStatusView, stock: string): string {
  const row = status.stocks.find((item) => item.stock === stock);
  if (row === undefined) return "无股东名册";
  if (row.status === "FlatWithholding") return "简税代扣（付款日按开局比例直接扣）";
  if (row.status === "IndividualPublicMarket") return "个人差别化税账已配置";
  return status.mode === "Exempt" ? "不扣税模式" : "未配置个人税账（不产生个人税事实）";
}

/**
 * 公司行为面板体系内的股息税状态区：展示本局税务模式、本人纳税人身份、
 * 各登记证券税账状态与未清税额（needs_funds 显式提示，不静默）。
 * 查询为 owner 隔离（宿主只返回本人数据）；错误完整展示并允许手动重试。
 */
export function DividendTaxPanel({ onQuery, refreshKey }: DividendTaxPanelProps) {
  const [data, setData] = useState<DividendTaxQueryResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const sequence = useRef(0);

  useEffect(() => {
    if (onQuery === undefined) return;
    const query = onQuery;
    const current = ++sequence.current;
    setData(null);
    setError(null);
    setLoading(true);
    query().then(
      (result) => {
        if (sequence.current === current) { setData(result); setLoading(false); }
      },
      (failure) => {
        if (sequence.current === current) {
          setError(failure instanceof Error ? failure.message : String(failure));
          setLoading(false);
        }
      },
    );
  }, [onQuery, refreshKey]);

  if (onQuery === undefined) {
    return (
      <section className="company-dividend-tax" aria-label="股息税状态">
        <h4>股息税状态</h4>
        <p className="company-state">当前宿主不支持股息税状态查询。</p>
      </section>
    );
  }
  return (
    <section className="company-dividend-tax" aria-label="股息税状态">
      <h4>股息税状态</h4>
      {loading && <p className="company-state" role="status">正在查询本人股息税状态…</p>}
      {error !== null && <p className="company-state is-error" role="alert">股息税状态查询失败：{error}</p>}
      {!loading && error === null && data === null && <p className="company-state">尚未查询股息税状态。</p>}
      {data !== null && (
        <>
          <p className="company-state">
            本局税务模式：<strong>{TAX_MODE_LABELS[data.status.mode]}</strong>；本人身份：{IDENTITY_LABELS[data.status.identity]}。
          </p>
          {data.status.stocks.length === 0 && <p className="company-state">当前没有已配置完整股东名册的证券。</p>}
          {data.status.stocks.length > 0 && (
            <ul>
              {data.status.stocks.map((row) => (
                <li key={row.stock}><span className="mono">{row.stock}</span> {stockStatusLabel(data.status, row.stock)}</li>
              ))}
            </ul>
          )}
          {data.outstanding.length === 0 && <p className="company-state">没有未划收的股息税额。</p>}
          {data.outstanding.length > 0 && (
            <table className="grid-table">
              <thead><tr><th>证券</th><th>未划收税额</th><th>状态</th></tr></thead>
              <tbody>
                {data.outstanding.map((view) => (
                  <tr key={view.stock}>
                    <td className="mono">{view.stock}</td>
                    <td className="num">{outstandingText(view)}</td>
                    <td>{view.needs_funds ? "资金不足，待补足后由日终继续追缴" : "已结清"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </>
      )}
      <button type="button" disabled={loading} onClick={() => {
        const current = ++sequence.current;
        setLoading(true);
        setError(null);
        onQuery().then(
          (result) => { if (sequence.current === current) { setData(result); setLoading(false); } },
          (failure) => {
            if (sequence.current === current) {
              setError(failure instanceof Error ? failure.message : String(failure));
              setLoading(false);
            }
          },
        );
      }}>刷新股息税状态</button>
    </section>
  );
}
