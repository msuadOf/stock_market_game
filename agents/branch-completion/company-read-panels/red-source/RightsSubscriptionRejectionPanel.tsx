/* oxlint-disable react/only-export-components -- 摘要 helper 供 SSR 测试与面板共用（同 DividendTaxPanel 模式） */
import { useEffect, useRef, useState } from "react";
import type { RejectedRightsSubscriptionView } from "../../host/corporate-action-views.ts";

interface RightsSubscriptionRejectionPanelProps {
  /** 宿主查询入口（owner 隔离：只返回本人配股认购拒绝回执）；undefined 表示宿主明确不支持。 */
  readonly onQuery: (() => Promise<readonly RejectedRightsSubscriptionView[]>) | undefined;
  /** 刷新键：自然日或会话 generation 变化时重查（回执只在日终产生）。 */
  readonly refreshKey: string;
}

/** 拒绝回执摘要：保留事件、申请数量、提交/拒绝日期与原因原文，不截断不静默。 */
export function rejectionSummary(receipt: RejectedRightsSubscriptionView): string {
  return `${receipt.event_id}：申请 ${receipt.requested_shares} 股被拒（${receipt.submitted_on} 提交、${receipt.rejected_on} 拒绝）——${receipt.reason}`;
}

/**
 * 公司行为面板体系内的配股认购拒绝回执区（M 批公开配售超额认购的极端竞态
 * 兜底留痕）：展示本人被日终显式拒绝的认购（owner 隔离查询），错误完整展示
 * 并允许手动重试；未启用配股机制或没有竞态拒绝时如实显示为空。
 */
export function RightsSubscriptionRejectionPanel({ onQuery, refreshKey }: RightsSubscriptionRejectionPanelProps) {
  const [data, setData] = useState<readonly RejectedRightsSubscriptionView[] | null>(null);
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
      <section className="company-rights-rejections" aria-label="配股认购拒绝回执">
        <h4>配股认购拒绝回执</h4>
        <p className="company-state">当前宿主不支持配股认购拒绝回执查询。</p>
      </section>
    );
  }
  return (
    <section className="company-rights-rejections" aria-label="配股认购拒绝回执">
      <h4>配股认购拒绝回执</h4>
      {loading && <p className="company-state" role="status">正在查询本人配股认购拒绝回执…</p>}
      {error !== null && <p className="company-state is-error" role="alert">配股认购拒绝回执查询失败：{error}</p>}
      {!loading && error === null && data === null && <p className="company-state">尚未查询配股认购拒绝回执。</p>}
      {data !== null && data.length === 0 && <p className="company-state">本人没有配股认购拒绝回执（仅公开配售超额认购的极端竞态会留痕）。</p>}
      {data !== null && data.length > 0 && (
        <table className="grid-table">
          <thead><tr><th>配股事件</th><th>申请股数</th><th>提交日</th><th>拒绝日</th><th>拒绝原因</th></tr></thead>
          <tbody>
            {data.map((receipt) => (
              <tr key={`${receipt.event_id}:${receipt.submitted_on}:${receipt.requested_shares}`}>
                <td className="mono">{receipt.event_id}</td>
                <td className="num">{receipt.requested_shares}</td>
                <td className="mono">{receipt.submitted_on}</td>
                <td className="mono">{receipt.rejected_on}</td>
                <td>{receipt.reason}</td>
              </tr>
            ))}
          </tbody>
        </table>
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
      }}>刷新拒绝回执</button>
    </section>
  );
}
