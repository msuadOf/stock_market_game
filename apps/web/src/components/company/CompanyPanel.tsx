import { useEffect, useMemo, useState, type KeyboardEvent } from "react";
import type { CompanyState } from "../../store/company-slice.ts";
import { publicCompanies, publicCompanyById, type PublicCompany } from "./company-catalog.ts";
import { DisclosureList } from "./DisclosureList.tsx";
import { FinancialStatementTable } from "./FinancialStatementTable.tsx";
import {
  formatCalendarStatus,
  formatReportKind,
  formatReportPeriod,
  reportStatementRows,
  reportViewState,
  selectVisibleReportId,
} from "./company-presentation.ts";
import { ReportNotes } from "./ReportNotes.tsx";
import { visibleReports } from "./company-view-model.ts";
import "./company.css";

interface CompanyPanelProps {
  readonly companyId: string | null;
  readonly companyState: CompanyState;
  readonly initialCivilDate: string;
  readonly onCompanyChange: (companyId: string) => void;
  readonly onQuery: (companyId: string, cursor: string | null) => void;
  readonly onAdvanceCivilDay: () => Promise<void>;
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

export function CompanyPanel({ companyId, companyState, initialCivilDate, onCompanyChange, onQuery, onAdvanceCivilDay }: CompanyPanelProps) {
  const company = companyId === null ? undefined : publicCompanyById(companyId);
  const cache = companyId === null ? undefined : companyState.companies[companyId];
  const rootPage = cache?.pages.root;
  const state = reportViewState(rootPage);
  const pageSummary = useMemo(() => visibleReports(cache), [cache]);
  const reports = pageSummary.reports;
  const [selectedReportId, setSelectedReportId] = useState<string | null>(null);
  const [selectedStatementId, setSelectedStatementId] = useState("balance");
  const [exactAmountsVisible, setExactAmountsVisible] = useState(false);
  const currentReportId = selectVisibleReportId(selectedReportId ?? cache?.currentReportId ?? null, reports.map((report) => report.id));
  const report = currentReportId === null ? undefined : reports.find((item) => item.id === currentReportId);
  const statements = report === undefined ? [] : reportStatementRows(report.accounting);
  const selectedStatement = statements.find((statement) => statement.id === selectedStatementId) ?? statements[0];

  useEffect(() => {
    if (companyId !== null && rootPage === undefined) onQuery(companyId, null);
  }, [companyId, onQuery, rootPage]);

  useEffect(() => {
    setSelectedReportId((current) => selectVisibleReportId(current, reports.map((item) => item.id)));
  }, [reports]);

  if (company === undefined || companyId === null) {
    return <section className="company-panel" aria-label="公司信息"><p className="company-empty">当前证券没有可公开查询的公司映射。</p></section>;
  }

  return (
    <section className="company-panel" aria-label="公司信息" data-company-id={company.id}>
      <header className="company-panel-head">
        <div>
          <label className="company-picker"><span>公司</span><select aria-label="选择公司" value={company.id} onChange={(event) => onCompanyChange(event.currentTarget.value)}>{publicCompanies.map((item) => <option key={item.id} value={item.id}>{item.name} · {item.industry}</option>)}</select></label>
          <CompanyIdentity company={company} />
        </div>
        <div className="company-calendar" aria-label="当前模拟日历">
          <span>模拟自然日</span><strong>{companyState.civilDate ?? initialCivilDate}</strong>
          <small>{companyState.dayStatus === null ? "日历状态等待权威事件" : formatCalendarStatus(companyState.dayStatus)}</small>
          <button type="button" onClick={() => void onAdvanceCivilDay()}>推进模拟自然日</button>
        </div>
      </header>
      {state.kind === "idle" && <p className="company-state">正在请求已公开报告…</p>}
      {state.kind === "loading" && <p className="company-state" aria-live="polite">正在加载已公开报告…</p>}
      {state.kind === "empty" && <p className="company-state">截至当前模拟自然日，该公司没有已公开报告。</p>}
      {state.kind === "error" && <p className="company-state is-error" role="alert">公开报告查询失败：{state.message}</p>}
      {state.kind === "unavailable" && <p className="company-state is-error" role="alert">公开报告不可用：{state.message}</p>}
      {state.kind === "ready" && report !== undefined && selectedStatement !== undefined && (
        <div className="company-report">
          <DisclosureList reports={reports} selectedReportId={currentReportId} onSelect={setSelectedReportId} />
          {pageSummary.nextCursor !== null && <button className="company-load-more" type="button" onClick={() => onQuery(company.id, pageSummary.nextCursor)}>加载更多公开报告</button>}
          <div className="company-report-summary">
            <span>{formatReportKind(report.kind)} · 期间 {formatReportPeriod(report.period)}</span>
            <span>公开编号 {report.id} · 版本 {report.version_sequence}</span>
            <button type="button" onClick={() => setExactAmountsVisible((visible) => !visible)} aria-pressed={exactAmountsVisible}>
              {exactAmountsVisible ? "显示缩写金额" : "查看精确值"}
            </button>
          </div>
          <div className="company-statement-tabs" role="tablist" aria-label="财务报表">
            {statements.map((statement) => <button id={`company-statement-tab-${statement.id}`} key={statement.id} type="button" role="tab" aria-controls={`company-statement-panel-${statement.id}`} aria-selected={selectedStatement.id === statement.id} tabIndex={selectedStatement.id === statement.id ? 0 : -1} onKeyDown={(event) => moveTabFocus(event, statements.map((item) => item.id))} onClick={() => setSelectedStatementId(statement.id)}>{statement.title}</button>)}
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
