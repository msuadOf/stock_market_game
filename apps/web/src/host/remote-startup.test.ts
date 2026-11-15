import { defaultCatalogFixture, catalogWireFixture } from "./catalog-test-fixture.ts";
import assert from "node:assert/strict";
import test from "node:test";
import type { SessionSetup } from "../types/engine.ts";
import { attachRemoteHost, createRemoteHost } from "./remote-host.ts";
import { parseProtocolSnapshot } from "./protocol/index.ts";
import { DEFAULT_SETUP } from "../config/defaults.ts";
import { mirrorOrigin } from "./protocol-test-fixtures.ts";

const snapshot = { seq: 4, tick: 0, day: 0, phase: "Continuous", mirror_origin: mirrorOrigin(0), markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} };

async function fixture(deliver: (socket: WebSocket) => void, loadResponse?: () => Promise<Response>) {
  const requests: { url: string; init?: RequestInit }[] = [];
  let connections = 0;
  let closes = 0;
  const socket = { readyState: 1, close() { closes += 1; }, send() {}, onmessage: null, onerror: null, onclose: null } as unknown as WebSocket;
  const fetchFn = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = input instanceof Request ? input.url : String(input);
    requests.push({ url, init });
    if (url.endsWith("/api/new")) return new Response(JSON.stringify({ session_id: "startup", session_token: "token" }));
    if (url.includes("/api/host-capabilities?")) return new Response(JSON.stringify({ npcDecisionDiagnostics: false }));
    if (url.endsWith("/api/load") && loadResponse) return await loadResponse();
    return new Response(null);
  }) as typeof fetch;
  const host = await createRemoteHost({} as SessionSetup, 7n, {
    baseUrl: "http://127.0.0.1:3000", fetchFn,
    webSocketFactory: () => {
      connections += 1;
      queueMicrotask(() => deliver(socket));
      return socket;
    },
  });
  return { host, socket, requests, connections: () => connections, closes: () => closes };
}

test("remote startup restores and caches the authoritative baseline without running the market", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const { host, requests, connections, closes } = await fixture((socket) => socket.onmessage?.({ data: JSON.stringify({ Baseline: { ...catalogWireFixture(snapshot),
    timeline_generation: 2, snapshot, civil_date: "2030-01-02", public_revision: 0, public_report_ids: [],
  } }) } as MessageEvent));
  try {
    await host.load({ daily: true });
    assert.deepEqual(host.snapshot(), parseProtocolSnapshot(snapshot, "expected baseline", defaultCatalogFixture));
    assert.equal(connections(), 1);
    assert.equal(requests.some(({ url }) => url.endsWith("/api/running")), false);
    const received: unknown[] = [];
    await host.start((update) => { received.push(update); });
    assert.equal(connections(), 1);
    assert.equal((received[0] as { generation: string }).generation, "2");
    await host.setSpeed(Infinity);
    await host.stop();
    const controls = requests.filter(({ url }) => /\/api\/(running|speed)$/.test(url)).map(({ init }) => JSON.parse(String(init?.body)));
    assert.deepEqual(controls, [
      { session_id: "startup", generation: "2", running: true },
      { session_id: "startup", generation: "2", speed: "Fastest" },
      { session_id: "startup", generation: "2", running: false },
    ]);
    assert.equal(requests.filter(({ url }) => url.endsWith("/api/load")).length, 1);
    context.mock.timers.tick(5000);
    assert.equal(closes(), 0, "successful restoration must clear its timeout");
  } finally { await host.dispose(); }
});

test("remote startup connection loss rejects restoration even while the market is paused", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const { host, closes } = await fixture((socket) => { socket.close(); socket.onclose?.({} as CloseEvent); });
  try {
    await assert.rejects(host.load({ daily: true }), /连接中断.*结果未知/);
    assert.equal(closes(), 2);
  }
  finally { await host.dispose(); }
});

test("remote startup without a baseline times out and closes its connection", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const { host, closes, connections } = await fixture(() => {});
  try {
    const loaded = host.load({ daily: true });
    const rejected = assert.rejects(loaded, /REMOTE_BASELINE_TIMEOUT.*5000ms/);
    await new Promise<void>((resolve) => setImmediate(resolve));
    assert.equal(connections(), 1);
    context.mock.timers.tick(4999);
    assert.equal(closes(), 0);
    context.mock.timers.tick(1);
    await rejected;
    assert.equal(closes(), 1);
    context.mock.timers.tick(5000);
    assert.equal(closes(), 1);
  } finally { await host.dispose(); }
});

test("disposed remote startup cannot reconnect after a late load response", async () => {
  let finish: (() => void) | undefined;
  const { host, connections } = await fixture(() => {}, () => new Promise<Response>((resolve) => { finish = () => resolve(new Response(null)); }));
  const loaded = host.load({ daily: true });
  assert.ok(finish);
  const rejected = assert.rejects(loaded, /销毁/);
  await host.dispose();
  finish();
  await rejected;
  assert.equal(connections(), 0);
});

test("已登录共享市场连接不创建、启动或删除市场，并通过 REST 和 WS 使用同一身份", async () => {
  const token = "f".repeat(64);
  const requests: { url: string; init?: RequestInit }[] = [];
  const socket = { readyState: 1, close() {}, send() {}, onmessage: null, onerror: null, onclose: null } as unknown as WebSocket;
  const host = await attachRemoteHost("http://127.0.0.1:3000", token, {
    session_id: "existing", generation: "2", setup: DEFAULT_SETUP, seed: "7", resumed: false,
    can_control: false, member: null, needs_rejoin: true,
  }, {
    fetchFn: (async (input, init) => {
      const url = String(input);
      requests.push({ url, init });
      return Response.json({ npcDecisionDiagnostics: false });
    }) as typeof fetch,
    webSocketFactory: (url, protocols) => {
      assert.match(url, /session_id=existing/);
      assert.equal(protocols[1], "stock-game.auth." + "66".repeat(64));
      queueMicrotask(() => socket.onmessage?.({ data: JSON.stringify({ Baseline: { ...catalogWireFixture(snapshot),
        timeline_generation: 3, snapshot, civil_date: "2030-01-02", public_revision: 0, public_report_ids: [],
      } }) } as MessageEvent));
      return socket;
    },
  });
  const generations: string[] = [];
  await host.subscribeUpdates((update) => { if (update.type === "baseline") generations.push(update.generation); });
  await host.dispose();
  assert.deepEqual(generations, ["3"]);
  assert.deepEqual(requests.map(({ url, init }) => [new URL(url).pathname, init?.method, new Headers(init?.headers).get("authorization")]),
    [["/api/host-capabilities", "GET", "Bearer " + token]]);
});
