import type { PublicReportSummary } from "../../types/engine.ts";
import { formatComparison, formatSecondOfDay } from "./company-presentation.ts";

interface ReportNotesProps {
  readonly report: PublicReportSummary;
}

export function ReportNotes({ report }: ReportNotesProps) {
  const comparison = formatComparison(report.accounting.prior_year_net_income);
  return (
    <section className="company-notes" aria-labelledby="company-notes-title">
      <h4 id="company-notes-title">报表附注与口径</h4>
      <dl>
        <div><dt>批准时刻</dt><dd>{report.approved_date} {formatSecondOfDay(report.approved_second_of_day)}</dd></div>
        <div><dt>发布时刻</dt><dd>{report.published_date} {formatSecondOfDay(report.published_second_of_day)}</dd></div>
        <div><dt>报告范围</dt><dd>公开摘要未提供单体或合并范围</dd></div>
        <div><dt>上年同期累计净利润</dt><dd className={comparison.kind}>{comparison.text}</dd></div>
      </dl>
      {report.supersedes !== null && <p className="company-correction">本版更正了公开报告 #{report.supersedes}；历史版本仍可查询。</p>}
      <p>本界面只显示已公开摘要，不显示总账、经营台账或 NPC 私有信息。</p>
    </section>
  );
}
