import { useCallback, useEffect, useRef, useState } from "react";
import type { EngineHost } from "../../host/engine-host.ts";
import type { ReportCorrectionStatus } from "../../host/report-corrections.ts";
import { parseCompanyReportCorrection } from "../../save/schema/company/report-corrections.ts";

export type ReportCorrectionControl = Pick<EngineHost, "submitReportCorrection" | "cancelReportCorrection" | "queryReportCorrections">;

interface Props {
  readonly companyId: string;
  readonly reportId: string | null;
  readonly control: ReportCorrectionControl;
  readonly refreshKey?: string;
}

export function ReportCorrectionPanel({ companyId, reportId, control, refreshKey }: Props) {
  const [operationId, setOperationId] = useState("");
  const [reason, setReason] = useState("");
  const [entries, setEntries] = useState("");
  const [status, setStatus] = useState<ReportCorrectionStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const alive = useRef(false);
  const querySequence = useRef(0);

  const refresh = useCallback(async () => {
    const sequence = ++querySequence.current;
    try {
      const next = await control.queryReportCorrections();
      if (alive.current && sequence === querySequence.current) { setStatus(next); setError(null); }
    } catch (failure) {
      if (alive.current && sequence === querySequence.current) setError(`市场更正查询失败：${failure instanceof Error ? failure.message : String(failure)}。请复制详情反馈，或刷新待办核对。`);
    }
  }, [control]);

  useEffect(() => {
    alive.current = true;
    void refresh();
    return () => { alive.current = false; querySequence.current += 1; };
  }, [refresh, refreshKey]);

  const perform = async (operation: "submit" | "cancel", identity?: string) => {
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      if (operation === "submit") {
        if (reportId === null || !/^(0|[1-9]\d*)$/.test(reportId)) throw new Error("请选择真实已公开报告的 PublicationId");
        const request = parseCompanyReportCorrection({ operation_id: operationId, company: companyId, supersedes: Number(reportId), entries: JSON.parse(entries) as unknown, reason });
        await control.submitReportCorrection(request);
        if (alive.current) setMessage("宿主已确认更正请求；请核对待办或已完成结果，入队不等于公开成功。同一 operation_id 重试必须保留原载荷。");
      } else {
        if (identity === undefined) throw new Error("取消更正缺少 operation_id");
        await control.cancelReportCorrection(identity);
        if (alive.current) setMessage("宿主已确认取消待办；已完成更正不能撤销。");
      }
      if (alive.current) await refresh();
    } catch (failure) {
      if (alive.current) setError(`市场财报更正${operation === "submit" ? "提交" : "取消"}失败：${failure instanceof Error ? failure.message : String(failure)}。请保留 operation_id 与凭证，刷新权威待办核对后反馈。`);
    } finally {
      if (alive.current) setBusy(false);
    }
  };

  return <details className="company-correction-control">
    <summary>市场控制 · 财报差错更正</summary>
    <p>当前单玩家市场由合法 owner 操作；不是本人交易账户功能。未来共享市场须校验组合后的市场控制能力，不授予普通交易玩家此权限。</p>
    <p>只在成功日终过账并公开；失败无部分账务或部分公开，暂停后可取消问题待办并重试。新局／换档丢弃日内队列，已完成结果随日终存档保留。</p>
    <p>这是受控差错更正，不是注资。Industrial／Bank／Insurance／RealEstate 均使用实际 TaxOwner。仅输入有真实来源的 JournalEntry[]；amount 为 AccountingAmount 元字符串，不是 Money 分。不支持用原始凭证修改应收、应付、库存、借款、存贷款、保险合同组及主营保险损益、地产项目、税等结构化子账。</p>
    <form onSubmit={(event) => { event.preventDefault(); void perform("submit"); }}>
      <p>公司 {companyId} · 目标 PublicationId {reportId === null ? "请先选择已公开报告" : reportId}</p>
      <label>operation_id（重试保持不变）<input value={operationId} onChange={(event) => setOperationId(event.currentTarget.value)} required disabled={busy} /></label>
      <label>更正原因<input value={reason} onChange={(event) => setReason(event.currentTarget.value)} required disabled={busy} /></label>
      <label>真实 JournalEntry[] JSON（会计元）<textarea value={entries} onChange={(event) => setEntries(event.currentTarget.value)} required disabled={busy} rows={6} /></label>
      <button type="submit" disabled={busy || reportId === null}>提交日终更正</button>
    </form>
    {message !== null && <p role="status">{message}</p>}
    {error !== null && <p role="alert">{error}</p>}
    <button type="button" disabled={busy} onClick={() => void refresh()}>刷新待办与已完成结果</button>
    <h4>待办（仅待办可取消）</h4>
    {status === null ? <p>尚未取得权威更正状态。</p> : status.pending.length === 0 ? <p>没有待办更正。</p> : <ul>{status.pending.map((request) => <li key={request.operation_id}>{request.operation_id} · {request.company} · 替代 {request.supersedes} · {request.reason}<button type="button" disabled={busy} onClick={() => void perform("cancel", request.operation_id)}>取消待办</button></li>)}</ul>}
    <h4>已完成（日终实际公开结果）</h4>
    {status !== null && (Object.keys(status.completed).length === 0 ? <p>没有已完成更正。</p> : <ul>{Object.values(status.completed).map((item) => <li key={item.request.operation_id}>{item.request.operation_id} · {item.request.company} · {item.committed_at.date} · 新 PublicationId：{item.publications.join("、")}</li>)}</ul>)}
  </details>;
}
