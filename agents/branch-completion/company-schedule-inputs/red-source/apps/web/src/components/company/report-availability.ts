import type { PublicReportUnavailableReason } from "../../types/engine.ts";

export function isIsoMonthEnd(value: string): boolean {
  if (!/^\d{4}-(?:0[1-9]|1[0-2])-(?:0[1-9]|[12]\d|3[01])$/.test(value)) return false;
  const date = new Date(`${value}T00:00:00Z`);
  return Number.isFinite(date.getTime()) && date.toISOString().slice(0, 10) === value
    && new Date(Date.UTC(date.getUTCFullYear(), date.getUTCMonth() + 1, 0)).toISOString().slice(0, 10) === value;
}

export function reportAvailabilityReason(reason: PublicReportUnavailableReason): string {
  switch (reason) {
    case "BeforeOpening": return "该公司尚未开业，不能形成此期间报告。";
    case "NotYetSettled": return "此报告期间尚未完成结算。";
    case "PeriodNotRepresented": return "当前未表示此报告期间。";
    case "NotYetPublished": return "报告期间已结算，但报告尚未公开。";
    case "NotScheduled": return "该类型报告不在此期间的披露安排内。";
    case "ScopeNotRepresented": return "该 scope 尚无可公开报告。";
    default: throw new Error(`未处理的公开报告可用性原因：${JSON.stringify(reason)}`);
  }
}
