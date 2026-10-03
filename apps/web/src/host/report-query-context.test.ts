import assert from "node:assert/strict";
import test from "node:test";
import { publicReportGold } from "../components/company/public-report-fixture.ts";
import { ReportQueryContext } from "./report-query-context.ts";

const options = { concurrency: true, timeout: 10000 };

test("ReportQueryContext 拒绝冲突页且不登记有效前缀，opaque ID 保持精确归属", options, () => {
  const context = new ReportQueryContext();
  const report = { ...publicReportGold(), id: "9007199254740993" };
  const epoch = context.captureEpoch();
  assert.throws(() => context.acceptPage(report.company_id, epoch, { reports: [report, { ...report, id: "123", company_id: "other" }], next_cursor: null }), /不匹配/);
  assert.throws(() => context.companyFor(report.id), /归属未知/);
  context.acceptPage(report.company_id, epoch, { reports: [report], next_cursor: null });
  assert.equal(context.companyFor(report.id), report.company_id);
  assert.throws(() => context.acceptPage("other", epoch, { reports: [{ ...report, company_id: "other" }], next_cursor: null }), /不匹配/);
  assert.equal(context.companyFor(report.id), report.company_id);
});

test("ReportQueryContext invalidate 清归属并拒绝晚到页和 by-ID，允许新时间线重用 ID", options, () => {
  const context = new ReportQueryContext();
  const report = publicReportGold();
  const oldEpoch = context.captureEpoch();
  context.acceptPage(report.company_id, oldEpoch, { reports: [report], next_cursor: null });
  context.invalidate();
  assert.equal(context.captureEpoch(), oldEpoch + 1n);
  assert.throws(() => context.companyFor(report.id), /归属未知/);
  assert.throws(() => context.acceptPage(report.company_id, oldEpoch, { reports: [report], next_cursor: null }), /请重新查询/);
  assert.throws(() => context.validateReport(oldEpoch, report.company_id, report.id, report), /请先查询新时间线/);
  const next = { ...report, company_id: "replacement" };
  context.acceptPage(next.company_id, context.captureEpoch(), { reports: [next], next_cursor: null });
  assert.equal(context.companyFor(report.id), next.company_id);
  assert.throws(() => context.validateReport(context.captureEpoch(), next.company_id, report.id, report), /不匹配/);
});
