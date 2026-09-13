import type {
  PublicComparativeAmount,
  PublicReportAccountingSummary,
  PublicReportKind,
} from "../../types/engine.ts";
import type { DayStatus } from "../../types/generated/DayStatus.ts";

export type ComparisonPresentation =
  | { readonly kind: "available"; readonly text: string }
  | { readonly kind: "unavailable"; readonly text: string };

export type StatementRow = { readonly subject: string; readonly amount: string };
export type StatementSection = { readonly id: string; readonly title: string; readonly rows: readonly StatementRow[] };
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

export function reportStatementRows(accounting: PublicReportAccountingSummary): readonly StatementSection[] {
  return [
    { id: "balance", title: "资产负债表", rows: [{ subject: "资产总计", amount: accounting.total_assets }, { subject: "负债合计", amount: accounting.total_liabilities }, { subject: "所有者权益合计", amount: accounting.total_equity }, { subject: "期末现金", amount: accounting.closing_cash }] },
    { id: "income", title: "利润表", rows: [{ subject: "本期净利润", amount: accounting.quarter_net_income }, { subject: "累计净利润", amount: accounting.net_income }, { subject: "所得税费用", amount: accounting.income_tax }] },
    { id: "cash-flow", title: "现金流量表", rows: [{ subject: "经营活动现金流量", amount: accounting.operating_cash_flow }, { subject: "投资活动现金流量", amount: accounting.investing_cash_flow }, { subject: "筹资活动现金流量", amount: accounting.financing_cash_flow }, { subject: "现金净增加额", amount: accounting.net_cash_change }] },
    { id: "equity", title: "所有者权益变动表", rows: [{ subject: "期末所有者权益", amount: accounting.total_equity }, { subject: "累计净利润", amount: accounting.net_income }, { subject: "本期净利润", amount: accounting.quarter_net_income }] },
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
