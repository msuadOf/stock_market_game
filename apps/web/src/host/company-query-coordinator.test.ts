import assert from "node:assert/strict";
import test from "node:test";
import type { EngineHost } from "./engine-host.ts";
import { CompanyQueryCoordinator } from "./company-query-coordinator.ts";

type QueryHost = Pick<EngineHost, "capabilities" | "publicReportById" | "queryPublicReports">;

function deferred<T>() {
  let resolve: (value: T) => void = () => {};
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

const report = (company: string, id: string) => ({
  id, company_id: company, period: "2030-03-31", kind: "Quarter" as const, version_sequence: "1", supersedes: null,
  approved_date: "2030-04-01", approved_second_of_day: 0, published_date: "2030-04-02", published_second_of_day: 0,
  accounting: { total_assets: "9007199254740993.00", total_liabilities: "0.00", total_equity: "1.00", closing_cash: "1.00", quarter_net_income: "1.00", net_income: "1.00", income_tax: "0.00", operating_cash_flow: "1.00", investing_cash_flow: "0.00", financing_cash_flow: "0.00", net_cash_change: "1.00", prior_year_net_income: { Unavailable: { reason: "NoPriorYearHistory" as const } } },
});

test("rejects delayed prior-session page and by-id responses while accepting current session data", async () => {
  const late = deferred<{ reports: ReturnType<typeof report>[]; next_cursor: string | null }>();
  const lateById = deferred<ReturnType<typeof report>>();
  const updates: unknown[] = [];
  const host = {
  capabilities: { deliveryModes: [], targetUiHz: 60, sharedMemory: false, reconnect: false, publicCompanyReports: true, npcDecisionDiagnostics: false },
    queryPublicReports(query) { return query.company_id === "old" ? late.promise : Promise.resolve({ reports: [report("new", query.cursor === null ? "7" : "8")], next_cursor: query.cursor === null ? "7" : null }); },
    publicReportById(id) { return id === "old-report" ? lateById.promise : Promise.resolve(report("new", id)); },
  } satisfies QueryHost;
  const coordinator = new CompanyQueryCoordinator(host, (action) => updates.push(action));
  coordinator.installBaseline({ civilDate: "2030-01-01", revision: "1", seq: 0 });
  const oldRequest = coordinator.query({ companyId: "old", cursor: null });
  const oldReportRequest = coordinator.queryReportById({ companyId: "old", reportId: "old-report" });
  coordinator.installBaseline({ civilDate: "2031-01-01", revision: "2", seq: 0 });
  await coordinator.query({ companyId: "new", cursor: null });
  await coordinator.query({ companyId: "new", cursor: "7" });
  late.resolve({ reports: [report("old", "7")], next_cursor: null });
  lateById.resolve(report("old", "old-report"));
  await oldRequest;
  await oldReportRequest;
  assert.equal(updates.some((action) => JSON.stringify(action).includes('"companyId":"old"') && JSON.stringify(action).includes('"reports"')), false);
  assert.equal(updates.some((action) => JSON.stringify(action).includes('"companyId":"old"') && JSON.stringify(action).includes('"reportId":"old-report"') && JSON.stringify(action).includes('recordCompanyReport')), false);
  assert.equal(updates.filter((action) => JSON.stringify(action).includes('"companyId":"new"') && JSON.stringify(action).includes('"reports"')).length, 2);
});

test("reports malformed public DTOs as explicit query errors and unavailable capability separately", async () => {
  const updates: unknown[] = [];
  const malformed = new CompanyQueryCoordinator({
  capabilities: { deliveryModes: [], targetUiHz: 60, sharedMemory: false, reconnect: false, publicCompanyReports: true, npcDecisionDiagnostics: false },
    queryPublicReports() {
      return Promise.resolve(JSON.parse('{"reports":[{"id":"7"}],"next_cursor":null}'));
    },
  } satisfies QueryHost, (action) => updates.push(action));
  malformed.installBaseline({ civilDate: "2030-01-01", revision: "1", seq: 0 });
  await malformed.query({ companyId: "600001", cursor: null });
  assert.match(JSON.stringify(updates), /recordCompanyQueryFailure/);
  assert.match(JSON.stringify(updates), /公共 DTO 契约/);

  const unsupported = new CompanyQueryCoordinator({
  capabilities: { deliveryModes: [], targetUiHz: 60, sharedMemory: false, reconnect: false, publicCompanyReports: false, npcDecisionDiagnostics: false },
  }, (action) => updates.push(action));
  unsupported.installBaseline({ civilDate: "2030-01-01", revision: "1", seq: 0 });
  await unsupported.query({ companyId: "600001", cursor: null });
  assert.match(JSON.stringify(updates), /unavailable/);
});

test("rejects public reports that violate shared opaque ID, decimal, or second-of-day contracts", async () => {
  const invalidReports = [
    { ...report("600001", "01") },
    { ...report("600001", "1"), accounting: { ...report("600001", "1").accounting, total_assets: "1" } },
    { ...report("600001", "1"), approved_second_of_day: 86_401 },
  ];
  for (const invalidReport of invalidReports) {
    const updates: unknown[] = [];
    const coordinator = new CompanyQueryCoordinator({
  capabilities: { deliveryModes: [], targetUiHz: 60, sharedMemory: false, reconnect: false, publicCompanyReports: true, npcDecisionDiagnostics: false },
      queryPublicReports: () => Promise.resolve({ reports: [invalidReport], next_cursor: null }),
    } satisfies QueryHost, (action) => updates.push(action));
    coordinator.installBaseline({ civilDate: "2030-01-01", revision: "1", seq: 0 });
    await coordinator.query({ companyId: "600001", cursor: null });
    assert.match(JSON.stringify(updates), /recordCompanyQueryFailure/);
  }
});

test("refreshes an already cached company after a disclosure without a trade", async () => {
  const updates: unknown[] = [];
  const host = {
  capabilities: { deliveryModes: [], targetUiHz: 60, sharedMemory: false, reconnect: false, publicCompanyReports: true, npcDecisionDiagnostics: false },
    queryPublicReports(query) { return Promise.resolve({ reports: [report(query.company_id, "8")], next_cursor: null }); },
  } satisfies QueryHost;
  const coordinator = new CompanyQueryCoordinator(host, (action) => updates.push(action));
  coordinator.installBaseline({ civilDate: "2030-01-01", revision: "1", seq: 0 });
  await coordinator.query({ companyId: "600001", cursor: null });
  coordinator.acceptEvents([{ CompanyDisclosurePublished: {
    seq: 1, publication_id: 9, company: "600001", published_at: { date: "2030-01-02", second_of_day: 64_800 }, kind: "Announcement",
  } }, { CivilDateAdvanced: {
    seq: 2, settled_date: "2030-01-01", next_date: "2030-01-02", next_status: { Closed: "Weekend" },
  } }], { fromSeq: 1, toSeq: 2 });
  await new Promise((resolve) => setImmediate(resolve));
  assert.match(JSON.stringify(updates), /recordCivilDateAdvanced/);
  assert.equal(JSON.stringify(updates).match(/recordCompanyPage/g)?.length, 2);
});

test("does not refresh a company absent from the cache", async () => {
  const updates: unknown[] = [];
  let queries = 0;
  const host = {
  capabilities: { deliveryModes: [], targetUiHz: 60, sharedMemory: false, reconnect: false, publicCompanyReports: true, npcDecisionDiagnostics: false },
    queryPublicReports() { queries += 1; return Promise.resolve({ reports: [], next_cursor: null }); },
  } satisfies QueryHost;
  const coordinator = new CompanyQueryCoordinator(host, (action) => updates.push(action));
  coordinator.installBaseline({ civilDate: "2030-01-01", revision: "1", seq: 9 });
  coordinator.acceptEvents([{ CompanyDisclosurePublished: {
    seq: 10, publication_id: 2, company: "uncached", published_at: { date: "2030-01-01", second_of_day: 2 }, kind: "Announcement",
  } }], { fromSeq: 10, toSeq: 10 });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(queries, 0);
  assert.equal(JSON.stringify(updates).includes('"seq":10'), true);
});

test("rejects a sequence-coverage gap so the host can deliver a fresh baseline", async () => {
  const calls: string[] = [];
  const host = {
  capabilities: { deliveryModes: [], targetUiHz: 60, sharedMemory: false, reconnect: false, publicCompanyReports: true, npcDecisionDiagnostics: false },
    queryPublicReports(query) {
      calls.push(query.company_id);
      return Promise.resolve({ reports: [report(query.company_id, "7")], next_cursor: null });
    },
  } satisfies QueryHost;
  const coordinator = new CompanyQueryCoordinator(host, () => {});
  coordinator.installBaseline({ civilDate: "2030-01-01", revision: "1", seq: 1 });
  await coordinator.query({ companyId: "600001", cursor: null });
  await coordinator.query({ companyId: "000001", cursor: null });
  assert.throws(() => coordinator.acceptEvents([{ CivilDateAdvanced: {
    seq: 4, settled_date: "2030-01-01", next_date: "2030-01-02", next_status: "Trading",
  } }], { fromSeq: 4, toSeq: 4 }), /需要宿主重同步/);
  assert.deepEqual(calls, ["600001", "000001"]);
});

test("accepts a compressed batch whose coverage is continuous", () => {
  const coordinator = new CompanyQueryCoordinator({
  capabilities: { deliveryModes: [], targetUiHz: 60, sharedMemory: false, reconnect: false, publicCompanyReports: false, npcDecisionDiagnostics: false },
  }, () => {});
  coordinator.installBaseline({ civilDate: "2030-01-01", revision: "1", seq: 1 });
  coordinator.acceptEvents([{ CivilDateAdvanced: {
    seq: 4, settled_date: "2030-01-01", next_date: "2030-01-02", next_status: "Trading",
  } }], { fromSeq: 2, toSeq: 4 });
});

test("rejects a reversed empty coverage range before it can desynchronize the coordinator", () => {
  const coordinator = new CompanyQueryCoordinator({
  capabilities: { deliveryModes: [], targetUiHz: 60, sharedMemory: false, reconnect: false, publicCompanyReports: false, npcDecisionDiagnostics: false },
  } satisfies QueryHost, () => {});
  coordinator.installBaseline({ civilDate: "2030-01-01", revision: "1", seq: 1 });
  assert.throws(() => coordinator.acceptEvents([], { fromSeq: 2, toSeq: 1 }), /覆盖区间无效/);
  coordinator.acceptEvents([{ CivilDateAdvanced: {
    seq: 2, settled_date: "2030-01-01", next_date: "2030-01-02", next_status: "Trading",
  } }], { fromSeq: 2, toSeq: 2 });
});

test("rejects out-of-order public events even when their outer coverage is continuous", () => {
  const coordinator = new CompanyQueryCoordinator({
  capabilities: { deliveryModes: [], targetUiHz: 60, sharedMemory: false, reconnect: false, publicCompanyReports: false, npcDecisionDiagnostics: false },
  } satisfies QueryHost, () => {});
  coordinator.installBaseline({ civilDate: "2030-01-01", revision: "1", seq: 0 });
  assert.throws(() => coordinator.acceptEvents([{ CivilDateAdvanced: {
    seq: 2, settled_date: "2030-01-01", next_date: "2030-01-02", next_status: "Trading",
  } }, { CompanyDisclosurePublished: {
    seq: 1, publication_id: 2, company: "600001", published_at: { date: "2030-01-01", second_of_day: 2 }, kind: "Announcement",
  } }], { fromSeq: 1, toSeq: 2 }), /覆盖区间不一致/);
});
