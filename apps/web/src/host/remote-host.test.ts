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
import type { HostFailure, HostUpdate } from "./host-update.ts";

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
  static readonly OPEN = 1;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onopen: ((event: Event) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  onclose: ((event: CloseEvent) => void) | null = null;
  readyState = FakeWebSocket.OPEN;
  sent: string[] = [];

  send(value: string) {
    this.sent.push(value);
  }

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

function collectEventSeqs(target: number[]) {
  return (update: HostUpdate) => {
    if (update.type === "delta") target.push(...update.events.map(remoteEventSeq));
  };
}

function collectFailureMessages(target: string[]) {
  return (failure: HostFailure) => target.push(failure.message);
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

test("publisher frame declares its covered seq range so compacted market samples may contain gaps", () => {
  const frame = parseRemoteMessage(JSON.stringify({ PublisherFrame: {
    from_seq: 1,
    to_seq: 3,
    events: [{ PriceTick: { seq: 1 } }, { PriceTick: { seq: 3 } }],
  } }));
  assert.deepEqual(frame, {
    kind: "frame",
    fromSeq: 1,
    toSeq: 3,
    events: [{ PriceTick: { seq: 1 } }, { PriceTick: { seq: 3 } }],
    runtimeSnapshot: undefined,
  });
});

test("remote host can switch each client between server push and client pull", async () => {
  const sockets = [new FakeWebSocket(), new FakeWebSocket()];
  const urls: string[] = [];
  const fetchFn = (async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/api/new")) return jsonResponse({ session_id: "session-delivery" });
    if (url.includes("/api/snapshot")) return jsonResponse(SNAPSHOT);
    if (url.endsWith("/api/running")) return new Response(null, { status: 200 });
    if (url.includes("/api/session")) return new Response(null, { status: 204 });
    throw new Error(`unexpected request: ${url}`);
  }) as typeof fetch;
  let index = 0;
  const host = await createRemoteHost(DEFAULT_SETUP, 11n, {
    baseUrl: "http://server.test",
    fetchFn,
    webSocketFactory: (url) => {
      urls.push(url);
      return sockets[index++] as unknown as WebSocket;
    },
  });
  host.start(() => {});
  assert.equal(host.getDeliveryMode?.(), "push");
  host.setDeliveryMode?.("pull");
  assert.equal(host.getDeliveryMode?.(), "pull");
  assert.match(urls[0]!, /delivery=push/);
  assert.match(urls[1]!, /delivery=pull/);

  sockets[1]!.onopen?.(new Event("open"));
  assert.deepEqual(sockets[1]!.sent, [JSON.stringify({ GetFrame: {} })]);
  host.dispose();
});

test("resuming a remote host does not replace the established UI baseline", async () => {
  const sockets = [new FakeWebSocket(), new FakeWebSocket()];
  let socketIndex = 0;
  const fetchFn = (async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/api/new")) return jsonResponse({ session_id: "session-resume" });
    if (url.includes("/api/snapshot")) return jsonResponse(SNAPSHOT);
    if (url.endsWith("/api/running")) return new Response(null, { status: 200 });
    if (url.includes("/api/session")) return new Response(null, { status: 204 });
    throw new Error(`unexpected request: ${url}`);
  }) as typeof fetch;
  const host = await createRemoteHost(DEFAULT_SETUP, 12n, {
    baseUrl: "http://server.test",
    fetchFn,
    webSocketFactory: () => sockets[socketIndex++] as unknown as WebSocket,
  });
  const baselineSeqs: number[] = [];
  const receive = (update: HostUpdate) => {
    if (update.type === "baseline") baselineSeqs.push(update.snapshot.seq);
  };

  host.start(receive);
  host.stop();
  host.start(receive);

  assert.deepEqual(baselineSeqs, [0]);
  assert.equal(socketIndex, 2);
  host.dispose();
});

test("switching Publisher mode is not blocked by the old connection resync", async () => {
  const sockets = [new FakeWebSocket(), new FakeWebSocket()];
  let snapshotRequests = 0;
  const never = new Promise<Response>(() => {});
  const fetchFn = (async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/api/new")) return jsonResponse({ session_id: "session-generation-chain" });
    if (url.includes("/api/snapshot")) {
      snapshotRequests += 1;
      return snapshotRequests === 1 ? jsonResponse(SNAPSHOT) : never;
    }
    if (url.endsWith("/api/running")) return new Response(null, { status: 200 });
    if (url.includes("/api/session")) return new Response(null, { status: 204 });
    throw new Error(`unexpected request: ${url}`);
  }) as typeof fetch;
  let socketIndex = 0;
  const host = await createRemoteHost(DEFAULT_SETUP, 13n, {
    baseUrl: "http://server.test",
    fetchFn,
    webSocketFactory: () => sockets[socketIndex++] as unknown as WebSocket,
  });
  const delivered: number[] = [];
  host.start(collectEventSeqs(delivered));
  sockets[0]!.onmessage?.({ data: JSON.stringify({ ResyncRequired: { missed: 1 } }) } as MessageEvent);
  await new Promise((resolve) => setTimeout(resolve, 0));

  host.setDeliveryMode?.("pull");
  sockets[1]!.onmessage?.({ data: JSON.stringify({ PublisherFrame: {
    from_seq: 1,
    to_seq: 1,
    events: [{ PriceTick: { seq: 1 } }],
  } }) } as MessageEvent);
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.deepEqual(delivered, [1]);
  host.dispose();
});

test("a state-changing Publisher frame is rejected before it mutates the UI", async () => {
  const socket = new FakeWebSocket();
  const fetchFn = (async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/api/new")) return jsonResponse({ session_id: "session-invalid-frame" });
    if (url.includes("/api/snapshot")) return jsonResponse(SNAPSHOT);
    if (url.endsWith("/api/running")) return new Response(null, { status: 200 });
    if (url.includes("/api/session")) return new Response(null, { status: 204 });
    throw new Error(`unexpected request: ${url}`);
  }) as typeof fetch;
  const delivered: number[] = [];
  const fatal: string[] = [];
  const host = await createRemoteHost(DEFAULT_SETUP, 14n, {
    baseUrl: "http://server.test",
    fetchFn,
    webSocketFactory: () => socket as unknown as WebSocket,
  });
  host.start(
    collectEventSeqs(delivered),
    collectFailureMessages(fatal),
  );
  socket.onmessage?.({ data: JSON.stringify({ PublisherFrame: {
    from_seq: 1,
    to_seq: 1,
    events: [{ Trade: { seq: 1 } }],
  } }) } as MessageEvent);
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.deepEqual(delivered, []);
  assert.match(fatal[0]!, /缺少权威运行快照/);
  host.dispose();
});

test("an EngineUpdate with a future snapshot is rejected before it mutates the UI", async () => {
  const socket = new FakeWebSocket();
  const fetchFn = (async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/api/new")) return jsonResponse({ session_id: "session-future-snapshot" });
    if (url.includes("/api/snapshot")) return jsonResponse(SNAPSHOT);
    if (url.endsWith("/api/running")) return new Response(null, { status: 200 });
    if (url.includes("/api/session")) return new Response(null, { status: 204 });
    throw new Error(`unexpected request: ${url}`);
  }) as typeof fetch;
  const delivered: number[] = [];
  const fatal: string[] = [];
  const host = await createRemoteHost(DEFAULT_SETUP, 15n, {
    baseUrl: "http://server.test",
    fetchFn,
    webSocketFactory: () => socket as unknown as WebSocket,
  });
  host.start(
    collectEventSeqs(delivered),
    collectFailureMessages(fatal),
  );
  socket.onmessage?.({ data: JSON.stringify({ EngineUpdate: {
    events: [{ Trade: { seq: 1 } }],
    runtime_snapshot: { ...SNAPSHOT, seq: 2 },
  } }) } as MessageEvent);
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.deepEqual(delivered, []);
  assert.match(fatal[0]!, /快照 seq 必须等于最终事件/);
  host.dispose();
});

test("remote writes use the client gateway queue and resolve only after CommandQueued", async () => {
  const socket = new FakeWebSocket();
  const fetchFn = (async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/api/new")) return jsonResponse({ session_id: "session-write" });
    if (url.includes("/api/snapshot")) return jsonResponse(SNAPSHOT);
    if (url.endsWith("/api/running")) return new Response(null, { status: 200 });
    if (url.includes("/api/session")) return new Response(null, { status: 204 });
    throw new Error(`unexpected request: ${url}`);
  }) as typeof fetch;
  const host = await createRemoteHost(DEFAULT_SETUP, 12n, {
    baseUrl: "http://server.test",
    fetchFn,
    webSocketFactory: () => socket as unknown as WebSocket,
  });
  host.start(() => {});
  const intent = { PlaceLimit: { code: "600101", side: "Buy", price: 1_000, qty: 100 } } as const;
  let settled = false;
  const submitted = host.submitIntent(intent).then(() => { settled = true; });
  const command = JSON.parse(socket.sent.at(-1)!) as { SubmitIntent: { request_id: number } };
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(settled, false);
  socket.onmessage?.({ data: JSON.stringify({ CommandQueued: { request_id: command.SubmitIntent.request_id } }) } as MessageEvent);
  await submitted;
  assert.equal(settled, true);
  host.dispose();
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
  host.start((update) => {
    if (update.type === "delta") batches.push(update.events.map(remoteEventSeq));
  });

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
  host.start(collectEventSeqs(delivered), collectFailureMessages(fatal));

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

test("remote PublisherFrame delivers events and its authoritative snapshot as one atomic update", async () => {
  const socket = new FakeWebSocket();
  let snapshotRequests = 0;
  const fetchFn = (async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/api/new")) return jsonResponse({ session_id: "session-seq" });
    if (url.includes("/api/snapshot")) {
      snapshotRequests += 1;
      return jsonResponse(SNAPSHOT);
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
  const receivedUpdates: Array<{
    type: string;
    seqs: number[];
    snapshotSeq?: number;
    turnoverCents?: string;
  }> = [];
  host.start(
    (update) => {
      if (update.type === "delta") {
        deliveredSeqs.push(...update.events.map(remoteEventSeq));
        receivedUpdates.push({
          type: update.type,
          seqs: update.events.map(remoteEventSeq),
          snapshotSeq: update.runtimeSnapshot?.seq,
          turnoverCents:
            update.runtimeSnapshot?.active_daily_candles.AAA?.trade_stats?.turnover_cents,
        });
      } else {
        receivedUpdates.push({ type: update.type, seqs: [], snapshotSeq: update.snapshot.seq });
      }
    },
  );

  socket.onmessage?.({
    data: JSON.stringify({
      PublisherFrame: {
        from_seq: 1,
        to_seq: 3,
        events: [{ Trade: { seq: 1 } }, { PriceTick: { seq: 2 } }, { PriceTick: { seq: 3 } }],
        runtime_snapshot: {
          ...SNAPSHOT,
          seq: 3,
          tick: 3,
          active_daily_candles: {
            AAA: {
              time: 0,
              open: 1_000_000_000,
              high: 1_000_000_000,
              low: 1_000_000_000,
              close: 1_000_000_000,
              volume: 9_007_200,
              trade_stats: { turnover_cents: "9007200000000000", trade_count: 7 },
            },
          },
        },
      },
    }),
  } as MessageEvent);
  await new Promise((resolve) => setTimeout(resolve, 10));

  assert.deepEqual(deliveredSeqs, [1, 2, 3]);
  assert.deepEqual(receivedUpdates, [
    { type: "baseline", seqs: [], snapshotSeq: 0 },
    {
      type: "delta",
      seqs: [1, 2, 3],
      snapshotSeq: 3,
      turnoverCents: "9007200000000000",
    },
  ]);
  assert.equal(snapshotRequests, 1);
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
    (update) => {
      if (update.type === "delta") deliveredSeqs.push(...update.events.map(remoteEventSeq));
      else snapshotSeqs.push(update.snapshot.seq);
    },
  );

  sockets[0]!.onmessage?.({ data: JSON.stringify({ PriceTick: { seq: 1 } }) } as MessageEvent);
  await new Promise((resolve) => setTimeout(resolve, 0));
  await host.load({} as never);
  sockets[0]!.onmessage?.({ data: JSON.stringify({ Trade: { seq: 11 } }) } as MessageEvent);
  releaseOldRefresh(jsonResponse({ ...SNAPSHOT, seq: 2, tick: 2 }));
  await new Promise((resolve) => setTimeout(resolve, 10));

  assert.deepEqual(deliveredSeqs, [1]);
  assert.deepEqual(snapshotSeqs, [0, 10]);
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
  host.start((update) => {
    if (update.type === "baseline") snapshots.push(update.snapshot.seq);
  });

  await assert.rejects(host.load({} as never), /INVALID_SAVE: bad save/);

  assert.equal(socketIndex, 2);
  assert.equal(host.tick(), 4);
  assert.deepEqual(snapshots, [0, 4]);
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

  host.start(() => {}, collectFailureMessages(fatalMessages));
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

  host.start(() => {}, collectFailureMessages(fatalMessages));
  socket.onerror?.(new Event("error"));
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.equal(fatalMessages.length, 1);
  assert.match(fatalMessages[0]!, /^远程 WebSocket 连接发生错误；无法确认远程会话已暂停：/);
  assert.match(fatalMessages[0]!, /PAUSE_FAILED: actor unavailable/);
  host.dispose();
});
