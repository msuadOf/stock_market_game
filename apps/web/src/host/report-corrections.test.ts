import assert from "node:assert/strict";
import test from "node:test";
import { parseReportCorrectionStatus, parseReportCorrectionResponse, reportCorrectionFailureGeneration } from "./report-corrections.ts";

const entry = { source: 7, date: "2030-01-02", kind: "CashRevenue", cash_flow: "NonCash", lines: [{ account: "1001", side: "Debit", amount: "10.00" }, { account: "6001", side: "Credit", amount: "10.00" }] };
const request = { operation_id: "correct-7", company: "C-600000", supersedes: 2, entries: [entry], reason: "真实差错" };
const completed = { request, publications: [3], committed_at: { date: "2030-01-02", second_of_day: 80000 } };

test("统一更正状态严格保留待办、真实公开结果和会计元单位", { timeout: 10000 }, () => {
  assert.deepEqual(parseReportCorrectionStatus({ pending: [request], completed: {} }), { pending: [request], completed: {} });
  assert.deepEqual(parseReportCorrectionStatus({ pending: [], completed: { "correct-7": completed } }).completed["correct-7"], completed);
  for (const invalid of [{ pending: [], completed: [] }, { pending: [request, request], completed: {} }, { pending: [request], completed: { "correct-7": completed } }, { pending: [], completed: {}, schema_version: 1 }]) {
    assert.throws(() => parseReportCorrectionStatus(invalid));
  }
});

test("统一更正响应拒绝旧generation、不完整或伪成功的mutation", { timeout: 10000 }, () => {
  assert.equal(parseReportCorrectionResponse({ generation: "2", value: null }, "2", true), null);
  assert.throws(() => parseReportCorrectionResponse({ generation: "1", value: null }, "2", true), /generation/);
  assert.throws(() => parseReportCorrectionResponse({ generation: 2, value: null }, "2", true), /generation/);
  assert.throws(() => parseReportCorrectionResponse({ generation: "2", value: {} }, "2", true), /null/);
  assert.throws(() => parseReportCorrectionResponse({ generation: "2" }, "2", true));
});

test("Native可恢复更正错误必须携带规范权威generation，不猜默认scope", { timeout: 10000 }, () => {
  const failure = { code: "REPORT_CORRECTION_REJECTED", where: "session.dayEnd", message: "真实拒绝", recoverable: true };
  assert.equal(reportCorrectionFailureGeneration({ ...failure, context: { generation: "2" } }), "2");
  for (const context of [undefined, {}, { generation: 2 }, { generation: "02" }, { generation: "-1" }]) assert.throws(() => reportCorrectionFailureGeneration({ ...failure, context }), /context|generation/);
});
