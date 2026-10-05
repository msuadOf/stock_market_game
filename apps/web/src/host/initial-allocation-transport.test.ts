import assert from "node:assert/strict";
import test from "node:test";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { createWorkerHost } from "./worker-host.ts";
import { createTauriHost } from "./tauri-host.ts";
import { remoteTestContext } from "./remote-test-context.ts";
import { createRemoteHost } from "./remote-host.ts";
import { DEFAULT_SETUP } from "../config/defaults.ts";
import { snapshot } from "./protocol-test-fixtures.ts";

const setup = { ...DEFAULT_SETUP, stocks: DEFAULT_SETUP.stocks.slice(0, 1) };
const allocation = { stocks: [{ code: "600101", float_shares: 7, unallocated_shares: 0, categories: [
  { kind: "Retail", shares: 3, account_count: 5, zero_holders: 2 },
  { kind: "Inst", shares: 4, account_count: 1, zero_holders: 0 },
  { kind: "Hot", shares: 0, account_count: 0, zero_holders: 0 },
] }] };

test("Worker实际分配query绑定requestId/generation，refresh后旧响应失效", { timeout: 10000 }, async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  class AllocationWorker extends EventTarget {
    static current: AllocationWorker;
    readonly sent: Record<string, unknown>[] = [];
    constructor() { super(); AllocationWorker.current = this; }
    postMessage(value: Record<string, unknown>) { this.sent.push(value); }
    terminate() {}
    emit(value: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data: value })); }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: AllocationWorker });
  let host: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
  try {
    const creating = createWorkerHost(setup, 17n);
    const worker = AllocationWorker.current;
    worker.emit({ type: "baseline", generation: 1, snapshot: snapshot(0, 0) });
    host = await creating;
    const current = host.initialAllocation();
    const request = worker.sent.at(-1)!;
    assert.deepEqual(request, { type: "initialAllocation", requestId: request.requestId, generation: 1 });
    worker.emit({ type: "initialAllocation", requestId: request.requestId, generation: 1, data: allocation });
    assert.deepEqual(await current, allocation);
    const stale = host.initialAllocation();
    const previous = worker.sent.at(-1)!;
    const rejected = assert.rejects(stale, /已过期会话 generation/);
    const refreshed = host.refreshBaseline();
    worker.emit({ type: "refreshed", requestId: worker.sent.at(-1)!.requestId, generation: 1, snapshot: snapshot(0, 0) });
    await refreshed;
    worker.emit({ type: "initialAllocation", requestId: previous.requestId, generation: 1, data: allocation });
    await rejected;
  } finally {
    await host?.dispose();
    if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
    else Object.defineProperty(globalThis, "Worker", original);
  }
});

test("Tauri实际分配query使用会话身份，refresh后拒绝旧响应", { timeout: 10000 }, async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", { configurable: true, value: { crypto: globalThis.crypto } });
  let finish!: (value: unknown) => void;
  let requestCount = 0;
  mockIPC((command, args) => {
    if (command === "create_session") return { sessionId: "allocation-session", setup: DEFAULT_SETUP, seed: "1", resumed: false };
    if (command === "host_capabilities") return { npcDecisionDiagnostics: false, indicatorCapabilities: { intradayAverage: true, macd: true, priceKdj: true, candleKdj: true }, personalTradeHistory: true };
    if (command === "engine_baseline") return { snapshot: snapshot(0, 0), generation: "1", timeline_id: "allocation-session" };
    if (command === "initial_allocation") {
      assert.deepEqual(args, { sessionId: "allocation-session", generation: "1" });
      if (++requestCount === 1) return { generation: "1", value: allocation };
      return new Promise((resolve) => { finish = resolve; });
    }
    return null;
  }, { shouldMockEvents: true });
  let host: Awaited<ReturnType<typeof createTauriHost>> | undefined;
  try {
    host = await createTauriHost(setup, 17n);
    assert.deepEqual(await host.initialAllocation(), allocation);
    const stale = assert.rejects(host.initialAllocation(), /已过期会话 generation/);
    await host.refreshBaseline();
    finish({ generation: "1", value: allocation });
    await stale;
  } finally {
    await host?.dispose();
    clearMocks();
    if (original === undefined) delete (globalThis as { window?: unknown }).window;
    else Object.defineProperty(globalThis, "window", original);
  }
});

test("Remote实际分配首次开跑前连接基线、不启动撮合，load后旧响应失效", { timeout: 10000 }, async () => {
  const requests: { pathname: string; init: RequestInit | undefined }[] = [];
  let responseGeneration = "1";
  let finish!: (value: Response) => void;
  let allocationCount = 0;
  const baseline = () => ({ Baseline: { timeline_generation: Number(responseGeneration), snapshot: snapshot(0, 0), civil_date: "2030-01-01", public_revision: 0, public_report_ids: [] } });
  const socket = {
    readyState: 1,
    onmessage: null as ((event: MessageEvent) => void) | null,
    onerror: null, onclose: null,
    close() {},
    send() { queueMicrotask(() => socket.onmessage?.({ data: JSON.stringify(baseline()) } as MessageEvent)); },
  };
  const json = (value: unknown) => new Response(JSON.stringify(value), { headers: { "content-type": "application/json" } });
  const host = await createRemoteHost(setup, 17n, {
    token: "owner-token", context: { ...remoteTestContext("allocation-session", "1", "17"), setup },
    baseUrl: "https://allocation.example",
    webSocketFactory: () => { queueMicrotask(() => socket.onmessage?.({ data: JSON.stringify(baseline()) } as MessageEvent)); return socket as unknown as WebSocket; },
    fetchFn: async (input, init) => {
      const url = new URL(String(input));
      requests.push({ pathname: url.pathname, init });
      if (url.pathname === "/api/market/context") return json({ ...remoteTestContext("allocation-session", responseGeneration, "17"), setup });
      if (url.pathname === "/api/host-capabilities") return json({ npcDecisionDiagnostics: false, indicatorCapabilities: { intradayAverage: true, macd: true, priceKdj: true, candleKdj: true }, personalTradeHistory: true });
      if (url.pathname === "/api/load") { responseGeneration = "2"; return json({}); }
      if (url.pathname === "/api/initial-allocation") {
        assert.equal(url.searchParams.get("session_id"), "allocation-session");
        assert.equal(url.searchParams.get("generation"), "1");
        assert.deepEqual(init?.headers, { authorization: "Bearer owner-token" });
        if (++allocationCount === 1) return json({ generation: "1", data: allocation });
        return new Promise((resolve) => { finish = resolve; });
      }
      return json({});
    },
  });
  try {
    assert.deepEqual(await host.initialAllocation(), allocation);
    assert.equal(requests.some((request) => request.pathname === "/api/running"), false);
    const stale = assert.rejects(host.initialAllocation(), (error: unknown) => {
      assert.ok(error instanceof Error);
      assert.equal(error.message, "远程基线重同步，确认中断，结果未知；请同步权威状态核对，勿重复提交委托");
      return true;
    });
    await host.load({});
    finish(json({ generation: "1", data: allocation }));
    await stale;
  } finally { await host.dispose(); }
});
