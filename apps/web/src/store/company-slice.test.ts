import assert from "node:assert/strict";
import test from "node:test";
import {
  companyReducer,
  installCompanyBaseline,
  recordCompanyPage,
  recordCompanyQueryFailure,
  invalidateCompanyForDisclosure,
  recordCivilDateAdvanced,
  startCompanyQuery,
} from "./company-slice.ts";

const report = (companyId: string, id: string, version = "1") => ({
  id,
  company_id: companyId,
  period: "2030-03-31",
  kind: "Quarter" as const,
  version_sequence: version,
  supersedes: null,
  approved_date: "2030-04-01",
  approved_second_of_day: 0,
  published_date: "2030-04-02",
  published_second_of_day: 0,
  accounting: {
    total_assets: "9007199254740993.01", total_liabilities: "1", total_equity: "2",
    closing_cash: "3", quarter_net_income: "4", net_income: "5", income_tax: "6",
    operating_cash_flow: "7", investing_cash_flow: "8", financing_cash_flow: "9",
    net_cash_change: "10", prior_year_net_income: { Unavailable: { reason: "NoPriorYearHistory" as const } },
  },
});

test("baseline atomically replaces old session public cache and selection", () => {
  const first = companyReducer(undefined, installCompanyBaseline({ generation: 1, civilDate: "2030-01-01", revision: "1", seq: 0 }));
  const loaded = companyReducer(first, recordCompanyPage({ generation: 1, companyId: "600001", cursor: null, reports: [report("600001", "7")], nextCursor: null }));
  const next = companyReducer(loaded, installCompanyBaseline({ generation: 2, civilDate: "2031-01-01", revision: "2", seq: 0 }));
  assert.deepEqual(next.companies, {});
  assert.equal(next.generation, 2);
  assert.equal(next.revision, "2");
});

test("keeps report versions immutable and isolates same report id by company", () => {
  let state = companyReducer(undefined, installCompanyBaseline({ generation: 1, civilDate: "2030-01-01", revision: "1", seq: 0 }));
  const original = report("600001", "7", "1");
  state = companyReducer(state, recordCompanyPage({ generation: 1, companyId: "600001", cursor: null, reports: [original], nextCursor: "7" }));
  state = companyReducer(state, recordCompanyPage({ generation: 1, companyId: "000001", cursor: null, reports: [report("000001", "7")], nextCursor: null }));
  state = companyReducer(state, recordCompanyPage({ generation: 1, companyId: "600001", cursor: "7", reports: [report("600001", "8", "2")], nextCursor: null }));
  assert.equal(state.companies["600001"]?.reports["7"]?.accounting.total_assets, "9007199254740993.01");
  assert.equal(state.companies["000001"]?.reports["7"]?.company_id, "000001");
  assert.equal(typeof state.companies["600001"]?.reports["7"]?.accounting.total_assets, "string");
  assert.equal(state.companies["600001"]?.reports["7"]?.accounting.total_assets, "9007199254740993.01");
  const secondPage = state.companies["600001"]?.pages["7"];
  assert.equal(secondPage?.kind, "ready");
  if (secondPage?.kind === "ready") assert.deepEqual(secondPage.reportIds, ["8"]);
});

test("models loading, failure, empty and unavailable comparison without numeric coercion", () => {
  let state = companyReducer(undefined, installCompanyBaseline({ generation: 1, civilDate: "2030-01-01", revision: "1", seq: 0 }));
  state = companyReducer(state, startCompanyQuery({ generation: 1, companyId: "600001", cursor: null }));
  assert.equal(state.companies["600001"]?.pages.root.kind, "loading");
  state = companyReducer(state, recordCompanyQueryFailure({ generation: 1, companyId: "600001", cursor: null, message: "network down" }));
  assert.equal(state.companies["600001"]?.pages.root.kind, "error");
  state = companyReducer(state, recordCompanyPage({ generation: 1, companyId: "600001", cursor: null, reports: [], nextCursor: null }));
  assert.equal(state.companies["600001"]?.pages.root.kind, "empty");
  state = companyReducer(state, recordCompanyPage({ generation: 1, companyId: "600001", cursor: "after-7", reports: [report("600001", "8")], nextCursor: null }));
  const comparative = state.companies["600001"]?.reports["8"]?.accounting.prior_year_net_income;
  assert.deepEqual(comparative, { Unavailable: { reason: "NoPriorYearHistory" } });
});

test("applies civil date and disclosure events without a trade while rejecting old sequence values", () => {
  let state = companyReducer(undefined, installCompanyBaseline({ generation: 1, civilDate: "2030-01-01", revision: "1", seq: 0 }));
  state = companyReducer(state, recordCompanyPage({ generation: 1, companyId: "600001", cursor: null, reports: [report("600001", "7")], nextCursor: null }));
  state = companyReducer(state, recordCivilDateAdvanced({
    generation: 1, seq: 5, civilDate: "2030-01-02", dayStatus: { Closed: "Weekend" },
  }));
  state = companyReducer(state, invalidateCompanyForDisclosure({ generation: 1, seq: 6, companyId: "600001" }));
  state = companyReducer(state, recordCivilDateAdvanced({ generation: 1, seq: 5, civilDate: "2030-01-03", dayStatus: "Trading" }));
  assert.equal(state.civilDate, "2030-01-02");
  assert.equal(state.revision, "1");
  assert.deepEqual(state.companies["600001"]?.pages, {});
});

test("anchors company event ordering at the baseline sequence", () => {
  let state = companyReducer(undefined, installCompanyBaseline({ generation: 1, civilDate: "2030-01-01", revision: "1", seq: 9 }));
  state = companyReducer(state, recordCivilDateAdvanced({
    generation: 1, seq: 9, civilDate: "2030-01-02", dayStatus: "Trading",
  }));
  assert.equal(state.civilDate, "2030-01-01");
  assert.equal(state.lastEventSeq, 9);
});

test("does not regress public event coverage when a stale completion arrives", () => {
  let state = companyReducer(undefined, installCompanyBaseline({ generation: 1, civilDate: "2030-01-01", revision: "1", seq: 8 }));
  state = companyReducer(state, recordCivilDateAdvanced({
    generation: 1, seq: 10, civilDate: "2030-01-02", dayStatus: "Trading",
  }));
  state = companyReducer(state, {
    type: "company/advanceCompanyEventCoverage",
    payload: { generation: 1, toSeq: 9 },
  });
  assert.equal(state.lastEventSeq, 10);
});
