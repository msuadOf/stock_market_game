import assert from "node:assert/strict";
import test from "node:test";
import { DEFAULT_SETUP } from "../config/defaults.ts";
import {
  createRemoteHost,
  parseRemoteMessage,
  remoteEventSeq,
  remoteSpeedValue,
} from "./remote-host.ts";
import { parseSpeedMetrics } from "./speed.ts";

const SNAPSHOT = {
  seq: 0,
  tick: 0,
  day: 0,
  phase: "Continuous",
  markets: {},
  accounts: {},
  daily_candles: {},
  active_daily_candles: {},
};

class FakeWebSocket {
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  onclose: ((event: CloseEvent) => void) | null = null;

  close() {
    this.onclose?.(new Event("close") as CloseEvent);
  }
}

function jsonResponse(value: unknown, status = 200): Response {
  return new Response(JSON.stringify(value), {
    status,
    headers: { "content-type": "application/json" },
  });
}

test("remote protocol distinguishes baseline snapshots, events, and resync signals", () => {
  const snapshot = parseRemoteMessage(JSON.stringify({
    seq: 4,
    tick: 4,
    day: 0,
    phase: "Continuous",
    markets: {},
    accounts: {},
    daily_candles: {},
    active_daily_candles: {},
  }));
  assert.equal(snapshot.kind, "snapshot");

  const event = parseRemoteMessage(JSON.stringify({ DayBoundary: {
    seq: 5,
    day: 1,
    closed_daily_candles: {},
  } }));
  assert.equal(event.kind, "event");
  if (event.kind === "event") assert.equal(remoteEventSeq(event.event), 5);

  assert.deepEqual(
    parseRemoteMessage(JSON.stringify({ ResyncRequired: { reason: "event_stream_lagged", missed: 3 } })),
    { kind: "resync", missed: 3 },
  );
});

test("remote protocol transports one host update as an atomic event batch with its runtime snapshot", () => {
  const update = parseRemoteMessage(JSON.stringify({ EngineUpdate: {
    events: [
      { Trade: { seq: 1 } },
      { PriceTick: { seq: 2 } },
    ],
    runtime_snapshot: { ...SNAPSHOT, seq: 2, tick: 1 },
  } }));

  assert.equal(update.kind, "update");
  if (update.kind === "update") {
    assert.deepEqual(update.events.map(remoteEventSeq), [1, 2]);
    assert.equal(update.runtimeSnapshot?.seq, 2);
  }
});

test("remote update rejects a runtime snapshot older than its final event", () => {
  assert.throws(() => parseRemoteMessage(JSON.stringify({ EngineUpdate: {
    events: [{ PriceTick: { seq: 2 } }],
    runtime_snapshot: { ...SNAPSHOT, seq: 1 },
  } })), /快照 seq/);
});

test("remote host delivers a websocket update as one application-layer event batch", async () => {
  const socket = new FakeWebSocket();
  const fetchFn = (async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/api/new")) return jsonResponse({ session_id: "session-batch" });
    if (url.includes("/api/snapshot")) return jsonResponse(SNAPSHOT);
    if (url.endsWith("/api/running")) return new Response(null, { status: 200 });
    if (url.includes("/api/session")) return new Response(null, { status: 204 });
    throw new Error(`unexpected request: ${url}`);
  }) as typeof fetch;
  const host = await createRemoteHost(DEFAULT_SETUP, 9n, {
    baseUrl: "http://server.test",
    fetchFn,
    webSocketFactory: () => socket as unknown as WebSocket,
  });
  const batches: number[][] = [];
  host.start((events) => batches.push(events.map(remoteEventSeq)));

  socket.onmessage?.({
    data: JSON.stringify({ EngineUpdate: {
      events: [{ PriceTick: { seq: 1 } }, { PriceTick: { seq: 2 } }],
    } }),
  } as MessageEvent);
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.deepEqual(batches, [[1, 2]]);
  host.dispose();
});

test("remote host ignores an old queued update after resync and accepts the next batch", async () => {
  const socket = new FakeWebSocket();
  let snapshotRequests = 0;
  const fetchFn = (async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/api/new")) return jsonResponse({ session_id: "session-resync-batch" });
    if (url.includes("/api/snapshot")) {
      snapshotRequests += 1;
      return jsonResponse(snapshotRequests === 1 ? SNAPSHOT : { ...SNAPSHOT, seq: 100, tick: 100 });
    }
    if (url.endsWith("/api/running")) return new Response(null, { status: 200 });
    if (url.includes("/api/session")) return new Response(null, { status: 204 });
    throw new Error(`unexpected request: ${url}`);
  }) as typeof fetch;
  const host = await createRemoteHost(DEFAULT_SETUP, 10n, {
    baseUrl: "http://server.test",
    fetchFn,
    webSocketFactory: () => socket as unknown as WebSocket,
  });
  const delivered: number[] = [];
  const fatal: string[] = [];
  host.start((events) => delivered.push(...events.map(remoteEventSeq)), undefined, (message) => fatal.push(message));

  socket.onmessage?.({ data: JSON.stringify({ ResyncRequired: { missed: 10 } }) } as MessageEvent);
  await new Promise((resolve) => setTimeout(resolve, 0));
  socket.onmessage?.({ data: JSON.stringify({ EngineUpdate: {
    events: [{ Trade: { seq: 90 } }],
    runtime_snapshot: { ...SNAPSHOT, seq: 90, tick: 90 },
  } }) } as MessageEvent);
  socket.onmessage?.({ data: JSON.stringify({ EngineUpdate: {
    events: [{ PriceTick: { seq: 101 } }],
  } }) } as MessageEvent);
  await new Promise((resolve) => setTimeout(resolve, 10));

  assert.deepEqual(fatal, []);
  assert.deepEqual(delivered, [101]);
  host.dispose();
});

test("remote protocol rejects unknown variants and unsafe sequence integers", () => {
  assert.throws(() => parseRemoteMessage('{"Unknown":{"seq":1}}'), /不是已知/);
  assert.throws(
    () => parseRemoteMessage(JSON.stringify({ PriceTick: { seq: Number.MAX_SAFE_INTEGER + 1 } })),
    /安全整数/,
  );
  assert.throws(
    () => parseRemoteMessage(JSON.stringify({
      ...SNAPSHOT,
      markets: {
        "600101": { bids: [[1_000, Number.MAX_SAFE_INTEGER + 1]], asks: [] },
      },
    })),
    /不是已知/,
  );
});

test("remote fastest speed uses the unbounded JSON-safe server mode", () => {
  assert.equal(remoteSpeedValue(Infinity), "Fastest");
  assert.equal(remoteSpeedValue(720), 720);
  assert.throws(() => remoteSpeedValue(0), /非法速度/);
});

test("all host speed metrics validate the authoritative measurement contract", () => {
  assert.deepEqual(parseSpeedMetrics({
    requested: { mode: "fastest" },
    actual_multiplier: 843.25,
    sample_duration_ms: 1_002,
    sample_ticks: 845,
    running: true,
  }), {
    requested: { mode: "fastest" },
    actual_multiplier: 843.25,
    sample_duration_ms: 1_002,
    sample_ticks: 845,
    running: true,
  });
  assert.throws(() => parseSpeedMetrics({
    requested: { mode: "fixed", multiplier: 60 },
    actual_multiplier: -1,
    sample_duration_ms: 1_000,
    sample_ticks: 60,
    running: true,
  }), /actual_multiplier/);
});

test("remote host reads speed metrics from its authoritative server session", async () => {
  const socket = new FakeWebSocket();
  const fetchFn = (async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/api/new")) return jsonResponse({ session_id: "session-speed" });
    if (url.includes("/api/snapshot")) return jsonResponse(SNAPSHOT);
    if (url.includes("/api/speed?session_id=session-speed")) {
      return jsonResponse({
        requested: { mode: "fixed", multiplier: 60 },
        actual_multiplier: 59.8,
        sample_duration_ms: 1_003,
        sample_ticks: 60,
        running: true,
      });
    }
    if (url.includes("/api/session")) return new Response(null, { status: 204 });
    throw new Error(`unexpected request: ${url}`);
  }) as typeof fetch;
  const host = await createRemoteHost(DEFAULT_SETUP, 7n, {
    baseUrl: "http://server.test",
    fetchFn,
    webSocketFactory: () => socket as unknown as WebSocket,
  });

  assert.deepEqual(await host.readSpeedMetrics(), {
    requested: { mode: "fixed", multiplier: 60 },
    actual_multiplier: 59.8,
    sample_duration_ms: 1_003,
    sample_ticks: 60,
    running: true,
  });
  host.dispose();
});

test("remote snapshot refresh waits for queued websocket events before advancing the visible baseline", async () => {
  const socket = new FakeWebSocket();
  let snapshotRequests = 0;
  const fetchFn = (async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/api/new")) return jsonResponse({ session_id: "session-seq" });
    if (url.includes("/api/snapshot")) {
      snapshotRequests += 1;
      return jsonResponse({ ...SNAPSHOT, seq: snapshotRequests === 1 ? 0 : 3, tick: 3 });
    }
    if (url.endsWith("/api/running")) return new Response(null, { status: 200 });
    if (url.includes("/api/session")) return new Response(null, { status: 204 });
    throw new Error(`unexpected request: ${url}`);
  }) as typeof fetch;
  const host = await createRemoteHost(DEFAULT_SETUP, 3n, {
    baseUrl: "http://server.test",
    fetchFn,
    webSocketFactory: () => socket as unknown as WebSocket,
  });
  const deliveredSeqs: number[] = [];
  const snapshotSeqs: number[] = [];
  host.start(
    (events) => deliveredSeqs.push(...events.map(remoteEventSeq)),
    (snapshot) => snapshotSeqs.push(snapshot.seq),
  );

  for (const payload of [
    { Trade: { seq: 1 } },
    { PriceTick: { seq: 2 } },
    { PriceTick: { seq: 3 } },
  ]) {
    socket.onmessage?.({ data: JSON.stringify(payload) } as MessageEvent);
  }
  await new Promise((resolve) => setTimeout(resolve, 10));

  assert.deepEqual(deliveredSeqs, [1, 2, 3]);
  assert.deepEqual(snapshotSeqs, [3]);
  host.dispose();
});

test("remote load invalidates queued events and refreshes from the previous connection", async () => {
  const sockets = [new FakeWebSocket(), new FakeWebSocket()];
  let socketIndex = 0;
  let snapshotRequests = 0;
  let releaseOldRefresh!: (response: Response) => void;
  const oldRefresh = new Promise<Response>((resolve) => {
    releaseOldRefresh = resolve;
  });
  const fetchFn = (async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/api/new")) return jsonResponse({ session_id: "session-load" });
    if (url.includes("/api/snapshot")) {
      snapshotRequests += 1;
      if (snapshotRequests === 1) return jsonResponse(SNAPSHOT);
      return await oldRefresh;
    }
    if (url.endsWith("/api/load")) return jsonResponse({ ...SNAPSHOT, seq: 10, tick: 10 });
    if (url.endsWith("/api/running")) return new Response(null, { status: 200 });
    if (url.includes("/api/session")) return new Response(null, { status: 204 });
    throw new Error(`unexpected request: ${url}`);
  }) as typeof fetch;
  const host = await createRemoteHost(DEFAULT_SETUP, 4n, {
    baseUrl: "http://server.test",
    fetchFn,
    webSocketFactory: () => sockets[socketIndex++] as unknown as WebSocket,
  });
  const deliveredSeqs: number[] = [];
  const snapshotSeqs: number[] = [];
  host.start(
    (events) => deliveredSeqs.push(...events.map(remoteEventSeq)),
    (snapshot) => snapshotSeqs.push(snapshot.seq),
  );

  sockets[0]!.onmessage?.({ data: JSON.stringify({ PriceTick: { seq: 1 } }) } as MessageEvent);
  await new Promise((resolve) => setTimeout(resolve, 0));
  await host.load({} as never);
  sockets[0]!.onmessage?.({ data: JSON.stringify({ Trade: { seq: 11 } }) } as MessageEvent);
  releaseOldRefresh(jsonResponse({ ...SNAPSHOT, seq: 2, tick: 2 }));
  await new Promise((resolve) => setTimeout(resolve, 10));

  assert.deepEqual(deliveredSeqs, [1]);
  assert.deepEqual(snapshotSeqs, [10]);
  assert.equal(socketIndex, 2);
  assert.equal(host.tick(), 10);
  host.dispose();
});

test("remote load failure reconnects and refreshes the unchanged authoritative session", async () => {
  const sockets = [new FakeWebSocket(), new FakeWebSocket()];
  let socketIndex = 0;
  let snapshotRequests = 0;
  const fetchFn = (async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/api/new")) return jsonResponse({ session_id: "session-load-failure" });
    if (url.includes("/api/snapshot")) {
      snapshotRequests += 1;
      return jsonResponse(snapshotRequests === 1 ? SNAPSHOT : { ...SNAPSHOT, seq: 4, tick: 4 });
    }
    if (url.endsWith("/api/load")) {
      return jsonResponse({ code: "INVALID_SAVE", message: "bad save" }, 400);
    }
    if (url.endsWith("/api/running")) return new Response(null, { status: 200 });
    if (url.includes("/api/session")) return new Response(null, { status: 204 });
    throw new Error(`unexpected request: ${url}`);
  }) as typeof fetch;
  const host = await createRemoteHost(DEFAULT_SETUP, 4n, {
    baseUrl: "http://server.test",
    fetchFn,
    webSocketFactory: () => sockets[socketIndex++] as unknown as WebSocket,
  });
  const snapshots: number[] = [];
  host.start(() => {}, (snapshot) => snapshots.push(snapshot.seq));

  await assert.rejects(host.load({} as never), /INVALID_SAVE: bad save/);

  assert.equal(socketIndex, 2);
  assert.equal(host.tick(), 4);
  assert.deepEqual(snapshots, [4]);
  host.dispose();
});

test("remote malformed load snapshot also reconnects to the unchanged session", async () => {
  const sockets = [new FakeWebSocket(), new FakeWebSocket()];
  let socketIndex = 0;
  let snapshotRequests = 0;
  let releaseRecovery!: () => void;
  const recoveryGate = new Promise<void>((resolve) => {
    releaseRecovery = resolve;
  });
  const fetchFn = (async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/api/new")) return jsonResponse({ session_id: "session-malformed-load" });
    if (url.includes("/api/snapshot")) {
      snapshotRequests += 1;
      if (snapshotRequests === 1) return jsonResponse(SNAPSHOT);
      await recoveryGate;
      return jsonResponse({ ...SNAPSHOT, seq: 6, tick: 6 });
    }
    if (url.endsWith("/api/load")) return jsonResponse({ malformed: true });
    if (url.endsWith("/api/running")) return new Response(null, { status: 200 });
    if (url.includes("/api/session")) return new Response(null, { status: 204 });
    throw new Error(`unexpected request: ${url}`);
  }) as typeof fetch;
  const host = await createRemoteHost(DEFAULT_SETUP, 4n, {
    baseUrl: "http://server.test",
    fetchFn,
    webSocketFactory: () => sockets[socketIndex++] as unknown as WebSocket,
  });
  host.start(() => {});

  const loading = host.load({} as never);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(socketIndex, 1, "recovery must establish its HTTP baseline before reconnecting WS");
  releaseRecovery();
  await assert.rejects(loading, /无效的读档快照/);

  assert.equal(socketIndex, 2);
  assert.equal(host.tick(), 6);
  host.dispose();
});

test("an older failed load recovery cannot invalidate a newer successful load", async () => {
  const sockets = [new FakeWebSocket(), new FakeWebSocket()];
  let socketIndex = 0;
  let loadRequests = 0;
  let snapshotRequests = 0;
  let releaseOldRecovery!: (response: Response) => void;
  const oldRecovery = new Promise<Response>((resolve) => {
    releaseOldRecovery = resolve;
  });
  const fetchFn = (async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/api/new")) return jsonResponse({ session_id: "session-concurrent-load" });
    if (url.includes("/api/snapshot")) {
      snapshotRequests += 1;
      if (snapshotRequests === 1) return jsonResponse(SNAPSHOT);
      return await oldRecovery;
    }
    if (url.endsWith("/api/load")) {
      loadRequests += 1;
      return loadRequests === 1
        ? jsonResponse({ malformed: true })
        : jsonResponse({ ...SNAPSHOT, seq: 10, tick: 10 });
    }
    if (url.endsWith("/api/running")) return new Response(null, { status: 200 });
    if (url.includes("/api/session")) return new Response(null, { status: 204 });
    throw new Error(`unexpected request: ${url}`);
  }) as typeof fetch;
  const host = await createRemoteHost(DEFAULT_SETUP, 4n, {
    baseUrl: "http://server.test",
    fetchFn,
    webSocketFactory: () => sockets[socketIndex++] as unknown as WebSocket,
  });
  host.start(() => {});

  const firstLoad = host.load({} as never);
  await new Promise((resolve) => setTimeout(resolve, 0));
  await host.load({} as never);
  releaseOldRecovery(jsonResponse({ ...SNAPSHOT, seq: 5, tick: 5 }));
  await assert.rejects(firstLoad, /无效的读档快照/);

  assert.equal(socketIndex, 2);
  assert.equal(host.tick(), 10);
  host.dispose();
});

test("remote runtime failure pauses the authoritative session before reporting it paused", async () => {
  const socket = new FakeWebSocket();
  const runningRequests: boolean[] = [];
  const fetchFn = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/api/new")) return jsonResponse({ session_id: "session-1" });
    if (url.includes("/api/snapshot")) return jsonResponse(SNAPSHOT);
    if (url.endsWith("/api/running")) {
      const body = JSON.parse(String(init?.body)) as { running: boolean };
      runningRequests.push(body.running);
      return new Response(null, { status: 200 });
    }
    if (url.includes("/api/session")) return new Response(null, { status: 204 });
    throw new Error(`unexpected request: ${url}`);
  }) as typeof fetch;
  const host = await createRemoteHost(DEFAULT_SETUP, 1n, {
    baseUrl: "http://server.test",
    fetchFn,
    webSocketFactory: () => socket as unknown as WebSocket,
  });
  const fatalMessages: string[] = [];

  host.start(() => {}, undefined, (message) => fatalMessages.push(message));
  socket.onerror?.(new Event("error"));
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.deepEqual(runningRequests, [true, false]);
  assert.deepEqual(fatalMessages, ["远程 WebSocket 连接发生错误；远程会话已暂停"]);
  host.dispose();
});

test("remote runtime failure does not claim pause when the pause request fails", async () => {
  const socket = new FakeWebSocket();
  const fetchFn = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/api/new")) return jsonResponse({ session_id: "session-2" });
    if (url.includes("/api/snapshot")) return jsonResponse(SNAPSHOT);
    if (url.endsWith("/api/running")) {
      const body = JSON.parse(String(init?.body)) as { running: boolean };
      return body.running
        ? new Response(null, { status: 200 })
        : jsonResponse({ code: "PAUSE_FAILED", message: "actor unavailable" }, 503);
    }
    if (url.includes("/api/session")) return new Response(null, { status: 204 });
    throw new Error(`unexpected request: ${url}`);
  }) as typeof fetch;
  const host = await createRemoteHost(DEFAULT_SETUP, 2n, {
    baseUrl: "http://server.test",
    fetchFn,
    webSocketFactory: () => socket as unknown as WebSocket,
  });
  const fatalMessages: string[] = [];

  host.start(() => {}, undefined, (message) => fatalMessages.push(message));
  socket.onerror?.(new Event("error"));
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.equal(fatalMessages.length, 1);
  assert.match(fatalMessages[0]!, /^远程 WebSocket 连接发生错误；无法确认远程会话已暂停：/);
  assert.match(fatalMessages[0]!, /PAUSE_FAILED: actor unavailable/);
  host.dispose();
});
