import assert from "node:assert/strict";
import test from "node:test";
import type { SessionSetup } from "../types/engine.ts";
import type { HostFailure } from "./host-update.ts";
import { createRemoteHost, remoteSpeedValue } from "./remote-host.ts";
import { parseRemoteMessage } from "./remote-wire.ts";
import { remotePausePreferencePayload } from "./remote-request.ts";

const snapshot = {
  seq: 0, tick: 0, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {},
};

test("Given the remote whole-update DTO, when parsed, then it preserves generation and opaque EngineUpdate", () => {
  const parsed = parseRemoteMessage(JSON.stringify({ PublisherFrame: {
    timeline_generation: 1,
    update: { TickBatch: { frames: [], runtime_snapshot: null } },
    civil_date: "2030-01-02",
    public_revision: 7,
  } }));
  assert.equal(parsed.kind, "protocol");
  if (parsed.kind === "protocol") {
    assert.equal(parsed.update.generation, "1");
    assert.deepEqual(parsed.update.update, { TickBatch: { frames: [], runtime_snapshot: null } });
  }
});

test("Given the remote baseline DTO, when parsed, then it preserves generation and public metadata", () => {
  const parsed = parseRemoteMessage(JSON.stringify({ Baseline: {
    timeline_generation: 2,
    snapshot,
    civil_date: "2030-01-02",
    public_revision: 8,
    public_report_ids: ["7"],
  } }));
  assert.equal(parsed.kind, "baseline");
  if (parsed.kind === "baseline") {
    assert.equal(parsed.update.generation, "2");
    assert.deepEqual(parsed.update.publicPublicationIds, ["7"]);
  }
});

test("Given legacy flat events or frames, when parsed in production, then remote rejects them", () => {
  assert.throws(() => parseRemoteMessage(JSON.stringify({ PriceTick: { seq: 1 } }), "1"), /flat/);
  assert.throws(() => parseRemoteMessage(JSON.stringify({ EngineUpdate: { events: [] } }), "1"), /flat/);
});

test("Given multiple remote envelope variants, when parsed, then the ambiguous message is rejected", () => {
  assert.throws(
    () => parseRemoteMessage(JSON.stringify({
      HostFailure: { code: "STEP_FATAL", message: "receipt chain broke" },
      FrameEmpty: {},
    }), "1"),
    /必须且只能包含一个消息变体/,
  );
});

test("Given malformed remote control payloads, when parsed, then their exact wire shapes are rejected", () => {
  assert.throws(
    () => parseRemoteMessage(JSON.stringify({ ResyncRequired: { reason: "lagged" } }), "1"),
    /字段不符合远程协议契约/,
  );
  assert.throws(
    () => parseRemoteMessage(JSON.stringify({ ResyncRequired: { reason: "lagged", missed: -1 } }), "1"),
    /missed 无效/,
  );
  assert.throws(
    () => parseRemoteMessage(JSON.stringify({ FrameEmpty: { unexpected: true } }), "1"),
    /字段不符合远程协议契约/,
  );
  assert.throws(
    () => parseRemoteMessage(JSON.stringify({ CommandQueued: { request_id: 7, unexpected: true } }), "1"),
    /字段不符合远程协议契约/,
  );
  assert.throws(
    () => parseRemoteMessage(JSON.stringify({ GatewayError: { request_id: null, code: "BROKEN" } }), "1"),
    /HostFailure\.message/,
  );
  assert.throws(
    () => parseRemoteMessage(JSON.stringify({ HostFailure: { code: "", message: "receipt chain broke" } }), "1"),
    /非空字符串/,
  );
});

test("Given a pause preference payload mode, when converted from speed, then fastest stays JSON safe", () => {
  assert.equal(remoteSpeedValue(Infinity), "Fastest");
  assert.equal(remoteSpeedValue(60), 60);
});

test("Given enriched remote failures, when parsed, then diagnostic context and recovery guidance survive", () => {
  const parsed = parseRemoteMessage(JSON.stringify({ HostFailure: {
    code: "STEP_FATAL",
    where: "server.actor.step",
    message: "receipt chain broke",
    cause: { code: "LEDGER_GAP" },
    context: { tick: 12 },
    recoverable: true,
    recoveryActions: ["重新载入日终存档"],
  } }));
  assert.equal(parsed.kind, "failure");
  if (parsed.kind === "failure") assert.deepEqual(parsed.failure, {
    code: "STEP_FATAL",
    where: "server.actor.step",
    message: "receipt chain broke",
    cause: { code: "LEDGER_GAP" },
    context: { tick: 12 },
    recoverable: true,
    recoveryActions: ["重新载入日终存档"],
  });
});

test("Given an authenticated remote host, when player orders are queried, then it uses the read-only endpoint", async () => {
  const requests: { url: string; init?: RequestInit }[] = [];
  const fetchFn = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = input instanceof Request ? input.url : String(input);
    requests.push({ url, init });
    if (url.endsWith("/api/new")) {
      return new Response(JSON.stringify({ session_id: "session-1", session_token: "token-1" }), { status: 200 });
    }
    if (url.includes("/api/host-capabilities?")) return new Response(JSON.stringify({ npcDecisionDiagnostics: false }), { status: 200 });
    if (url.includes("/api/player-working-orders?")) {
      return new Response(JSON.stringify({ generation: "2", orders: [{
        id: 7, code: "600000", side: "Buy", price: "1234", remainingQty: 200, venue: "continuous", frozen: "cash",
      }] }), { status: 200 });
    }
    if (url.endsWith("/api/indicators")) return new Response(JSON.stringify({
      macd: { dif: [0], dea: [0], histogram: [0] },
      priceKdj: { k: [50], d: [50], j: [50] },
      candleKdj: { k: [50], d: [50], j: [50] },
    }), { status: 200 });
    return new Response(null, { status: 200 });
  }) as typeof fetch;
  let socket: WebSocket | null = null;
  const host = await createRemoteHost({} as SessionSetup, 1n, {
    baseUrl: "http://127.0.0.1:3000",
    fetchFn,
    webSocketFactory: () => {
      socket = { readyState: 1, close() {}, send() {}, onmessage: null, onerror: null, onclose: null } as unknown as WebSocket;
      return socket;
    },
  });
  await host.start(() => undefined);
  socket!.onmessage!({ data: JSON.stringify({ Baseline: { timeline_generation: 2, snapshot, civil_date: "2030-01-02", public_revision: 0, public_report_ids: [] } }) } as MessageEvent);
  assert.deepEqual(await host.playerWorkingOrders(), [{
    id: 7, code: "600000", side: "Buy", price: "1234", remainingQty: 200, venue: "continuous", frozen: "cash",
  }]);
  assert.equal((await host.calculateIndicators({ prices: [10], candles: [{ high: 11, low: 9, close: 10 }] })).macd.dif[0], 0);
  const request = requests.find(({ url }) => url.includes("/api/player-working-orders?"));
  assert.ok(request);
  assert.equal(new URL(request.url).searchParams.get("session_id"), "session-1");
  assert.equal(new URL(request.url).searchParams.get("generation"), "2");
  assert.equal(new Headers(request.init?.headers).get("authorization"), "Bearer token-1");
});

test("远程股票历史只通过显式 query endpoint 读取并绑定 generation", { timeout: 10000 }, async () => {
  const requests: { url: string; init?: RequestInit }[] = [];
  const fetchFn = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = input instanceof Request ? input.url : String(input);
    requests.push({ url, init });
    if (url.endsWith("/api/new")) return new Response(JSON.stringify({ session_id: "session-1", session_token: "token-1" }), { status: 200 });
    if (url.includes("/api/host-capabilities?")) return new Response(JSON.stringify({ npcDecisionDiagnostics: false }), { status: 200 });
    if (url.includes("/api/stock-history?")) return new Response(JSON.stringify({ generation: "2", data: { code: "600000", daily_candles: [], active_daily_candle: null } }), { status: 200 });
    return new Response(null, { status: 200 });
  }) as typeof fetch;
  let socket: WebSocket | null = null;
  const host = await createRemoteHost({} as SessionSetup, 1n, {
    baseUrl: "http://127.0.0.1:3000", fetchFn,
    webSocketFactory: () => {
      socket = { readyState: 1, close() {}, send() {}, onmessage: null, onerror: null, onclose: null } as unknown as WebSocket;
      return socket;
    },
  });
  await host.start(() => undefined);
  socket!.onmessage!({ data: JSON.stringify({ Baseline: { timeline_generation: 2, snapshot, civil_date: "2030-01-02", public_revision: 0, public_report_ids: [] } }) } as MessageEvent);
  assert.deepEqual(await host.queryStockHistory("600000"), { code: "600000", daily_candles: [], active_daily_candle: null });
  const request = requests.find(({ url }) => url.includes("/api/stock-history?"));
  assert.ok(request);
  const params = new URL(request.url).searchParams;
  assert.equal(params.get("generation"), "2");
  assert.equal(params.get("code"), "600000");
  assert.equal(params.has("account"), false);
  assert.equal(new Headers(request.init?.headers).get("authorization"), "Bearer token-1");
  await host.dispose();
});

test("Given a keyed day-end save, when RemoteHost sends it, then the exact candidate key reaches the authenticated save endpoint", async () => {
  const requests: { url: string; init?: RequestInit }[] = [];
  let completeSave: ((response: Response) => void) | null = null;
  let delaySave = false;
  const fetchFn = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = input instanceof Request ? input.url : String(input);
    requests.push({ url, init });
    if (url.endsWith("/api/new")) return new Response(JSON.stringify({ session_id: "session-1", session_token: "token-1" }), { status: 200 });
    if (url.includes("/api/host-capabilities?")) return new Response(JSON.stringify({ npcDecisionDiagnostics: false }), { status: 200 });
    if (url.endsWith("/api/save")) {
      if (delaySave) return new Promise<Response>((resolve) => { completeSave = resolve; });
      return new Response(JSON.stringify({ saved: true }), { status: 200 });
    }
    return new Response(null, { status: 200 });
  }) as typeof fetch;
  const socket = { readyState: 1, close() {}, send() {}, onmessage: null, onerror: null, onclose: null } as unknown as WebSocket;
  const host = await createRemoteHost({} as SessionSetup, 1n, {
    baseUrl: "http://127.0.0.1:3000",
    fetchFn,
    webSocketFactory: () => socket,
  });

  await host.start(() => undefined);
  socket.onmessage!({ data: JSON.stringify({ Baseline: { timeline_generation: 2, snapshot, civil_date: "2030-01-02", public_revision: 0, public_report_ids: [] } }) } as MessageEvent);
  await host.save({ seq: 42, settledDate: "2030-01-02" });

  const request = requests.find(({ url }) => url.endsWith("/api/save"));
  assert.ok(request);
  assert.deepEqual(JSON.parse(String(request.init?.body)), { session_id: "session-1", generation: "2", candidate: { seq: 42, settledDate: "2030-01-02" } });
  assert.equal(new Headers(request.init?.headers).get("authorization"), "Bearer token-1");
  delaySave = true;
  const pending = host.save();
  assert.deepEqual(JSON.parse(String(requests.at(-1)!.init?.body)), { session_id: "session-1", generation: "2" });
  socket.onmessage!({ data: JSON.stringify({ Baseline: { timeline_generation: 3, snapshot, civil_date: "2030-01-02", public_revision: 1, public_report_ids: [] } }) } as MessageEvent);
  completeSave!(new Response(JSON.stringify({ old: true }), { status: 200 }));
  await assert.rejects(pending, /generation/);
});

test("Given invalid indicators rejected by the server, when calculated remotely, then the host rejects instead of returning result arrays", async () => {
  const fetchFn = (async (input: string | URL | Request) => {
    const url = input instanceof Request ? input.url : String(input);
    if (url.endsWith("/api/new")) return new Response(JSON.stringify({ session_id: "session-1", session_token: "token-1" }), { status: 200 });
    if (url.includes("/api/host-capabilities?")) return new Response(JSON.stringify({ npcDecisionDiagnostics: false }), { status: 200 });
    if (url.endsWith("/api/indicators")) return new Response(JSON.stringify({ code: "INVALID_INDICATOR_INPUT", message: "candles[0]: high must be at least low" }), { status: 400 });
    return new Response(null, { status: 200 });
  }) as typeof fetch;
  const socket = { readyState: 1, close() {}, send() {}, onmessage: null, onerror: null, onclose: null } as unknown as WebSocket;
  const host = await createRemoteHost({} as SessionSetup, 1n, {
    baseUrl: "http://127.0.0.1:3000",
    fetchFn,
    webSocketFactory: () => socket,
  });

  await assert.rejects(host.calculateIndicators({ prices: [10] }), /INVALID_INDICATOR_INPUT/);
});

test("Given a remote host without a Publisher baseline, when player orders are queried, then it rejects without inventing generation one", async () => {
  const calls: string[] = [];
  const fetchFn = (async (input: string | URL | Request) => {
    const url = input instanceof Request ? input.url : String(input);
    calls.push(url);
    if (url.endsWith("/api/new")) return new Response(JSON.stringify({ session_id: "session-1", session_token: "token-1" }), { status: 200 });
    if (url.includes("/api/host-capabilities?")) return new Response(JSON.stringify({ npcDecisionDiagnostics: false }), { status: 200 });
    return new Response(JSON.stringify([]), { status: 200 });
  }) as typeof fetch;
  const socket = { readyState: 1, close() {}, send() {}, onmessage: null, onerror: null, onclose: null } as unknown as WebSocket;
  const host = await createRemoteHost({} as SessionSetup, 1n, { baseUrl: "http://127.0.0.1:3000", fetchFn, webSocketFactory: () => socket });
  await assert.rejects(host.playerWorkingOrders(), /基线尚未就绪/);
  await assert.rejects(host.save(), /基线尚未就绪/);
  assert.equal(calls.some((url) => url.includes("/api/player-working-orders?")), false);
});

test("Given a same-generation Publisher resync, when refreshBaseline completes, then it redelivers the authoritative baseline", async () => {
  const fetchFn = (async (input: string | URL | Request) => {
    const url = input instanceof Request ? input.url : String(input);
    if (url.endsWith("/api/new")) return new Response(JSON.stringify({ session_id: "session-1", session_token: "token-1" }), { status: 200 });
    if (url.includes("/api/host-capabilities?")) return new Response(JSON.stringify({ npcDecisionDiagnostics: false }), { status: 200 });
    return new Response(null, { status: 200 });
  }) as typeof fetch;
  let socket: WebSocket | null = null;
  const host = await createRemoteHost({} as SessionSetup, 1n, {
    baseUrl: "http://127.0.0.1:3000",
    fetchFn,
    webSocketFactory: () => {
      socket = { readyState: 1, close() {}, send() { queueMicrotask(() => socket?.onmessage?.({ data: JSON.stringify({ Baseline: { timeline_generation: 1, snapshot: { ...snapshot, seq: 3, tick: 3 }, civil_date: "2030-01-02", public_revision: 0, public_report_ids: [] } }) } as MessageEvent)); }, onmessage: null, onerror: null, onclose: null } as unknown as WebSocket;
      return socket;
    },
  });
  const received: unknown[] = [];
  await host.start((update) => { received.push(update); });
  socket!.onmessage!({ data: JSON.stringify({ Baseline: { timeline_generation: 1, snapshot, civil_date: "2030-01-02", public_revision: 0, public_report_ids: [] } }) } as MessageEvent);
  await host.refreshBaseline();
  assert.equal(received.length, 2);
  assert.equal((received[1] as { generation: string }).generation, "1");
  assert.equal((received[1] as { snapshot: { seq: number } }).snapshot.seq, 3);
});

test("Given a server without diagnostic support, when created, then RemoteHost negotiates it as disabled", async () => {
  const calls: string[] = [];
  const fetchFn = (async (input: string | URL | Request) => {
    const url = input instanceof Request ? input.url : String(input);
    calls.push(url);
    if (url.endsWith("/api/new")) return new Response(JSON.stringify({ session_id: "session-1", session_token: "token-1" }), { status: 200 });
    if (url.includes("/api/host-capabilities?")) return new Response(JSON.stringify({ npcDecisionDiagnostics: false }), { status: 200 });
    return new Response(null, { status: 200 });
  }) as typeof fetch;
  const socket = { readyState: 1, close() {}, send() {}, onmessage: null, onerror: null, onclose: null } as unknown as WebSocket;
  const host = await createRemoteHost({} as SessionSetup, 1n, { baseUrl: "http://127.0.0.1:3000", fetchFn, webSocketFactory: () => socket });
  assert.equal(host.capabilities.npcDecisionDiagnostics, false);
  assert.ok(host.npcDecisionTrace);
  await assert.rejects(host.npcDecisionTrace(1), /未协商启用/);
  assert.ok(calls.some((url) => url.includes("/api/host-capabilities?")));
});

test("Given remote pause preferences, when serialized for the authenticated current generation API, then both flags remain explicit", () => {
  assert.deepEqual(remotePausePreferencePayload("session-1", "2", {
    pause_after_close: true,
    pause_before_open: false,
  }), {
    session_id: "session-1",
    generation: "2",
    preferences: { pause_after_close: true, pause_before_open: false },
  });
});

test("Given remote session deletion fails, when disposed, then the host reports an explicit fatal failure", async () => {
  const failures: HostFailure[] = [];
  const fetchFn = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = input instanceof Request ? input.url : String(input);
    if (url.endsWith("/api/new")) {
      return new Response(JSON.stringify({ session_id: "session-1", session_token: "token-1" }), {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    }
    if (url.includes("/api/host-capabilities?")) return new Response(JSON.stringify({ npcDecisionDiagnostics: false }), { status: 200 });
    if (init?.method === "DELETE") return new Response("delete denied", { status: 500 });
    return new Response(null, { status: 200 });
  }) as typeof fetch;
  const socket = {
    readyState: 1,
    close() {},
    send() {},
    onmessage: null,
    onerror: null,
    onclose: null,
  } as unknown as WebSocket;
  const remote = await createRemoteHost({} as SessionSetup, 1n, {
    baseUrl: "http://127.0.0.1:3000",
    fetchFn,
    webSocketFactory: () => socket,
  });
  await remote.start(() => {}, (failure) => failures.push(failure));
  await assert.rejects(remote.dispose(), /HTTP 500/);
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(failures, [{
    code: "REMOTE_DISPOSE",
    where: "remote-host.dispose",
    message: "远程服务请求失败（HTTP 500）：delete denied",
  }]);
});

test("remote disposal authenticates the owning session before deletion", async () => {
  let deletion: RequestInit | undefined;
  const fetchFn = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = input instanceof Request ? input.url : String(input);
    if (url.endsWith("/api/new")) return new Response(JSON.stringify({ session_id: "owned", session_token: "owner-token" }));
    if (url.includes("/api/host-capabilities?")) return new Response(JSON.stringify({ npcDecisionDiagnostics: false }));
    if (init?.method === "DELETE") deletion = init;
    return new Response(null);
  }) as typeof fetch;
  const host = await createRemoteHost({} as SessionSetup, 1n, { fetchFn });
  await host.dispose();
  await new Promise<void>((resolve) => setImmediate(resolve));
  assert.ok(deletion);
  assert.equal(new Headers(deletion.headers).get("authorization"), "Bearer owner-token");
});
