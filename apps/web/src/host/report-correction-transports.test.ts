import assert from "node:assert/strict";
import test from "node:test";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { createTauriHost } from "./tauri-host.ts";
import { remoteTestContext } from "./remote-test-context.ts";
import { createRemoteHost } from "./remote-host.ts";
import type { SessionSetup } from "../types/engine.ts";
import { DEFAULT_SETUP } from "../config/defaults.ts";

const snapshot = { seq: 0, tick: 0, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} };
const capabilities = { npcDecisionDiagnostics: false, indicatorCapabilities: { intradayAverage: true, macd: true, priceKdj: true, candleKdj: true }, personalTradeHistory: true };
const request = { operation_id: "correct-7", company: "C-600000", supersedes: 2, entries: [{ source: 7, date: "2030-01-02", kind: "CashRevenue", cash_flow: "NonCash", lines: [{ account: "1001", side: "Debit", amount: "10.00" }, { account: "6001", side: "Credit", amount: "10.00" }] }], reason: "真实差错" } as const;

test("Tauri更正真实host命令携带generation，严格解析且换档拒绝晚到查询", { timeout: 10000 }, async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", { configurable: true, value: { crypto: globalThis.crypto } });
  const calls: { command: string; args: unknown }[] = [];
  let complete: ((value: unknown) => void) | undefined;
  let defer = false;
  mockIPC((command, args) => {
    calls.push({ command, args });
    if (command === "create_session") return { sessionId: "session-1", setup: DEFAULT_SETUP, seed: "1", resumed: false };
    if (command === "host_capabilities") return capabilities;
    if (command === "engine_baseline") return { snapshot, generation: "1", timeline_id: "timeline-1" };
    if (command === "restore_session") return { snapshot, generation: "2", timeline_id: "timeline-2" };
    if (command === "query_report_corrections") return defer ? new Promise((resolve) => { complete = resolve; }) : { generation: "1", value: { pending: [request], completed: {} } };
    if (command === "submit_report_correction" || command === "cancel_report_correction") return { generation: "1", value: null };
    return null;
  }, { shouldMockEvents: true });
  let host: Awaited<ReturnType<typeof createTauriHost>> | undefined;
  try {
    host = await createTauriHost({} as SessionSetup, 1n);
    const failures: unknown[] = [];
    await host.start(() => true, (failure) => failures.push(failure));
    const failure = { session_id: "session-1", timeline_id: "timeline-1", code: "REPORT_CORRECTION_REJECTED", where: "session.dayEnd", message: "真实拒绝", events: [], recoverable: true, recoveryActions: ["取消待办"], context: { generation: "1" } };
    await emit("engine-failure", { ...failure, context: { generation: "0" } });
    assert.equal(failures.length, 0);
    await emit("engine-failure", failure);
    assert.equal(failures.length, 1);
    await host.submitReportCorrection(request);
    await host.cancelReportCorrection(request.operation_id);
    assert.deepEqual(await host.queryReportCorrections(), { pending: [request], completed: {} });
    assert.deepEqual(calls.find((item) => item.command === "submit_report_correction")?.args, { sessionId: "session-1", generation: "1", request });
    assert.deepEqual(calls.find((item) => item.command === "cancel_report_correction")?.args, { sessionId: "session-1", generation: "1", operationId: "correct-7" });
    defer = true;
    const stale = host.queryReportCorrections();
    await host.load({});
    complete!({ generation: "1", value: { pending: [request], completed: {} } });
    await assert.rejects(stale, /generation/);
  } finally {
    await host?.dispose();
    clearMocks();
    if (original === undefined) delete (globalThis as { window?: unknown }).window;
    else Object.defineProperty(globalThis, "window", original);
  }
});

test("Remote更正真实REST限于市场控制且Bearer+generation，换档拒绝旧状态", { timeout: 10000 }, async () => {
  const calls: { url: URL; init?: RequestInit }[] = [];
  const json = (value: unknown) => new Response(JSON.stringify(value), { headers: { "content-type": "application/json" } });
  let generation = 1;
  let complete: ((value: Response) => void) | undefined;
  let defer = false;
  const socket = {
    readyState: 1, onmessage: null as ((event: MessageEvent) => void) | null, onerror: null, onclose: null,
    close() {},
    send() { queueMicrotask(() => socket.onmessage?.({ data: JSON.stringify({ Baseline: { timeline_generation: generation, snapshot, civil_date: "2030-01-02", public_revision: 0, public_report_ids: [] } }) } as MessageEvent)); },
  };
  const host = await createRemoteHost(DEFAULT_SETUP, 1n, { token: "owner-token", context: remoteTestContext("session-1", "1", "1"),
    baseUrl: "https://market.example", webSocketFactory: () => {
      queueMicrotask(() => socket.send());
      return socket as unknown as WebSocket;
    },
    fetchFn: async (input, init) => {
      const url = new URL(String(input));
      if (url.pathname === "/api/market/context") return json(remoteTestContext("session-1", String(generation)));
      if (url.pathname === "/api/host-capabilities") return json(capabilities);
      if (url.pathname === "/api/load") { generation = 2; return json({}); }
      if (url.pathname === "/api/market/report-corrections") {
        calls.push({ url, init });
        if (init?.method === "GET") return defer ? new Promise((resolve) => { complete = resolve; }) : json({ generation: "1", value: { pending: [request], completed: {} } });
        return json({ generation: "1", value: null });
      }
      return json({});
    },
  });
  try {
    const failures: unknown[] = [];
    await host.start(() => true, (failure) => failures.push(failure));
    await new Promise<void>((resolve) => setImmediate(resolve));
    const failure = { code: "REPORT_CORRECTION_REJECTED", where: "session.dayEnd", message: "真实拒绝", recoverable: true, recoveryActions: ["取消待办"], context: { generation: "1" } };
    socket.onmessage?.({ data: JSON.stringify({ HostFailure: { ...failure, context: { generation: "0" } } }) } as MessageEvent);
    assert.equal(failures.length, 0);
    socket.onmessage?.({ data: JSON.stringify({ HostFailure: failure }) } as MessageEvent);
    assert.equal(failures.length, 1);
    await host.submitReportCorrection(request);
    await host.cancelReportCorrection(request.operation_id);
    assert.deepEqual(await host.queryReportCorrections(), { pending: [request], completed: {} });
    assert.deepEqual(JSON.parse(String(calls[0]!.init?.body)), { session_id: "session-1", generation: "1", request });
    assert.equal(calls[1]!.init?.method, "DELETE");
    assert.deepEqual([...calls[2]!.url.searchParams], [["session_id", "session-1"], ["generation", "1"]]);
    for (const call of calls) assert.equal(new Headers(call.init?.headers).get("authorization"), "Bearer owner-token");
    defer = true;
    const stale = host.queryReportCorrections();
    const interrupted = assert.rejects(stale, /远程基线重同步，确认中断，结果未知/);
    await host.load({});
    complete!(json({ generation: "1", value: { pending: [request], completed: {} } }));
    await interrupted;
  } finally { await host.dispose(); }
});
