import assert from "node:assert/strict";
import test from "node:test";
import { DEFAULT_SETUP } from "../config/defaults.ts";
import { createRemoteHost, parseRemoteMessage, remoteEventSeq, remoteSpeedValue } from "./remote-host.ts";

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

test("remote protocol rejects unknown variants and unsafe sequence integers", () => {
  assert.throws(() => parseRemoteMessage('{"Unknown":{"seq":1}}'), /不是已知/);
  assert.throws(
    () => parseRemoteMessage(JSON.stringify({ PriceTick: { seq: Number.MAX_SAFE_INTEGER + 1 } })),
    /安全整数/,
  );
});

test("remote fastest speed uses the bounded JSON-safe server mode", () => {
  assert.equal(remoteSpeedValue(Infinity), "Fastest");
  assert.equal(remoteSpeedValue(720), 720);
  assert.throws(() => remoteSpeedValue(0), /非法速度/);
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
