import assert from "node:assert/strict";
import test from "node:test";
import type { EngineHost } from "./engine-host.ts";
import { CompanyQueryCoordinator } from "./company-query-coordinator.ts";
import { frame, civilUpdate, isJsonRecord, recordArray } from "./protocol-test-fixtures.ts";
import { parseNormalizedEngineUpdate, type NormalizedTickFrame } from "./protocol/index.ts";
import type { EngineEvent } from "../types/engine.ts";
import { canonicalJson } from "./protocol/canonical.ts";

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

function normalizedFrame(events: readonly EngineEvent[], seqFrom: number, seqTo: number): NormalizedTickFrame {
  return {
    tick: 1,
    events,
    facts: [],
    continuousPoints: {},
    auctionPoints: {},
    closedDailyCandles: {},
    activeDailyCandles: {},
    markets: {},
    seqFrom,
    seqTo,
  };
}

function civil(seq: number): EngineEvent {
  return { CivilDateAdvanced: {
    seq, settled_date: "2030-01-01", next_date: "2030-01-02", next_status: "Trading",
  } };
}

function disclosure(seq: number, company: string): EngineEvent {
  return { CompanyDisclosurePublished: {
    seq, publication_id: 9, company, published_at: { date: "2030-01-02", second_of_day: 64_800 }, kind: "Announcement",
  } };
}

const host = {
  capabilities: { deliveryModes: [], targetUiHz: 60, sharedMemory: false, reconnect: false, publicCompanyReports: false, npcDecisionDiagnostics: false },
};

test("Given a normalized tick frame, when public metadata advances, then company coverage follows its protocol cursor", () => {
  const actions: unknown[] = [];
  const coordinator = new CompanyQueryCoordinator(host, (action) => actions.push(action));
  coordinator.installBaseline({ civilDate: "2030-01-01", revision: "1", seq: 0 });
  const normalized = parseNormalizedEngineUpdate({ TickBatch: { frames: [frame(1, 0, ["600000"])], runtime_snapshot: null } });
  if (normalized.kind !== "tick-batch") throw new Error("test fixture invalid");
  coordinator.acceptFrame(normalized.frames[0]!, { civilDate: "2030-01-01", revision: "2" });
  assert.match(JSON.stringify(actions), /advanceCompanyEventCoverage/);
  assert.match(JSON.stringify(actions), /"revision":"2"/);
});

test("Given a CivilUpdate refresh, when accepted, then the coordinator installs its authoritative public date", () => {
  const actions: unknown[] = [];
  const coordinator = new CompanyQueryCoordinator(host, (action) => actions.push(action));
  coordinator.installBaseline({ civilDate: "2030-01-01", revision: "1", seq: 1 });
  const normalized = parseNormalizedEngineUpdate(civilUpdate());
  if (normalized.kind !== "civil-update") throw new Error("test fixture invalid");
  coordinator.acceptCivil(normalized, { civilDate: "2030-01-03", revision: "3" });
  assert.match(JSON.stringify(actions), /recordCivilDateAdvanced/);
  assert.match(JSON.stringify(actions), /2030-01-03/);
});

test("rejects delayed prior-session page and by-id responses while accepting current session data", async () => {
  const late = deferred<{ reports: ReturnType<typeof report>[]; next_cursor: string | null }>();
  const lateById = deferred<ReturnType<typeof report>>();
  const updates: unknown[] = [];
  const queryHost = {
    capabilities: { ...host.capabilities, publicCompanyReports: true },
    queryPublicReports(query) {
      return query.company_id === "old" ? late.promise : Promise.resolve({
        reports: [report("new", query.cursor === null ? "7" : "8")],
        next_cursor: query.cursor === null ? "7" : null,
      });
    },
    publicReportById(id) { return id === "old-report" ? lateById.promise : Promise.resolve(report("new", id)); },
  } satisfies QueryHost;
  const coordinator = new CompanyQueryCoordinator(queryHost, (action) => updates.push(action));
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
  assert.equal(updates.some((action) => JSON.stringify(action).includes('"companyId":"old"') && JSON.stringify(action).includes("recordCompanyReport")), false);
  assert.equal(updates.filter((action) => JSON.stringify(action).includes('"companyId":"new"') && JSON.stringify(action).includes('"reports"')).length, 2);
});

test("reports malformed public DTOs as explicit query errors and unavailable capability separately", async () => {
  const updates: unknown[] = [];
  const malformed = new CompanyQueryCoordinator({
    capabilities: { ...host.capabilities, publicCompanyReports: true },
    queryPublicReports: () => Promise.resolve(JSON.parse('{"reports":[{"id":"7"}],"next_cursor":null}')),
  } satisfies QueryHost, (action) => updates.push(action));
  malformed.installBaseline({ civilDate: "2030-01-01", revision: "1", seq: 0 });
  await malformed.query({ companyId: "600001", cursor: null });
  assert.match(JSON.stringify(updates), /recordCompanyQueryFailure/);
  assert.match(JSON.stringify(updates), /公共 DTO 契约/);

  const unsupported = new CompanyQueryCoordinator(host, (action) => updates.push(action));
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
      capabilities: { ...host.capabilities, publicCompanyReports: true },
      queryPublicReports: () => Promise.resolve({ reports: [invalidReport], next_cursor: null }),
    } satisfies QueryHost, (action) => updates.push(action));
    coordinator.installBaseline({ civilDate: "2030-01-01", revision: "1", seq: 0 });
    await coordinator.query({ companyId: "600001", cursor: null });
    assert.match(JSON.stringify(updates), /recordCompanyQueryFailure/);
  }
});

test("refreshes an already cached company after a disclosure before advancing coverage", async () => {
  const updates: unknown[] = [];
  const queryHost = {
    capabilities: { ...host.capabilities, publicCompanyReports: true },
    queryPublicReports(query) { return Promise.resolve({ reports: [report(query.company_id, "8")], next_cursor: null }); },
  } satisfies QueryHost;
  const coordinator = new CompanyQueryCoordinator(queryHost, (action) => updates.push(action));
  coordinator.installBaseline({ civilDate: "2030-01-01", revision: "1", seq: 0 });
  await coordinator.query({ companyId: "600001", cursor: null });
  coordinator.acceptFrame(normalizedFrame([disclosure(1, "600001"), civil(2)], 0, 2));
  await new Promise((resolve) => setImmediate(resolve));
  const serialized = updates.map((action) => JSON.stringify(action));
  assert.ok(serialized.findIndex((action) => action.includes("invalidateCompanyForDisclosure"))
    < serialized.findIndex((action) => action.includes("advanceCompanyEventCoverage")));
  assert.match(serialized.join("\n"), /recordCivilDateAdvanced/);
  assert.equal(serialized.filter((action) => action.includes("recordCompanyPage")).length, 2);
});

test("does not refresh a company absent from the cache", async () => {
  const updates: unknown[] = [];
  let queries = 0;
  const queryHost = {
    capabilities: { ...host.capabilities, publicCompanyReports: true },
    queryPublicReports() { queries += 1; return Promise.resolve({ reports: [], next_cursor: null }); },
  } satisfies QueryHost;
  const coordinator = new CompanyQueryCoordinator(queryHost, (action) => updates.push(action));
  coordinator.installBaseline({ civilDate: "2030-01-01", revision: "1", seq: 9 });
  coordinator.acceptFrame(normalizedFrame([disclosure(10, "uncached")], 9, 10));
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(queries, 0);
  assert.equal(JSON.stringify(updates).includes('"seq":10'), true);
});

test("rejects a TickFrame coverage gap so the host can deliver a fresh baseline", () => {
  const coordinator = new CompanyQueryCoordinator(host, () => {});
  coordinator.installBaseline({ civilDate: "2030-01-01", revision: "1", seq: 1 });
  assert.throws(() => coordinator.acceptFrame(normalizedFrame([civil(4)], 3, 4)), /需要宿主重同步/);
});

test("accepts a multi-event TickFrame whose exclusive coverage cursor is continuous", () => {
  const coordinator = new CompanyQueryCoordinator(host, () => {});
  coordinator.installBaseline({ civilDate: "2030-01-01", revision: "1", seq: 1 });
  coordinator.acceptFrame(normalizedFrame([disclosure(2, "600001"), civil(3)], 1, 3));
});

test("rejects a reversed empty TickFrame range before it can desynchronize the coordinator", () => {
  const coordinator = new CompanyQueryCoordinator(host, () => {});
  coordinator.installBaseline({ civilDate: "2030-01-01", revision: "1", seq: 1 });
  assert.throws(() => coordinator.acceptFrame(normalizedFrame([], 2, 1)), /覆盖区间无效/);
  coordinator.acceptFrame(normalizedFrame([civil(2)], 1, 2));
});

test("accepts reordered TickFrame events after protocol normalization restores canonical seq order", () => {
  const coordinator = new CompanyQueryCoordinator(host, () => {});
  coordinator.installBaseline({ civilDate: "2030-01-01", revision: "1", seq: 0 });
  const wireFrame = frame(1, 0, ["600001", "600002"]);
  const normalized = parseNormalizedEngineUpdate({ TickBatch: { frames: [{
    ...wireFrame,
    events: [...recordArray(wireFrame.events, "events")].reverse(),
    facts: [...recordArray(wireFrame.facts, "facts")].reverse(),
  }], runtime_snapshot: null } });
  if (normalized.kind !== "tick-batch") throw new Error("test fixture invalid");
  coordinator.acceptFrame(normalized.frames[0]!);
});

test("accepts reordered CivilUpdate events and facts after protocol normalization", () => {
  const source = civilUpdate();
  if (!isJsonRecord(source.CivilUpdate)) throw new Error("test fixture invalid");
  const update = source.CivilUpdate;
  if (!isJsonRecord(update.refresh) || !isJsonRecord(update.refresh.snapshot)) throw new Error("test fixture invalid");
  const disclosureEvent = {
    CompanyDisclosurePublished: {
      seq: 2, publication_id: 9, company: "600001",
      published_at: { date: "2030-01-02", second_of_day: 64_800 }, kind: "Announcement",
    },
  };
  const civilEvent: EngineEvent = { CivilDateAdvanced: {
    seq: 3, settled_date: "2030-01-02", next_date: "2030-01-03", next_status: "Trading",
  } };
  const normalized = parseNormalizedEngineUpdate({ CivilUpdate: {
    ...update,
    events: [civilEvent, disclosureEvent],
    facts: [{
      key: { phase_rank: 6, entity: "Session", source: "Session", local_event_index: 1 },
      event: civilEvent,
      canonical_payload: canonicalJson(civilEvent),
    }, {
      key: { phase_rank: 6, entity: "Session", source: "Session", local_event_index: 0 },
      event: disclosureEvent,
      canonical_payload: canonicalJson(disclosureEvent),
    }],
    seq_to: 3,
    refresh: {
      ...update.refresh,
      snapshot: { ...update.refresh.snapshot, seq: 3 },
      public_publication_ids: ["9"],
    },
  } });
  if (normalized.kind !== "civil-update") throw new Error("test fixture invalid");
  const [first, second] = normalized.update.events;
  assert.ok(first !== undefined && "CompanyDisclosurePublished" in first);
  assert.ok(second !== undefined && "CivilDateAdvanced" in second);
  assert.equal(first.CompanyDisclosurePublished.seq, 2);
  assert.equal(second.CivilDateAdvanced.seq, 3);
  const coordinator = new CompanyQueryCoordinator(host, () => {});
  coordinator.installBaseline({ civilDate: "2030-01-01", revision: "1", seq: 1 });
  coordinator.acceptCivil(normalized);
});
