import assert from "node:assert/strict";
import { test } from "node:test";
import { isIsoMonthEnd, reportAvailabilityReason } from "./report-availability.ts";
import type { PublicReportUnavailableReason } from "../../types/generated/PublicReportUnavailableReason.ts";

test("六种 Rust 报告不可用原因各有准确解释", { timeout: 10000 }, () => {
  const cases = {
    BeforeOpening: /尚未开业/,
    NotYetSettled: /尚未完成结算/,
    PeriodNotRepresented: /未表示此报告期间/,
    NotYetPublished: /已结算.*尚未公开/,
    NotScheduled: /不在.*披露安排/,
    ScopeNotRepresented: /scope.*无可公开报告/,
  } satisfies Record<PublicReportUnavailableReason, RegExp>;
  for (const [reason, message] of Object.entries(cases)) assert.match(reportAvailabilityReason(reason as PublicReportUnavailableReason), message);
  assert.throws(() => reportAvailabilityReason("FutureReason" as PublicReportUnavailableReason), /未处理.*FutureReason/);
});
test("只认可真实自然月末，正确区分闰年与世纪年", { timeout: 10000 }, () => {
  for (const value of ["1900-02-28", "2000-02-29", "2024-02-29", "2100-02-28", "2030-04-30", "2030-12-31"]) assert.equal(isIsoMonthEnd(value), true, value);
  for (const value of ["1900-02-29", "2100-02-29", "2023-02-29", "2024-02-28", "2030-04-31", "2030-01-30", "2030-00-31", "2030-13-31", "2030-01-00", "2030-1-31", " 2030-01-31", "2030-01-31T00:00:00Z", ""]) assert.equal(isIsoMonthEnd(value), false, value);
});
test("期末日期与 Rust CivilDate 算法年份窗一致", { timeout: 10000 }, () => {
  assert.equal(isIsoMonthEnd("1900-01-31"), true); assert.equal(isIsoMonthEnd("2199-12-31"), true);
  for (const value of ["1899-12-31", "2200-01-31", "0000-01-31", "9999-12-31"]) assert.equal(isIsoMonthEnd(value), false, value);
});
