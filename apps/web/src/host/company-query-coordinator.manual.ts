import type { EngineHost } from "./engine-host.ts";
import { CompanyQueryCoordinator } from "./company-query-coordinator.ts";
import { companyReducer } from "../store/company-slice.ts";
import type { CompanyState } from "../store/company-slice.ts";

function deferred<T>(): { promise: Promise<T>; resolve(value: T): void } {
  let resolve: (value: T) => void = () => {};
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

const report = (companyId: string, id: string, totalAssets: string) => ({
  id, company_id: companyId, period: "2030-03-31", kind: "Quarter" as const, version_sequence: "1", supersedes: null,
  approved_date: "2030-04-01", approved_second_of_day: 0, published_date: "2030-04-02", published_second_of_day: 0,
  accounting: { total_assets: totalAssets, total_liabilities: "0.00", total_equity: "1.00", closing_cash: "1.00", quarter_net_income: "1.00", net_income: "1.00", income_tax: "0.00", operating_cash_flow: "1.00", investing_cash_flow: "0.00", financing_cash_flow: "0.00", net_cash_change: "1.00", prior_year_net_income: { Unavailable: { reason: "NoPriorYearHistory" as const } } },
});

const oldResponse = deferred<{ reports: ReturnType<typeof report>[]; next_cursor: null }>();
let state: CompanyState | undefined;
const host = {
  capabilities: { deliveryModes: [], targetUiHz: 60, sharedMemory: false, reconnect: false, publicCompanyReports: true, npcDecisionDiagnostics: false },
  queryPublicReports(query) {
    return query.company_id === "old" ? oldResponse.promise : Promise.resolve({
      reports: [report("new", "7", "9007199254740993.01")], next_cursor: null,
    });
  },
} satisfies Pick<EngineHost, "capabilities" | "queryPublicReports">;
const coordinator = new CompanyQueryCoordinator(host, (action) => { state = companyReducer(state, action); });
coordinator.installBaseline({ civilDate: "2030-01-01", revision: "1", seq: 0 });
const oldQuery = coordinator.query({ companyId: "old", cursor: null });
coordinator.installBaseline({ civilDate: "2031-01-01", revision: "2", seq: 0 });
await coordinator.query({ companyId: "new", cursor: null });
const beforeLateResponse = JSON.stringify(state);
oldResponse.resolve({ reports: [report("old", "7", "1.00")], next_cursor: null });
await oldQuery;
const afterLateResponse = JSON.stringify(state);
coordinator.acceptEvents([{ CompanyDisclosurePublished: {
  seq: 1, publication_id: 8, company: "new", published_at: { date: "2031-01-01", second_of_day: 64_800 }, kind: "Announcement",
} }, { CivilDateAdvanced: {
  seq: 2, settled_date: "2031-01-01", next_date: "2031-01-02", next_status: { Closed: "Weekend" },
} }], { fromSeq: 1, toSeq: 2 });
await new Promise<void>((resolve) => setImmediate(resolve));
let outOfOrderRejected = false;
try {
  coordinator.acceptEvents([{ CivilDateAdvanced: {
    seq: 4, settled_date: "2031-01-02", next_date: "2031-01-03", next_status: "Trading",
  } }, { CompanyDisclosurePublished: {
    seq: 3, publication_id: 9, company: "new", published_at: { date: "2031-01-02", second_of_day: 64_800 }, kind: "Announcement",
  } }], { fromSeq: 3, toSeq: 4 });
} catch (error) {
  outOfOrderRejected = error instanceof Error && error.message.includes("覆盖区间不一致");
}
const storedReport = state?.companies.new?.reports["7"];
console.log(JSON.stringify({
  delayedOldResponseIgnored: beforeLateResponse === afterLateResponse,
  currentCompany: storedReport?.company_id,
  exactAssets: storedReport?.accounting.total_assets,
  immutableOldReportVersion: storedReport?.accounting.total_assets === "9007199254740993.01",
  civilDateAfterClosedDay: state?.civilDate,
  cachedCompanyRefreshedAfterDisclosure: state?.companies.new?.pages.root.kind === "ready",
  outOfOrderRejected,
}, null, 2));
