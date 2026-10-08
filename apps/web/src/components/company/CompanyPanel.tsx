import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import type { PublicReportAvailability, PublicReportAvailabilityQuery, PublicReportKind, PublicReportScope } from "../../types/engine.ts";
import { selectCompanyReading, type CompanyReading, type CompanyState } from "../../store/company-slice.ts";
import { publicCompanies, publicCompanyById, type PublicCompany } from "./company-catalog.ts";
import { DisclosureList } from "./DisclosureList.tsx";
import { FinancialStatementTable } from "./FinancialStatementTable.tsx";
import {
  formatCalendarStatus,
  formatReportKind,
  formatReportPeriod,
  reportRoeIndicators,
  reportStatementRows,
  reportViewState,
  selectVisibleReportId,
} from "./company-presentation.ts";
import { ReportNotes } from "./ReportNotes.tsx";
import { visibleReports } from "./company-view-model.ts";
import { ReportCorrectionPanel, type ReportCorrectionControl } from "./ReportCorrectionPanel.tsx";
import { DividendTaxPanel, type DividendTaxQueryResult } from "./DividendTaxPanel.tsx";
import { RightsSubscriptionRejectionPanel } from "./RightsSubscriptionRejectionPanel.tsx";
import { PreferenceRejectionsPanel } from "./PreferenceRejectionsPanel.tsx";
import { CompanyContractPanel } from "./CompanyContractPanel.tsx";
import type { CompanyCapabilities, OwnerRightsOfferingView, PeriodChangeExplanation, FlatWithholdingReceiptView } from "../../host/engine-host.ts";
import type { CompanyPreferenceRejectionView, RejectedRightsSubscriptionView } from "../../host/corporate-action-views.ts";
import { isIsoMonthEnd, reportAvailabilityReason } from "./report-availability.ts";
import "./company.css";

interface CompanyPanelProps {
  readonly allowCompanySelection?: boolean;
  readonly companyId: string | null;
  readonly companyState: CompanyState;
  readonly initialCivilDate: string;
  readonly onCompanyChange: (companyId: string) => void;
  readonly onReadingChange: (changes: Partial<CompanyReading>) => void;
  readonly onQuery: (companyId: string, cursor: string | null) => void;
  readonly onAvailabilityQuery: (query: PublicReportAvailabilityQuery) => Promise<PublicReportAvailability>;
  readonly onAdvanceCivilDay: () => Promise<void>;
  readonly reportCorrectionControl?: ReportCorrectionControl | null;
  readonly timelineGeneration?: string | null;
  /** 本人股息税状态查询（owner 隔离）；undefined 表示宿主明确不支持，面板显式提示。 */
  readonly onDividendTaxQuery?: (() => Promise<DividendTaxQueryResult>) | undefined;
  /** 本人配股认购拒绝回执查询（owner 隔离）；undefined 表示宿主明确不支持，面板显式提示。 */
  readonly onRightsRejectionQuery?: (() => Promise<readonly RejectedRightsSubscriptionView[]>) | undefined;
  /** 该公司偏好提案拒绝台账查询；undefined 表示宿主明确不支持，面板显式提示。 */
  readonly onPreferenceRejectionsQuery?: ((companyId: string) => Promise<readonly CompanyPreferenceRejectionView[]>) | undefined;
  /** 公司共同契约能力面查询（F 批收口；owner 隔离）；undefined 表示宿主明确不支持，面板显式提示。 */
  readonly onCapabilitiesQuery?: ((companyId: string) => Promise<CompanyCapabilities>) | undefined;
  /** 本人配股权益查询（owner 隔离）；undefined 表示宿主明确不支持。 */
  readonly onOwnerRightsQuery?: (() => Promise<readonly OwnerRightsOfferingView[]>) | undefined;
  /** 期间变化解释查询；undefined 表示宿主明确不支持。 */
  readonly onExplanationQuery?: ((companyId: string, periodEnd: string) => Promise<PeriodChangeExplanation>) | undefined;
  /** 简税代扣回执查询（owner 隔离）；undefined 表示宿主明确不支持。 */
  readonly onFlatReceiptsQuery?: (() => Promise<readonly FlatWithholdingReceiptView[]>) | undefined;
}

function CompanyIdentity({ company }: { readonly company: PublicCompany }) {
  return <div className="company-identity"><h3>{company.name}</h3><p>{company.industry} · {company.business}</p></div>;
}

function moveTabFocus(event: KeyboardEvent<HTMLButtonElement>, ids: readonly string[]): void {
  if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
  event.preventDefault();
  const buttons = Array.from(event.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>("[role=tab]") ?? []);
  const current = buttons.indexOf(event.currentTarget);
  const next = event.key === "ArrowRight" ? current + 1 : current - 1;
  buttons[(next + ids.length) % ids.length]?.focus();
}

export function CompanyPanel({ allowCompanySelection = true, companyId, companyState, initialCivilDate, onCompanyChange, onReadingChange, onQuery, onAvailabilityQuery, onAdvanceCivilDay, reportCorrectionControl, timelineGeneration, onDividendTaxQuery, onRightsRejectionQuery, onPreferenceRejectionsQuery, onCapabilitiesQuery, onOwnerRightsQuery, onExplanationQuery, onFlatReceiptsQuery }: CompanyPanelProps) {
  const company = companyId === null ? undefined : publicCompanyById(companyId);
  const cache = companyId === null ? undefined : companyState.companies[companyId];
  const rootPage = cache?.pages.root;
  const state = reportViewState(rootPage);
  const pageSummary = useMemo(() => visibleReports(cache), [cache]);
  const reports = pageSummary.reports;
  const reading = selectCompanyReading(companyState, companyId);
  const { selectedReportId, selectedStatementId, exactAmountsVisible } = reading;
  const currentReportId = selectVisibleReportId(selectedReportId ?? cache?.currentReportId ?? null, reports.map((report) => report.id));
  const [periodEnd, setPeriodEnd] = useState("");
  const [kind, setKind] = useState<PublicReportKind | "">("");
  const [scopeKind, setScopeKind] = useState<"Standalone" | "Consolidated" | "">("");
  const [availability, setAvailability] = useState<PublicReportAvailability | null>(null);
  const availabilityOwner = JSON.stringify([companyId, timelineGeneration]);
  const [availabilityResultOwner, setAvailabilityResultOwner] = useState(availabilityOwner);
  const [availabilityError, setAvailabilityError] = useState<string | null>(null);
  const [availabilityErrorOwner, setAvailabilityErrorOwner] = useState(availabilityOwner);
  const [availabilityLoading, setAvailabilityLoading] = useState(false);
  const [availabilityLoadingOwner, setAvailabilityLoadingOwner] = useState(availabilityOwner);
  const [formOwner, setFormOwner] = useState(availabilityOwner);
  const querySequence = useRef(0);
  const currentAvailabilityOwner = useRef(availabilityOwner);
  currentAvailabilityOwner.current = availabilityOwner;
  const isCurrentForm = formOwner === availabilityOwner;
  const currentPeriodEnd = isCurrentForm ? periodEnd : "";
  const currentKind = isCurrentForm ? kind : "";
  const currentScopeKind = isCurrentForm ? scopeKind : "";
  const availabilityReport = availabilityResultOwner === availabilityOwner && availability?.status === "Available" ? availability.report : undefined;
  const isAvailabilityLoading = availabilityLoading && availabilityLoadingOwner === availabilityOwner;
  const report = availabilityReport ?? (currentReportId === null ? undefined : reports.find((item) => item.id === currentReportId));
  const statements = report === undefined ? [] : reportStatementRows(report.financials);
  const selectedStatement = statements.find((statement) => statement.id === selectedStatementId) ?? statements[0];

  useEffect(() => {
    if (companyId !== null && rootPage === undefined) onQuery(companyId, null);
  }, [companyId, onQuery, rootPage]);

  useEffect(() => {
    if (state.kind !== "ready" && state.kind !== "empty") return;
    if (currentReportId !== selectedReportId) onReadingChange({ selectedReportId: currentReportId });
  }, [currentReportId, onReadingChange, selectedReportId, state.kind]);

  useEffect(() => {
    setAvailability(null);
    setAvailabilityResultOwner(availabilityOwner);
    setAvailabilityError(null);
    setAvailabilityErrorOwner(availabilityOwner);
    setAvailabilityLoading(false);
    setAvailabilityLoadingOwner(availabilityOwner);
    setFormOwner(availabilityOwner);
    setPeriodEnd("");
    setKind("");
    setScopeKind("");
    querySequence.current += 1;
  }, [availabilityOwner]);

  const queryAvailability = async () => {
    if (companyId === null || currentAvailabilityOwner.current !== availabilityOwner || !isIsoMonthEnd(currentPeriodEnd) || currentKind === "" || currentScopeKind === "") return;
    const queryScope: PublicReportScope = currentScopeKind === "Standalone"
      ? { Standalone: { entity_id: companyId } }
      : { Consolidated: { root_entity_id: companyId } };
    const query: PublicReportAvailabilityQuery = { company_id: companyId, period_end: currentPeriodEnd, kind: currentKind, scope: queryScope };
    const sequence = ++querySequence.current;
    setAvailability(null);
    setAvailabilityError(null);
    setAvailabilityLoading(true);
    setAvailabilityLoadingOwner(availabilityOwner);
    try {
      const result = await onAvailabilityQuery(query);
      if (sequence === querySequence.current && availabilityOwner === currentAvailabilityOwner.current) { setAvailability(result); setAvailabilityResultOwner(availabilityOwner); }
    } catch (error) {
      if (sequence === querySequence.current && availabilityOwner === currentAvailabilityOwner.current) { setAvailabilityError(error instanceof Error ? error.message : String(error)); setAvailabilityErrorOwner(availabilityOwner); }
    } finally {
      if (sequence === querySequence.current && availabilityOwner === currentAvailabilityOwner.current) setAvailabilityLoading(false);
    }
  };

  const clearAvailability = () => {
    querySequence.current += 1;
    setAvailability(null);
    setAvailabilityError(null);
    setAvailabilityLoading(false);
  };

  if (company === undefined || companyId === null) {
    return <section className="company-panel" aria-label="公司信息"><p className="company-empty">当前证券没有可公开查询的公司映射。</p></section>;
  }

  return (
    <section className="company-panel" aria-label="公司信息" data-company-id={company.id}>
      <header className="company-panel-head">
        <div>
          {allowCompanySelection && <label className="company-picker"><span>公司</span><select aria-label="选择公司" value={company.id} onChange={(event) => onCompanyChange(event.currentTarget.value)}>{publicCompanies.map((item) => <option key={item.id} value={item.id}>{item.name} · {item.industry}</option>)}</select></label>}
          <CompanyIdentity company={company} />
        </div>
        <div className="company-calendar" aria-label="当前模拟日历">
          <span>模拟自然日</span><strong>{companyState.civilDate ?? initialCivilDate}</strong>
          <small>{companyState.dayStatus === null ? "日历状态等待权威事件" : formatCalendarStatus(companyState.dayStatus)}</small>
          <button type="button" onClick={() => void onAdvanceCivilDay()}>推进模拟自然日</button>
        </div>
      </header>
      {reportCorrectionControl != null && <ReportCorrectionPanel key={`${timelineGeneration}:${companyId}`} companyId={companyId} reportId={currentReportId} control={reportCorrectionControl} refreshKey={companyState.civilDate ?? initialCivilDate} />}
      <DividendTaxPanel onQuery={onDividendTaxQuery} refreshKey={`${timelineGeneration ?? "current"}:${companyState.civilDate ?? initialCivilDate}`} />
      <RightsSubscriptionRejectionPanel onQuery={onRightsRejectionQuery} refreshKey={`${timelineGeneration ?? "current"}:${companyState.civilDate ?? initialCivilDate}`} />
      <PreferenceRejectionsPanel companyId={companyId} onQuery={onPreferenceRejectionsQuery} refreshKey={`${timelineGeneration ?? "current"}:${companyState.civilDate ?? initialCivilDate}:${companyId}`} />
      {companyId !== null && (
        <CompanyContractPanel
          companyId={companyId}
          onCapabilitiesQuery={onCapabilitiesQuery}
          onOwnerRightsQuery={onOwnerRightsQuery}
          onExplanationQuery={onExplanationQuery}
          onFlatReceiptsQuery={onFlatReceiptsQuery}
          refreshKey={`${timelineGeneration ?? "current"}:${companyState.civilDate ?? initialCivilDate}`}
        />
      )}
      {state.kind === "idle" && <p className="company-state">正在请求已公开报告…</p>}
      {state.kind === "loading" && <p className="company-state" aria-live="polite">正在加载已公开报告…</p>}
      {state.kind === "empty" && availabilityReport === undefined && <p className="company-state">截至当前模拟自然日，该公司没有已公开报告。</p>}
      {state.kind === "error" && <p className="company-state is-error" role="alert">公开报告查询失败：{state.message}</p>}
      {state.kind === "unavailable" && <p className="company-state is-error" role="alert">公开报告不可用：{state.message}</p>}
      <div className="company-availability" aria-label="按期间查询公开报告">
        <label>报告期末<input aria-label="报告期末" type="date" value={currentPeriodEnd} onChange={(event) => { clearAvailability(); setPeriodEnd(event.currentTarget.value); }} /></label>
        <label>报告类型<select aria-label="报告类型" value={currentKind} onChange={(event) => { clearAvailability(); setKind(event.currentTarget.value as PublicReportKind | ""); }}><option value="">请选择</option><option value="Monthly">月度</option><option value="Quarter">季度</option><option value="HalfYear">半年度</option><option value="Annual">年度</option></select></label>
        <label>Scope<select aria-label="报告 scope" value={currentScopeKind} onChange={(event) => { clearAvailability(); setScopeKind(event.currentTarget.value as typeof scopeKind); }}><option value="">请选择</option><option value="Standalone">单体</option><option value="Consolidated">合并</option></select></label>
        <button type="button" disabled={isAvailabilityLoading || companyId === null || !isIsoMonthEnd(currentPeriodEnd) || currentKind === "" || currentScopeKind === ""} onClick={() => void queryAvailability()}>{isAvailabilityLoading ? "正在查询…" : "查询该报告是否已公开"}</button>
        {availabilityError !== null && availabilityErrorOwner === availabilityOwner && <p className="company-state is-error" role="alert">公开报告可用性查询失败：{availabilityError}</p>}
        {availabilityReport !== undefined && <p className="company-state" role="status">该报告已公开，以下显示完整财务信息。</p>}
        {availabilityResultOwner === availabilityOwner && availability?.status === "Unavailable" && <p className="company-state" role="status">{reportAvailabilityReason(availability.reason)}</p>}
      </div>
      {(state.kind === "ready" || availabilityReport !== undefined) && report !== undefined && selectedStatement !== undefined && (
        <div className="company-report">
          {state.kind === "ready" && <DisclosureList reports={reports} selectedReportId={availabilityReport === undefined ? currentReportId : null} onSelect={(id) => { clearAvailability(); onReadingChange({ selectedReportId: id }); }} />}
          {state.kind === "ready" && pageSummary.nextCursor !== null && <button className="company-load-more" type="button" onClick={() => onQuery(company.id, pageSummary.nextCursor)}>加载更多公开报告</button>}
          <div className="company-report-summary">
            <span>{formatReportKind(report.kind)} · 期间 {formatReportPeriod(report.period)}</span>
            <span>公开编号 {report.id} · 版本 {report.version_sequence}</span>
            <button type="button" onClick={() => onReadingChange({ exactAmountsVisible: !exactAmountsVisible })} aria-pressed={exactAmountsVisible}>
              {exactAmountsVisible ? "显示缩写金额" : "查看精确值"}
            </button>
          </div>
          <div className="company-report-indicators" role="group" aria-label="报告窗口净资产收益率">
            {reportRoeIndicators(report.financials.roe).map((indicator) => <p key={indicator.label} className={indicator.unavailable ? "is-unavailable" : undefined}><span>{indicator.label}</span><strong>{indicator.value}</strong></p>)}
          </div>
          <div className="company-statement-tabs" role="tablist" aria-label="财务报表">
            {statements.map((statement) => <button id={`company-statement-tab-${statement.id}`} key={statement.id} type="button" role="tab" aria-controls={`company-statement-panel-${statement.id}`} aria-selected={selectedStatement.id === statement.id} tabIndex={selectedStatement.id === statement.id ? 0 : -1} onKeyDown={(event) => moveTabFocus(event, statements.map((item) => item.id))} onClick={() => onReadingChange({ selectedStatementId: statement.id })}>{statement.title}</button>)}
          </div>
          <div id={`company-statement-panel-${selectedStatement.id}`} role="tabpanel" aria-labelledby={`company-statement-tab-${selectedStatement.id}`}>
            <FinancialStatementTable statement={selectedStatement} exactAmountsVisible={exactAmountsVisible} />
          </div>
          <ReportNotes report={report} />
        </div>
      )}
    </section>
  );
}
