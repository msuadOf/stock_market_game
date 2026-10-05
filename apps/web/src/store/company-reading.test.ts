import assert from "node:assert/strict";
import test from "node:test";
import { companyReducer, installCompanyBaseline, recordCompanyPage, startCompanyQuery, recordCompanyQueryFailure } from "./company-slice.ts";
import { publicReportGold } from "../components/company/public-report-fixture.ts";

const companyId = "C-600101";
const otherCompanyId = "C-002156";

function fixture() {
  let state = companyReducer(undefined, installCompanyBaseline({ generation: 1, civilDate: "2030-01-02", revision: "1", seq: 0 }));
  for (const id of [companyId, otherCompanyId]) {
    const reports = ["7", "8"].map((reportId) => {
      const report = publicReportGold();
      return { ...report, company_id: id, id: reportId, financials: { ...report.financials, scope: { Standalone: { entity_id: id } } } };
    });
    state = companyReducer(state, recordCompanyPage({ generation: 1, companyId: id, cursor: null, reports, nextCursor: null }));
  }
  return state;
}

function readingAction(id: string, changes: Record<string, unknown>, generation = 1) {
  return { type: "company/updateCompanyReading", payload: { generation, companyId: id, changes } };
}

test("每家公司分别保存报告、报表和金额显示，局部操作不覆盖其他选择", () => {
  let state = companyReducer(fixture(), readingAction(companyId, { selectedReportId: "8", selectedStatementId: "income", exactAmountsVisible: true }));
  state = companyReducer(state, readingAction(otherCompanyId, { selectedReportId: "7", selectedStatementId: "cash-flow" }));
  state = companyReducer(state, readingAction(companyId, { selectedStatementId: "equity" }));
  assert.deepEqual(Reflect.get(state, "readingByCompany"), {
    [companyId]: { selectedReportId: "8", selectedStatementId: "equity", exactAmountsVisible: true },
    [otherCompanyId]: { selectedReportId: "7", selectedStatementId: "cash-flow", exactAmountsVisible: false },
  });
});

test("新 baseline 清空阅读状态，旧 generation 的界面操作不能写入新会话", () => {
  let state = companyReducer(fixture(), readingAction(companyId, { selectedReportId: "8", selectedStatementId: "income", exactAmountsVisible: true }));
  state = companyReducer(state, installCompanyBaseline({ generation: 2, civilDate: "2031-02-03", revision: "2", seq: 0 }));
  state = companyReducer(state, readingAction(companyId, { selectedReportId: "8" }));
  assert.deepEqual(Reflect.get(state, "readingByCompany"), {});
});

test("公开报告 loading 和 error 不清除阅读位置", () => {
  let state = companyReducer(fixture(), readingAction(companyId, { selectedReportId: "8", selectedStatementId: "income", exactAmountsVisible: true }));
  const selected = Reflect.get(state, "readingByCompany");
  state = companyReducer(state, startCompanyQuery({ generation: 1, companyId, cursor: null }));
  state = companyReducer(state, recordCompanyQueryFailure({ generation: 1, companyId, cursor: null, message: "显式查询失败" }));
  assert.deepEqual(Reflect.get(state, "readingByCompany"), selected);
});

for (const changes of [{ selectedStatementId: "unsupported" }, { exactAmountsVisible: "true" }, { selectedReportId: "unknown" }]) {
  test(`无效阅读操作显式拒绝：${JSON.stringify(changes)}`, () => {
    assert.throws(() => companyReducer(fixture(), readingAction(companyId, changes)), /阅读|报表|报告|金额/);
  });
}
