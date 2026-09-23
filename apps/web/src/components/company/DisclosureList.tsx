import type { PublicReportSummary } from "../../types/engine.ts";
import { formatReportKind, formatReportPeriod, formatSecondOfDay } from "./company-presentation.ts";

interface DisclosureListProps {
  readonly reports: readonly PublicReportSummary[];
  readonly selectedReportId: string | null;
  readonly onSelect: (reportId: string) => void;
}

export function DisclosureList({ reports, selectedReportId, onSelect }: DisclosureListProps) {
  return (
    <section className="company-disclosures" aria-labelledby="company-disclosure-title">
      <h4 id="company-disclosure-title">公开披露</h4>
      <p className="company-disclosure-hint">横向滑动查看全部报告</p>
      <div className="company-disclosure-list" role="list" aria-label="公开报告列表">
        {reports.map((report) => (
          <div key={report.id} role="listitem">
            <button
              type="button"
              className={report.id === selectedReportId ? "is-selected" : ""}
              aria-pressed={report.id === selectedReportId}
              onClick={() => onSelect(report.id)}
            >
              <span>{formatReportKind(report.kind)} · {formatReportPeriod(report.period)}</span>
              <small>{report.published_date} {formatSecondOfDay(report.published_second_of_day)} 发布 · 版本 {report.version_sequence}</small>
            </button>
          </div>
        ))}
      </div>
    </section>
  );
}
