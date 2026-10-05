import assert from "node:assert/strict";
import test from "node:test";
import { publicReportGold, serdeValue } from "../components/company/public-report-fixture.ts";
import { parsePublicReport } from "./public-report-normalize.ts";
import { normalizePublicReportById } from "./serde-normalize.ts";
import { normalizePublicReportAvailabilityQuery, parsePublicReportAvailability } from "./public-report-normalize.ts";

test("两种来源共用完整四表及更正原因，不以模式省略财务", () => {
  for (const source of ["SimpleGenerated", "SimulationAccounting"]) {
    const report = { ...publicReportGold(), source };
    assert.deepEqual(parsePublicReport(report), report);
    assert.deepEqual(normalizePublicReportById(serdeValue(report)), report);
    assert.deepEqual(report.financials.version_kind, { Correction: { reason: "已公布更正说明" } });
  }
});

test("来源是严格必填枚举，不补来源或接旧mode内容", () => {
  const report = { ...publicReportGold(), source: "SimpleGenerated" };
  for (const source of [null, undefined, "Unknown", 1]) assert.throws(() => parsePublicReport({ ...report, source }), TypeError);
  const missing = { ...report };
  delete (missing as Partial<typeof missing>).source;
  assert.throws(() => parsePublicReport(missing), /字段/);
  assert.throws(() => parsePublicReport({ ...report, content: { mode: "Simple", fundamentals: {} } }), /字段/);
  const noStatements = { ...report };
  delete (noStatements as Partial<typeof noStatements>).financials;
  assert.throws(() => parsePublicReport(noStatements), /字段/);
});

test("availability 只接受完整已公开报告或明确不可用原因", () => {
  const report = publicReportGold();
  const available = { status: "Available", report };
  const query = { company_id: report.company_id, period_end: report.period, kind: report.kind, scope: report.financials.scope };
  assert.deepEqual(parsePublicReportAvailability(available, query), available);
  assert.throws(() => parsePublicReportAvailability(available, { ...query, period_end: "2030-12-31" }), /与请求.*不一致/);
  for (const reason of ["BeforeOpening", "NotYetSettled", "PeriodNotRepresented", "NotYetPublished", "NotScheduled", "ScopeNotRepresented"]) {
    const unavailable = { status: "Unavailable", reason };
    assert.deepEqual(parsePublicReportAvailability(unavailable), unavailable);
  }
  for (const invalid of [
    { status: "Available", report: { ...report, source: undefined } },
    { status: "Unavailable", reason: "Unknown" },
    { status: "Unavailable", reason: "NotYetPublished", private_report: report },
    { status: "Available", report, future_facts: { net_income: "999.00" } },
    { status: "Unknown", reason: "NotYetPublished" },
  ]) assert.throws(() => parsePublicReportAvailability(invalid), TypeError);
});

test("availability 请求拒绝非月末日期和未知字段", () => {
  const query = { company_id: "C-600101", period_end: "2030-03-31", kind: "Quarter", scope: { Standalone: { entity_id: "C-600101" } } };
  assert.deepEqual(normalizePublicReportAvailabilityQuery(query), query);
  assert.throws(() => normalizePublicReportAvailabilityQuery({ ...query, period_end: "2030-03-30" }), /自然月末/);
  assert.throws(() => normalizePublicReportAvailabilityQuery({ ...query, future: true }), /字段不符合/);
});
