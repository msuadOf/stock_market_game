import assert from "node:assert/strict";
import test from "node:test";
import { isIsoMonthEnd, reportAvailabilityReason } from "./report-availability.ts";

test("报告期间输入只接受有效自然月末", { timeout: 10000 }, () => {
  for (const date of ["2030-01-31", "2030-02-28", "2032-02-29", "2030-04-30"]) assert.equal(isIsoMonthEnd(date), true);
  for (const date of ["2030-01-30", "2030-02-29", "2030-13-31", "2030-04-31", "nonsense"]) assert.equal(isIsoMonthEnd(date), false);
});

test("六种不可用原因都有明确用户文案", { timeout: 10000 }, () => {
  for (const reason of ["BeforeOpening", "NotYetSettled", "PeriodNotRepresented", "NotYetPublished", "NotScheduled", "ScopeNotRepresented"] as const) {
    assert.ok(reportAvailabilityReason(reason).length > 0);
  }
});
