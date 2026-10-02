import type { PublicReportNoteItem, PublicReportSummary } from "../../types/engine.ts";
import { formatComparison, formatSecondOfDay } from "./company-presentation.ts";

interface ReportNotesProps {
  readonly report: PublicReportSummary;
}

function NoteDetails({ items, title }: { readonly items: readonly PublicReportNoteItem[]; readonly title: string }) {
  return <div className="company-table-wrap"><table className="company-table" aria-label={title}>
    <caption>{title}（元，精确值）</caption>
    <thead><tr>{["科目", "列报归属", "期初净借方", "报告窗口净借方变动", "年初至今净借方变动", "期末净借方"].map((heading) => <th scope="col" key={heading}>{heading}</th>)}</tr></thead>
    <tbody>{items.map((item) => <tr key={item.code}><th scope="row">{item.code} · {item.name}</th>
      <td>{"BalanceSheet" in item.target ? `资产负债表：${item.target.BalanceSheet}` : `利润表：${item.target.Income}`}</td>
      {[item.opening, item.movement, item.ytd_movement, item.closing].map((amount, index) => <td key={index}>{amount}</td>)}
    </tr>)}</tbody>
  </table>{items.length === 0 && <p>本报告未列报该类附注明细。</p>}</div>;
}

export function ReportNotes({ report }: ReportNotesProps) {
  const comparison = formatComparison(report.accounting.prior_year_net_income);
  const financials = report.financials;
  const scope = "Standalone" in financials.scope ? `单体 · ${financials.scope.Standalone.entity_id}` : `合并 · 根公司 ${financials.scope.Consolidated.root_entity_id}`;
  return (
    <section className="company-notes" aria-labelledby="company-notes-title">
      <h4 id="company-notes-title">报表附注与口径</h4>
      <dl>
        <div><dt>批准时刻</dt><dd>{report.approved_date} {formatSecondOfDay(report.approved_second_of_day)}</dd></div>
        <div><dt>发布时刻</dt><dd>{report.published_date} {formatSecondOfDay(report.published_second_of_day)}</dd></div>
        <div><dt>报告范围</dt><dd>{scope}</dd></div>
        <div><dt>报告窗口</dt><dd>{financials.window_start} 至 {financials.window_end}</dd></div>
        <div><dt>上年同期净利润（报告窗口）</dt><dd className={comparison.kind}>{comparison.text}</dd></div>
      </dl>
      {report.supersedes !== null && <p className="company-correction">本版更正了公开报告 #{report.supersedes}；历史版本仍可查询。</p>}
      {financials.version_kind !== "Original" && <p className="company-correction">更正原因：{financials.version_kind.Correction.reason}</p>}
      {financials.version_supersedes !== null && <p>替代本报告期间的会计版本 {financials.version_supersedes}（与公开报告编号分开）。</p>}
      <NoteDetails items={financials.notes.items} title="已披露科目明细" />
      {"Consolidated" in financials.scope && <NoteDetails items={financials.notes.consolidation_split_items} title="合并权益拆分附注" />}
      <p>附注金额为净借方口径，负数不等于股价下跌。本界面只显示已公布报告，不显示总账、未披露经营事实或 NPC 私有信息。</p>
      <p>游戏简化：未实现其他综合收益或股东分配；无对应科目的类别不补造明细，未披露的比较项不填零。报表按现有会计模型列报，不构成真实会计准则合规声明。</p>
    </section>
  );
}
