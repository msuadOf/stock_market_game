import assert from "node:assert/strict";
import test from "node:test";
import { prepareCatalogResolver } from "./catalog-resolver.ts";
import { CompanyQueryCoordinator } from "./company-query-coordinator.ts";
import { companyReducer } from "../store/company-slice.ts";
import { publicReportGold } from "../components/company/public-report-fixture.ts";

const identity = (id: number, code = "C-600101") => prepareCatalogResolver([{ id: 0, code: "600101" }], [{ id, code, name: "真实公司", listed_stock: 0 }]);
const base = (id: number, code = "C-600101") => ({ civilDate: "2030-01-02", revision: "0", seq: 0, catalogResolver: identity(id, code) });
const report = (id: number) => ({ ...publicReportGold(), company_id: id, financials: { ...publicReportGold().financials, scope: { Standalone: { entity_id: id } } } });
const capabilities = { deliveryModes: [], targetUiHz: 60, sharedMemory: false, reconnect: false, publicCompanyReports: true, npcDecisionDiagnostics: false };

test("实际 UI C-code 查询使用本代数值 id，结果仍落 C-code 缓存，scope错公司显式错误", async () => {
  let state = companyReducer(undefined, { type: "init" });
  const calls: number[] = [];
  const coordinator = new CompanyQueryCoordinator({ capabilities, queryPublicReports: async query => { calls.push(query.company_id); return { reports: [report(query.company_id)], next_cursor: null }; } }, action => { state = companyReducer(state, action); });
  coordinator.installBaseline(base(0));
  await coordinator.query({ companyId: "C-600101", cursor: null });
  assert.deepEqual(calls, [0]);
  assert.equal(state.companies["C-600101"].pages.root.kind, "ready");
  coordinator.installBaseline(base(7));
  await coordinator.query({ companyId: "C-600101", cursor: null });
  assert.deepEqual(calls, [0, 7]);
});

test("旧 generation resolve 与 reject 不能污染换代后的同 code", async () => {
  for (const rejectOld of [false, true]) {
    let resolve!: (value: unknown) => void;
    let reject!: (value: Error) => void;
    const pending = new Promise<unknown>((yes, no) => { resolve = yes; reject = no; });
    let state = companyReducer(undefined, { type: "init" });
    const coordinator = new CompanyQueryCoordinator({ capabilities, queryPublicReports: async () => pending as never }, action => { state = companyReducer(state, action); });
    coordinator.installBaseline(base(0));
    const request = coordinator.query({ companyId: "C-600101", cursor: null });
    coordinator.installBaseline(base(7));
    if (rejectOld) reject(new Error("旧请求失败")); else resolve({ reports: [report(0)], next_cursor: null });
    await request;
    assert.deepEqual(Object.keys(state.companies), []);
  }
});

test("含NUL合法公司 code 的 page/by-id 不同tuple不能碰撞", async () => {
  const resolver = prepareCatalogResolver([], [
    { id: 0, code: "A\u0000report", name: "公司一", listed_stock: null },
    { id: 1, code: "A", name: "公司二", listed_stock: null },
  ]);
  let pageCalls = 0;
  let reportCalls = 0;
  let pageResolve!: (value: unknown) => void;
  let reportResolve!: (value: unknown) => void;
  const coordinator = new CompanyQueryCoordinator({ capabilities,
    queryPublicReports: async () => { pageCalls++; return new Promise(resolve => { pageResolve = resolve; }) as never; },
    publicReportById: async () => { reportCalls++; return new Promise(resolve => { reportResolve = resolve; }) as never; },
  }, () => {});
  coordinator.installBaseline({ ...base(0), catalogResolver: resolver });
  const page = coordinator.query({ companyId: "A\u0000report", cursor: "7" });
  const byId = coordinator.queryReportById({ companyId: "A", reportId: "7" });
  assert.equal(pageCalls, 1);
  assert.equal(reportCalls, 1);
  pageResolve({ reports: [report(0)], next_cursor: null });
  reportResolve(report(1));
  await Promise.all([page, byId]);
});
