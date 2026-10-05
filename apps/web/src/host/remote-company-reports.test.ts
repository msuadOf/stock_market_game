import assert from "node:assert/strict";
import test from "node:test";
import type { PublicReportAvailabilityQuery, PublicReportSummary } from "../types/engine.ts";
import { DEFAULT_SETUP } from "../config/defaults.ts";
import { publicReportGold } from "../components/company/public-report-fixture.ts";
import { remoteTestContext } from "./remote-test-context.ts";
import { createRemoteHost } from "./remote-host.ts";

function reportFixture(): PublicReportSummary {
  return publicReportGold();
}

function json(value: unknown): Response {
  return new Response(JSON.stringify(value), { headers: { "content-type": "application/json" } });
}

async function remoteFixture(respond: (request: number) => Response | Promise<Response>, token = "session-token") {
  const requests: { url: URL; init: RequestInit | undefined }[] = [];
  const host = await createRemoteHost(DEFAULT_SETUP, 1n, { context: remoteTestContext("session /?&"),
    baseUrl: "https://reports.example",
    token,
    fetchFn: async (input, init) => {
      const url = new URL(input instanceof Request ? input.url : String(input));
      if (url.pathname === "/api/host-capabilities") return json({ npcDecisionDiagnostics: false, indicatorCapabilities: { intradayAverage: true, macd: true, priceKdj: true, candleKdj: true }, personalTradeHistory: true });
      requests.push({ url, init });
      return respond(requests.length);
    },
    webSocketFactory: () => { throw new Error("report queries must not open a WebSocket"); },
  });
  assert.ok(host.queryPublicReports);
  assert.ok(host.publicReportById);
  return {
    host, requests,
    query: host.queryPublicReports.bind(host),
    byId: host.publicReportById.bind(host),
  };
}

test("remote report ownership cannot survive a restored timeline with reused IDs", async () => {
  const old = reportFixture();
  const replacement = { ...old, company_id: "C-replacement" };
  let restored = false;
  const socket = {
    readyState: 1,
    onmessage: null as ((event: MessageEvent) => void) | null,
    onerror: null,
    onclose: null,
    close() {},
    send() {
      queueMicrotask(() => socket.onmessage?.({ data: JSON.stringify({ Baseline: {
        timeline_generation: restored ? 2 : 1,
        snapshot: { seq: 0, tick: 0, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} },
        civil_date: "2030-01-02", public_revision: 0, public_report_ids: [],
      } }) } as MessageEvent));
    },
  };
  const host = await createRemoteHost(DEFAULT_SETUP, 1n, { token: "token-1", context: remoteTestContext("session-1", "1", "1"),
    baseUrl: "https://reports.example",
    webSocketFactory: () => socket as unknown as WebSocket,
    fetchFn: async (input) => {
      const url = new URL(String(input));
      if (url.pathname === "/api/host-capabilities") return json({ npcDecisionDiagnostics: false, indicatorCapabilities: { intradayAverage: true, macd: true, priceKdj: true, candleKdj: true }, personalTradeHistory: true });
      if (url.pathname === "/api/market/context") return json(remoteTestContext("session-1", restored ? "2" : "1"));
      if (url.pathname === "/api/load") { restored = true; return json({}); }
      if (url.pathname.endsWith(`/reports/${old.id}`)) return json(restored ? replacement : old);
      return json({ reports: [restored ? replacement : old], next_cursor: null });
    },
  });
  await host.start(() => {});
  await host.queryPublicReports!({ company_id: old.company_id, cursor: null, page_size: null });
  await host.load({});
  await assert.rejects(host.publicReportById!(old.id), /公司.*先查询/);
  const page = await host.queryPublicReports!({ company_id: replacement.company_id, cursor: null, page_size: null });
  assert.deepEqual(page.reports, [replacement]);
  assert.deepEqual(await host.publicReportById!(old.id), replacement);
});

test("remote advertises its public company report capability", { concurrency: true }, async () => {
  const { host } = await remoteFixture(() => json({ reports: [], next_cursor: null }));
  assert.equal(host.capabilities.publicCompanyReports, true);
});

test("remote page uses the company route, opaque cursor, server limit and owning token", { concurrency: true }, async () => {
  const report = { ...reportFixture(), id: "9007199254740993", company_id: "C-/ ?&" };
  const page = { reports: [report], next_cursor: report.id };
  const { query, requests } = await remoteFixture(() => json(page));
  assert.deepEqual(await query({ company_id: report.company_id, cursor: report.id, page_size: 1 }), page);
  assert.equal(requests.length, 1);
  assert.equal(requests[0]!.url.pathname, `/api/companies/${encodeURIComponent(report.company_id)}/reports`);
  assert.deepEqual([...requests[0]!.url.searchParams], [["session_id", "session /?&"], ["cursor", report.id], ["limit", "1"]]);
  assert.equal(requests[0]!.init?.method, "GET");
  assert.equal(new Headers(requests[0]!.init?.headers).get("authorization"), "Bearer session-token");
});

test("remote availability posts the required query and strictly parses tagged results", async () => {
  const companyId = "C-600101";
  const query = { company_id: companyId, period_end: "2030-03-31", kind: "Quarter", scope: { Standalone: { entity_id: companyId } } } satisfies PublicReportAvailabilityQuery;
  const available = { status: "Available", report: { ...reportFixture(), company_id: companyId } };
  const { host, requests } = await remoteFixture(() => json(available));
  assert.deepEqual(await host.queryPublicReportAvailability!(query), available);
  assert.equal(requests[0]!.url.pathname, `/api/companies/${encodeURIComponent(companyId)}/reports/availability`);
  assert.deepEqual([...requests[0]!.url.searchParams], [["session_id", "session /?&"]]);
  assert.equal(requests[0]!.init?.method, "POST");
  assert.deepEqual(JSON.parse(String(requests[0]!.init?.body)), query);
  const bad = await remoteFixture(() => json({ status: "Unavailable", reason: "NotYetPublished", future_facts: { income: "999.00" } }));
  await assert.rejects(bad.host.queryPublicReportAvailability!(query), /字段不符合公共 DTO 契约/);
});

test("remote omits null pagination rather than sending null or inventing limits", { concurrency: true }, async () => {
  const page = { reports: [], next_cursor: null };
  const { query, requests } = await remoteFixture(() => json(page));
  assert.deepEqual(await query({ company_id: "C-600101", cursor: null, page_size: null }), page);
  assert.deepEqual([...requests[0]!.url.searchParams], [["session_id", "session /?&"]]);
});

test("remote by-ID refetches the report from its verified company route with the explicit token", { concurrency: true }, async () => {
  const report = { ...reportFixture(), id: "9007199254740993" };
  const { query, byId, requests } = await remoteFixture((request) => json(request === 1 ? { reports: [report], next_cursor: null } : report), "explicit-token");
  await query({ company_id: report.company_id, cursor: null, page_size: null });
  assert.deepEqual(await byId(report.id), report);
  assert.equal(requests.length, 2);
  assert.equal(requests[1]!.url.pathname, `/api/companies/${report.company_id}/reports/${report.id}`);
  assert.deepEqual([...requests[1]!.url.searchParams], [["session_id", "session /?&"]]);
  assert.equal(requests[1]!.init?.method, "GET");
  for (const request of requests) assert.equal(new Headers(request.init?.headers).get("authorization"), "Bearer explicit-token");
});

test("remote rejects unknown report ownership without guessing a company or fetching", { concurrency: true }, async () => {
  const { byId, requests } = await remoteFixture(() => { throw new Error("unknown reports must not be fetched"); });
  await assert.rejects(byId("123"), /123.*公司.*先查询/);
  assert.equal(requests.length, 0);
});

test("remote rejects cross-company pages atomically without registering their valid prefix", { concurrency: true }, async () => {
  const report = reportFixture();
  const { query, byId, requests } = await remoteFixture(() => json({
    reports: [report, { ...report, id: "123", company_id: "C-other" }], next_cursor: null,
  }));
  await assert.rejects(query({ company_id: report.company_id, cursor: null, page_size: null }), /公司.*不匹配/);
  await assert.rejects(byId(report.id), /公司.*先查询/);
  assert.equal(requests.length, 1);
});

test("remote rejects conflicting report ownership without overwriting the verified company", { concurrency: true }, async () => {
  const report = reportFixture();
  const { query, byId, requests } = await remoteFixture((request) => json(request === 1
    ? { reports: [report], next_cursor: null }
    : request === 2 ? { reports: [{ ...report, company_id: "C-other" }], next_cursor: null } : report));
  await query({ company_id: report.company_id, cursor: null, page_size: null });
  await assert.rejects(query({ company_id: "C-other", cursor: null, page_size: null }), /公司.*不匹配/);
  assert.deepEqual(await byId(report.id), report);
  assert.equal(requests[2]!.url.pathname, `/api/companies/${report.company_id}/reports/${report.id}`);
});

for (const changed of [{ company_id: "C-other" }, { id: "123" }]) {
  test(`remote rejects a by-ID response with mismatched ${Object.keys(changed)[0]}`, { concurrency: true }, async () => {
    const report = reportFixture();
    const { query, byId } = await remoteFixture((request) => json(request === 1
      ? { reports: [report], next_cursor: null } : { ...report, ...changed }));
    await query({ company_id: report.company_id, cursor: null, page_size: null });
    await assert.rejects(byId(report.id), /报告.*不匹配/);
  });
}

for (const status of [403, 404, 500]) {
  test(`remote exposes HTTP ${status} for both report query entries without cached fallback`, { concurrency: true }, async () => {
    const report = reportFixture();
    const { query, byId, requests } = await remoteFixture((request) => request === 1
      ? json({ reports: [report], next_cursor: null }) : new Response("REPORT_NOT_VISIBLE", { status }));
    await query({ company_id: report.company_id, cursor: null, page_size: null });
    await assert.rejects(byId(report.id), new RegExp(`HTTP ${status}.*REPORT_NOT_VISIBLE`));
    await assert.rejects(query({ company_id: report.company_id, cursor: null, page_size: null }), new RegExp(`HTTP ${status}.*REPORT_NOT_VISIBLE`));
    assert.equal(requests.length, 3);
  });
}

test("remote exposes network, JSON and DTO failures instead of returning empty reports", { concurrency: true }, async () => {
  const report = reportFixture();
  const failures = [
    () => { throw new Error("network offline"); },
    () => new Response("not JSON"),
    () => json({ reports: [{ ...report, accounting: { ...report.accounting, total_assets: 1 } }], next_cursor: null }),
  ];
  for (const [index, failure] of failures.entries()) {
    const { query } = await remoteFixture(failure);
    await assert.rejects(query({ company_id: report.company_id, cursor: null, page_size: null }), [/network offline/, /不是合法 JSON/, /精确十进制字符串/][index]);
  }
});

for (const queryKind of ["page", "by-id"] as const) {
  test(`remote ${queryKind} response cannot cross a baseline timeline change`, { concurrency: true, timeout: 10000 }, async () => {
    const report = reportFixture();
    let delayed = false;
    let finish: ((response: Response) => void) | null = null;
    const socket = { readyState: 1, close() {}, send() {}, onmessage: null, onerror: null, onclose: null } as unknown as WebSocket;
    const host = await createRemoteHost(DEFAULT_SETUP, 1n, { token: "token-1", context: remoteTestContext("session-1", "1", "1"),
      baseUrl: "https://reports.example",
      webSocketFactory: () => socket,
      fetchFn: async (input) => {
        const path = new URL(String(input)).pathname;
        if (path === "/api/market/context") return json(remoteTestContext("session-1", "2"));
        if (path === "/api/host-capabilities") return json({ npcDecisionDiagnostics: false, indicatorCapabilities: { intradayAverage: true, macd: true, priceKdj: true, candleKdj: true }, personalTradeHistory: true });
        if (path.includes("/reports")) {
          if (delayed) return new Promise<Response>((resolve) => { finish = resolve; });
          return json({ reports: [report], next_cursor: null });
        }
        return json({});
      },
    });
    const publishBaseline = (generation: number) => socket.onmessage!({ data: JSON.stringify({ Baseline: {
      timeline_generation: generation,
      snapshot: { seq: 0, tick: 0, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} },
      civil_date: "2030-01-02", public_revision: 0, public_report_ids: [],
    } }) } as MessageEvent);
    host.start(() => {});
    publishBaseline(1);
    const query = { company_id: report.company_id, cursor: null, page_size: null };
    await host.queryPublicReports!(query);
    publishBaseline(1);
    delayed = true;
    const pending = queryKind === "page" ? host.queryPublicReports!(query) : host.publicReportById!(report.id);
    const rejected = assert.rejects(pending, /时间线变更失效/);
    publishBaseline(2);
    await new Promise<void>((resolve) => setImmediate(resolve));
    finish!(json(queryKind === "page" ? { reports: [report], next_cursor: null } : report));
    await rejected;
    await assert.rejects(host.publicReportById!(report.id), /公司.*先查询/);
    delayed = false;
    assert.deepEqual((await host.queryPublicReports!(query)).reports, [report]);
    await host.dispose();
  });
}
