import type {
  PublicComparativeAmount,
  PublicReportFinancials,
  PublicReportIncomeColumns,
  PublicReportLine,
  PublicReportKind,
} from "../../types/engine.ts";
import type { ReportRoe, ReportRoeUnavailable, ReportRoeValue } from "../../save/schema/company/reports.ts";
import type { DayStatus } from "../../types/generated/DayStatus.ts";

export type ComparisonPresentation =
  | { readonly kind: "available"; readonly text: string }
  | { readonly kind: "unavailable"; readonly text: string };
export type ReportIndicator = { readonly label: string; readonly value: string; readonly unavailable: boolean };

export type StatementCell = string | { readonly kind: "unavailable"; readonly text: string };
export type StatementRow = { readonly subject: string; readonly amount: StatementCell; readonly comparisons?: readonly StatementCell[] };
export const COMPANY_STATEMENT_IDS = ["balance", "income", "cash-flow", "equity"] as const;
export type CompanyStatementId = typeof COMPANY_STATEMENT_IDS[number];
export type StatementSection = { readonly id: CompanyStatementId; readonly title: string; readonly columns?: readonly string[]; readonly rows: readonly StatementRow[]; readonly details?: readonly PublicReportLine[] };
export type ReportViewState =
  | { readonly kind: "idle" }
  | { readonly kind: "loading" }
  | { readonly kind: "ready" }
  | { readonly kind: "empty" }
  | { readonly kind: "error"; readonly message: string }
  | { readonly kind: "unavailable"; readonly message: string };

const ACCOUNTING_DECIMAL = /^(-?)(\d+)\.(\d{2})$/;
const REPORT_PERIOD = /^\d{4}-(?:0[1-9]|1[0-2])-(?:0[1-9]|[12]\d|3[01])$/;

function assertNever(value: never): never {
  throw new Error(`未处理的公开公司 DTO 变体：${JSON.stringify(value)}`);
}

function unitAt(tier: number): string {
  if (tier === 0) return "";
  return `${tier % 2 === 1 ? "万" : ""}${"亿".repeat(Math.floor(tier / 2))}`;
}

function trimFraction(value: string): string {
  return value.replace(/(\.\d*?[1-9])0+$|\.0+$/, "$1");
}

export function formatAccountingAmount(value: string): string {
  const match = ACCOUNTING_DECIMAL.exec(value);
  if (!match) throw new RangeError(`公开财务金额必须是两位小数十进制字符串，收到 ${value}`);
  const sign = match[1] === "-" ? "-" : "";
  let scaled = BigInt(match[2]!) * 100n + BigInt(match[3]!);
  let tier = 0;
  const unit = 10_000n;
  while (scaled >= unit * 100n) {
    scaled = (scaled + unit / 2n) / unit;
    tier += 1;
  }
  const whole = scaled / 100n;
  const fraction = scaled % 100n;
  const rendered = fraction === 0n ? whole.toString() : trimFraction(`${whole}.${fraction.toString().padStart(2, "0")}`);
  return `${sign}${rendered}${unitAt(tier)}`;
}

export function formatReportKind(kind: PublicReportKind): string {
  switch (kind) {
    case "Monthly": return "月度报告";
    case "Quarter": return "季度报告";
    case "HalfYear": return "半年度报告";
    case "Annual": return "年度报告";
    default: return assertNever(kind);
  }
}

export function formatSecondOfDay(seconds: number): string {
  const hours = Math.floor(seconds / 3_600).toString().padStart(2, "0");
  const minutes = Math.floor((seconds % 3_600) / 60).toString().padStart(2, "0");
  return `${hours}:${minutes}`;
}

export function formatReportPeriod(period: string): string {
  if (!REPORT_PERIOD.test(period)) throw new RangeError(`公开报告期间必须是 YYYY-MM-DD，收到 ${period}`);
  return period;
}

export function formatCalendarStatus(dayStatus: DayStatus): string {
  if (dayStatus === "Trading") return "交易日";
  const reason = dayStatus.Closed;
  if (reason === "Weekend") return "休市：周末";
  if ("OfficialHoliday" in reason) return "休市：交易所公告假期";
  const holiday = reason.SimulatedHoliday;
  switch (holiday) {
    case "NewYearDay": return "休市：模拟元旦假期";
    case "LabourDay": return "休市：模拟劳动节假期";
    case "NationalDay": return "休市：模拟国庆节假期";
    case "SpringFestival": return "休市：模拟春节假期";
    case "Qingming": return "休市：模拟清明节假期";
    case "DragonBoat": return "休市：模拟端午节假期";
    case "MidAutumn": return "休市：模拟中秋节假期";
    default: return assertNever(holiday);
  }
}

export function formatComparison(comparison: PublicComparativeAmount): ComparisonPresentation {
  if ("Available" in comparison) return { kind: "available", text: formatAccountingAmount(comparison.Available.amount) };
  switch (comparison.Unavailable.reason) {
    case "NoPriorYearHistory": return { kind: "unavailable", text: "暂无上年同期：无上年历史" };
    default: return assertNever(comparison.Unavailable.reason);
  }
}

function reportRoeUnavailableReason(reason: ReportRoeUnavailable): string {
  switch (reason) {
    case "NonPositiveAverageEquity": return "平均归母净资产非正"
    case "MissingNonRecurringIncomeFacts": return "缺少扣非归母利润事实"
    case "IncompleteEquityEventHistory": return "权益变动历史不完整"
    case "SameControlCombination": return "同一控制下企业合并特殊处理尚不支持"
    case "ComparativePeriodSpecialTreatment": return "比较期间特殊处理尚不支持"
    case "UnclassifiedEquityEvent": return "存在未分类权益变动"
    case "ConsolidatedAttributionFactsUnavailable": return "合并归母数据不可用"
    default: return assertNever(reason)
  }
}

function reportRoeValue(value: ReportRoeValue): { readonly text: string; readonly unavailable: boolean } {
  if ("Unavailable" in value) return { text: `不可用：${reportRoeUnavailableReason(value.Unavailable.reason)}`, unavailable: true }
  const numerator = BigInt(value.Available.numerator)
  const denominator = BigInt(value.Available.denominator)
  const negative = numerator < 0n
  const magnitude = negative ? -numerator : numerator
  const scaled = magnitude * 10_000n
  let hundredths = scaled / denominator
  const remainder = scaled % denominator
  const doubledRemainder = remainder * 2n
  if (doubledRemainder > denominator || (doubledRemainder === denominator && hundredths % 2n !== 0n)) hundredths += 1n
  return { text: `${negative && hundredths !== 0n ? "-" : ""}${hundredths / 100n}.${(hundredths % 100n).toString().padStart(2, "0")}%`, unavailable: false }
}

export function reportRoeIndicators(roe: ReportRoe): readonly ReportIndicator[] {
  const basis = typeof roe.basis === "string"
    ? "归属于普通股股东"
    : `不可用：${reportRoeUnavailableReason(roe.basis.Unsupported.reason)}`
  const ordinary = reportRoeValue(roe.ordinary_roe)
  const adjusted = reportRoeValue(roe.adjusted_roe)
  return [
    { label: "ROE 口径", value: basis, unavailable: typeof roe.basis !== "string" },
    { label: "加权平均净资产收益率（归母净利润）", value: ordinary.text, unavailable: ordinary.unavailable },
    { label: "加权平均净资产收益率（扣非归母净利润）", value: adjusted.text, unavailable: adjusted.unavailable },
  ]
}

function unavailable(text: string): StatementCell {
  return { kind: "unavailable", text };
}

function lookup(lines: readonly PublicReportLine[], subject: string): StatementCell {
  const line = lines.find((entry) => entry.subject === subject);
  return line === undefined ? unavailable("该比较列未列报此科目") : line.amount;
}

function incomeRows(columns: PublicReportIncomeColumns): PublicReportLine[] {
  return [
    ...columns.operating, { subject: "经营类别小计", amount: columns.operating_subtotal },
    ...columns.investing, { subject: "投资类别小计", amount: columns.investing_subtotal },
    ...columns.financing, { subject: "筹资类别小计", amount: columns.financing_subtotal },
    ...columns.discontinued, { subject: "终止经营小计", amount: columns.discontinued_subtotal },
    { subject: "所得税费用", amount: columns.income_tax }, { subject: "净利润", amount: columns.net_income },
  ];
}

export function reportStatementRows(financials: PublicReportFinancials): readonly StatementSection[] {
  const consolidated = "Consolidated" in financials.scope;
  const balance = financials.balance_sheet;
  const parentEquitySubject = consolidated ? "归属于母公司所有者权益" : "所有者权益（单体口径）";
  const prior = "Available" in balance.prior_year_end ? balance.prior_year_end.Available : null;
  const priorBalanceRows = prior === null ? null : [
    ...prior.lines, { subject: "资产总计", amount: prior.total_assets },
    { subject: "负债合计", amount: prior.total_liabilities },
    { subject: "所有者权益合计", amount: prior.total_equity },
    { subject: parentEquitySubject, amount: prior.equity_to_parent },
    { subject: "负债和所有者权益总计", amount: prior.liabilities_and_equity },
  ];
  const balanceRows = [
    ...balance.asset_lines, { subject: "资产总计", amount: balance.total_assets },
    ...balance.liability_lines, { subject: "负债合计", amount: balance.total_liabilities },
    ...balance.equity_lines, { subject: "所有者权益合计", amount: balance.total_equity },
    { subject: parentEquitySubject, amount: balance.equity_to_parent },
    { subject: "负债和所有者权益总计", amount: balance.liabilities_and_equity },
  ].map((row) => ({ ...row, comparisons: [priorBalanceRows === null
    ? unavailable("暂无上年年末：无上年历史") : lookup(priorBalanceRows, row.subject)] }));
  const income = financials.income;
  const reportPeriod = incomeRows(income.report_period);
  const cumulative = incomeRows(income.cumulative);
  const priorIncomeRows = "Available" in income.prior_year ? incomeRows(income.prior_year.Available) : null;
  const subjects = [...new Set([...reportPeriod, ...cumulative, ...(priorIncomeRows === null ? [] : priorIncomeRows)].map((line) => line.subject))];
  const incomeStatementRows: StatementRow[] = subjects.map((subject) => ({
    subject, amount: lookup(reportPeriod, subject), comparisons: [lookup(cumulative, subject),
      priorIncomeRows === null ? unavailable("暂无上年同期：无上年历史") : lookup(priorIncomeRows, subject)],
  }));
  const incomeDetails: PublicReportLine[] = [];
  if (income.net_income_to_parent !== null) incomeDetails.push({ subject: "已披露合并拆分：归母净利润", amount: income.net_income_to_parent });
  if (income.minority_net_income !== null) incomeDetails.push({ subject: "已披露合并拆分：少数股东损益", amount: income.minority_net_income });
  const cash = financials.cash_flow;
  const equity = financials.equity;
  const equityRows: StatementRow[] = [
    { subject: consolidated ? "期初归母权益" : "期初所有者权益", amount: equity.opening_parent }, { subject: consolidated ? "已披露归母净利润" : "本报告窗口净利润", amount: equity.net_income },
    { subject: "其他综合收益", amount: equity.other_comprehensive }, { subject: "所有者投入", amount: equity.capital_contributions },
    { subject: "对所有者分配", amount: equity.distributions }, { subject: consolidated ? "期末归母权益" : "期末所有者权益", amount: equity.closing_parent },
  ];
  if (equity.opening_minority !== null) equityRows.push({ subject: "期初少数股东权益", amount: equity.opening_minority });
  if (equity.minority_net_income !== null) equityRows.push({ subject: "本报告窗口少数股东损益", amount: equity.minority_net_income });
  if (equity.closing_minority !== null) equityRows.push({ subject: "期末少数股东权益", amount: equity.closing_minority });
  return [
    { id: "balance", title: "资产负债表", columns: ["期末", "上年年末"], rows: balanceRows },
    { id: "income", title: "利润表", columns: ["报告窗口", "年初至今累计", "上年同期（报告窗口）"], rows: incomeStatementRows, details: incomeDetails },
    { id: "cash-flow", title: "现金流量表", rows: [
      { subject: "经营活动现金流量", amount: cash.operating }, { subject: "投资活动现金流量", amount: cash.investing },
      { subject: "筹资活动现金流量", amount: cash.financing }, { subject: "现金净增加额", amount: cash.net_change },
      { subject: "期初现金", amount: cash.opening_cash }, { subject: "期末现金", amount: cash.closing_cash }, ...cash.indirect,
    ] },
    { id: "equity", title: "所有者权益变动表", rows: equityRows },
  ];
}

export function reportViewState(page: { readonly kind: string; readonly message?: string } | undefined): ReportViewState {
  if (page === undefined) return { kind: "idle" };
  switch (page.kind) {
    case "loading": return { kind: "loading" };
    case "ready": return { kind: "ready" };
    case "empty": return { kind: "empty" };
    case "error": return { kind: "error", message: page.message ?? "公开报告查询失败" };
    case "unavailable": return { kind: "unavailable", message: page.message ?? "当前宿主不支持公开公司报告查询" };
    default: throw new Error(`未知公开报告页面状态：${page.kind}`);
  }
}

export function selectVisibleReportId(selectedReportId: string | null, reportIds: readonly string[]): string | null {
  if (selectedReportId !== null && reportIds.includes(selectedReportId)) return selectedReportId;
  return reportIds[0] ?? null;
}
