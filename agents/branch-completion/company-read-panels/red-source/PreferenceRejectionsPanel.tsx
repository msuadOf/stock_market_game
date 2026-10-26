/* oxlint-disable react/only-export-components -- 标签/摘要 helper 供 SSR 测试与面板共用（同 DividendTaxPanel 模式） */
import { useEffect, useRef, useState } from "react";
import type { CompanyPreferenceRejectionView } from "../../host/corporate-action-views.ts";

interface PreferenceRejectionsPanelProps {
  readonly companyId: string;
  /** 宿主查询入口（按公司查询偏好提案拒绝台账）；undefined 表示宿主明确不支持。 */
  readonly onQuery: ((companyId: string) => Promise<readonly CompanyPreferenceRejectionView[]>) | undefined;
  /** 刷新键：自然日或会话 generation 变化时重查（台账只在结算周期末日日结变化）。 */
  readonly refreshKey: string;
}

export function rejectionKindLabel(kind: CompanyPreferenceRejectionView["kind"]): string {
  return kind === "CashDividend" ? "现金分红" : "送转";
}

/** 拒绝台账摘要：保留评估日与拒绝原因原文（approve_* 错误或构造期无法成案原因）。 */
export function rejectionSummary(rejection: CompanyPreferenceRejectionView): string {
  return `${rejection.evaluated_on} ${rejectionKindLabel(rejection.kind)}提案被拒：${rejection.detail}`;
}

/**
 * 公司行为面板体系内的偏好提案拒绝台账区（ADR-0037）：展示该公司被制度拒绝
 * 的自动提案（同周期幂等留痕）。查询的是最近一次已完成日终存档中的公司系统
 * 状态——首个日终完成前引擎显式报错，面板如实展示，不冒充「无拒绝」；
 * 模拟日已推进但尚未完成当日日结时，显示的是上一日终快照（界面注明）。
 */
export function PreferenceRejectionsPanel({ companyId, onQuery, refreshKey }: PreferenceRejectionsPanelProps) {
  const [data, setData] = useState<readonly CompanyPreferenceRejectionView[] | null>(null);
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
    query(companyId).then(
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
  }, [onQuery, refreshKey, companyId]);

  if (onQuery === undefined) {
    return (
      <section className="company-preference-rejections" aria-label="偏好提案拒绝台账">
        <h4>偏好提案拒绝台账</h4>
        <p className="company-state">当前宿主不支持偏好拒绝台账查询。</p>
      </section>
    );
  }
  return (
    <section className="company-preference-rejections" aria-label="偏好提案拒绝台账">
      <h4>偏好提案拒绝台账</h4>
      <p className="company-state">台账读取最近一次已完成自然日日终的存档状态；当前模拟日推进后未完成日结前，以下不是最新评估。</p>
      {loading && <p className="company-state" role="status">正在查询公司偏好提案拒绝台账…</p>}
      {error !== null && <p className="company-state is-error" role="alert">偏好提案拒绝台账查询失败：{error}</p>}
      {!loading && error === null && data === null && <p className="company-state">尚未查询偏好提案拒绝台账。</p>}
      {data !== null && data.length === 0 && <p className="company-state">该公司没有偏好提案拒绝记录（未配置偏好=不自动产生方案）。</p>}
      {data !== null && data.length > 0 && (
        <table className="grid-table">
          <thead><tr><th>评估日</th><th>类别</th><th>拒绝原因</th></tr></thead>
          <tbody>
            {data.map((rejection) => (
              <tr key={`${rejection.kind}:${rejection.evaluated_on}`}>
                <td className="mono">{rejection.evaluated_on}</td>
                <td>{rejectionKindLabel(rejection.kind)}</td>
                <td>{rejection.detail}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <button type="button" disabled={loading} onClick={() => {
        const current = ++sequence.current;
        setLoading(true);
        setError(null);
        onQuery(companyId).then(
          (result) => { if (sequence.current === current) { setData(result); setLoading(false); } },
          (failure) => {
            if (sequence.current === current) {
              setError(failure instanceof Error ? failure.message : String(failure));
              setLoading(false);
            }
          },
        );
      }}>刷新拒绝台账</button>
    </section>
  );
}
