import assert from "node:assert/strict";
import test from "node:test";
import type { Intent } from "../types/engine.ts";
import { DEFAULT_SETUP } from "../config/defaults.ts";
import type { HostFailure, HostUpdate } from "./host-update.ts";
import { remoteTestContext } from "./remote-test-context.ts";
import { createRemoteHost } from "./remote-host.ts";

const snapshot = { seq: 0, tick: 0, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} };
const baseline = (generation = 1) => ({ Baseline: { timeline_generation: generation, snapshot, civil_date: "2030-01-02", public_revision: 0, public_report_ids: [] } });
const protocol = (generation = 1) => ({ PublisherFrame: { timeline_generation: generation, update: { TickBatch: { frames: [], runtime_snapshot: null } }, civil_date: "2030-01-02", public_revision: 0 } });
const intent = {} as Intent;

class FakeSocket {
  readyState = 1;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: (() => void) | null = null;
  sent: unknown[] = [];
  closed = 0;
  sendError: Error | null = null;
  send(raw: string) {
    if (this.sendError !== null) throw this.sendError;
    this.sent.push(JSON.parse(raw));
  }
  close() { this.closed += 1; this.onclose?.(); }
  receive(value: unknown) { this.onmessage?.({ data: JSON.stringify(value) } as MessageEvent); }
}

async function fixture(onUpdate?: (update: HostUpdate) => void, respond?: (url: URL) => Response | Promise<Response>, start = true) {
  const sockets: FakeSocket[] = [];
  const failures: HostFailure[] = [];
  const received: HostUpdate[] = [];
  const host = await createRemoteHost(DEFAULT_SETUP, 1n, { token: "token", context: remoteTestContext("session", "1", "1"),
    baseUrl: "https://remote.example",
    fetchFn: async (input) => {
      const url = new URL(String(input));
      if (url.pathname === "/api/market/context") return Response.json(remoteTestContext("session", "2"));
      if (url.pathname === "/api/host-capabilities") return Response.json({ npcDecisionDiagnostics: respond !== undefined, indicatorCapabilities: { intradayAverage: true, macd: true, priceKdj: true, candleKdj: true }, personalTradeHistory: true });
      return respond === undefined ? Response.json({}) : respond(url);
    },
    webSocketFactory: () => {
      const socket = new FakeSocket();
      sockets.push(socket);
      return socket as unknown as WebSocket;
    },
  });
  if (start) await host.start((update) => { received.push(update); onUpdate?.(update); }, (failure) => failures.push(failure));
  return { host, sockets, failures, received, socket: sockets[0]! };
}

function commandId(socket: FakeSocket, position: number): number {
  return (socket.sent[position] as { SubmitIntent: { request_id: number } }).SubmitIntent.request_id;
}

const options = { timeout: 10000, concurrency: true };

test("Remote SubmitIntent 严格携带权威 generation，不用默认值且旧代确认拒绝", options, async () => {
  const { host, socket } = await fixture();
  await assert.rejects(host.submitIntent(intent), /尚未完成权威基线同步/);
  assert.deepEqual(socket.sent, []);
  socket.receive(baseline());
  const pending = host.submitIntent(intent);
  const rejected = assert.rejects(pending, /已过期.*generation/);
  socket.receive(baseline(2));
  await new Promise<void>((resolve) => setImmediate(resolve));
  socket.receive({ CommandQueued: { request_id: commandId(socket, 0) } });
  await rejected;
  assert.deepEqual(socket.sent[0], { SubmitIntent: { request_id: 1, generation: "1", intent } });
  const current = host.submitIntent(intent);
  socket.receive({ CommandQueued: { request_id: commandId(socket, 1) } });
  await current;
  assert.deepEqual(socket.sent[1], { SubmitIntent: { request_id: 2, generation: "2", intent } });
  await host.dispose();
});

test("并发 remote command 逆序确认只完成对应 request，重复确认显式失败", options, async () => {
  const { host, socket, failures } = await fixture();
  socket.receive(baseline());
  const settled: number[] = [];
  const first = host.submitIntent(intent).then(() => settled.push(1));
  const second = host.submitIntent(intent).then(() => settled.push(2));
  const firstId = commandId(socket, 0);
  const secondId = commandId(socket, 1);
  assert.equal(secondId, firstId + 1);
  socket.receive({ CommandQueued: { request_id: secondId } });
  await second;
  assert.deepEqual(settled, [2]);
  socket.receive({ CommandQueued: { request_id: firstId } });
  await first;
  socket.receive({ CommandQueued: { request_id: firstId } });
  assert.match(failures[0]!.message, /未知写请求.*入队确认/);
});

test("request gateway error 只拒绝对应 command，HostFailure 批量拒绝其余 command", options, async () => {
  const { host, socket, failures } = await fixture();
  socket.receive(baseline());
  const first = host.submitIntent(intent);
  const second = host.submitIntent(intent);
  const third = host.submitIntent(intent);
  const rejectedFirst = assert.rejects(first, /REJECTED: 请求失败/);
  const rejectedSecond = assert.rejects(second, /BROKEN: 会话失败/);
  const rejectedThird = assert.rejects(third, /BROKEN: 会话失败/);
  socket.receive({ GatewayError: { request_id: commandId(socket, 0), code: "REJECTED", message: "请求失败" } });
  await rejectedFirst;
  assert.equal(failures.length, 0);
  socket.receive({ HostFailure: { code: "BROKEN", message: "会话失败" } });
  await Promise.all([rejectedSecond, rejectedThird]);
  assert.equal(socket.closed, 1);
  assert.equal(host.snapshot().seq, 0);
});

test("mode 切换丢弃旧 socket message/close，已同步的新连接沿用 protocol gate", options, async () => {
  const { host, socket, sockets, received, failures } = await fixture();
  assert.ok(host.setDeliveryMode);
  assert.ok(host.getDeliveryMode);
  socket.receive(baseline());
  host.setDeliveryMode("pull");
  const replacement = sockets[1]!;
  socket.receive(baseline(2));
  await new Promise<void>((resolve) => setImmediate(resolve));
  socket.onclose?.();
  replacement.receive(protocol());
  assert.deepEqual(received.map((update) => update.type), ["baseline"]);
  replacement.receive(baseline());
  replacement.receive(protocol());
  assert.deepEqual(received.map((update) => update.type), ["baseline", "baseline", "protocol"]);
  assert.equal(host.getDeliveryMode(), "pull");
  assert.equal(failures.length, 0);
});

test("旧 socket onerror 不关闭新连接，恢复前仍保留 baseline gate", options, async () => {
  const { host, socket, sockets, failures } = await fixture();
  assert.ok(host.setDeliveryMode);
  socket.receive(baseline());
  host.setDeliveryMode("pull");
  socket.onerror?.();
  assert.equal(sockets[1]!.closed, 0);
  assert.equal(failures.length, 0);
  assert.equal(host.tick(), 0);
  await assert.rejects(host.submitIntent(intent), /尚未完成权威基线同步/);
});

test("并发 resync 共享权威 baseline，所有 waiter 均完成", options, async () => {
  const { host, socket } = await fixture();
  socket.receive(baseline());
  let firstSettled = false;
  void host.refreshBaseline().then(() => { firstSettled = true; });
  const second = host.refreshBaseline();
  socket.receive(baseline());
  await second;
  assert.equal(firstSettled, true);
  assert.deepEqual(socket.sent, [{ Resync: {} }]);
  await host.dispose();
});

test("baseline callback 抛错时先安装 cache，waiter 由 fail 拒绝", options, async () => {
  let shouldThrow = false;
  const { host, socket, failures } = await fixture(() => { if (shouldThrow) throw new Error("callback broke"); });
  socket.receive(baseline());
  const refresh = host.refreshBaseline();
  const rejected = assert.rejects(refresh, /REMOTE_PROTOCOL: callback broke/);
  shouldThrow = true;
  socket.receive(baseline(2));
  await new Promise<void>((resolve) => setImmediate(resolve));
  await rejected;
  assert.equal(host.snapshot().seq, 0);
  assert.equal(failures[0]!.code, "REMOTE_PROTOCOL");
});

test("同步 send throw 清理 command，dispose 显式拒绝未确认请求", options, async () => {
  const { host, socket, failures } = await fixture();
  socket.receive(baseline());
  socket.sendError = new Error("send broke");
  await assert.rejects(host.submitIntent(intent), /send broke/);
  socket.sendError = null;
  assert.equal(failures.length, 0);
  let settled = false;
  void host.submitIntent(intent).then(() => { settled = true; }, () => { settled = true; });
  await host.dispose();
  await Promise.resolve();
  assert.equal(settled, true);
});

test("generation mismatch 和 ResyncRequired 请求重同步且同步前丢弃 protocol", options, async () => {
  const { host, socket, received } = await fixture();
  socket.receive(protocol());
  assert.equal(received.length, 0);
  socket.receive(baseline());
  socket.receive(protocol(2));
  assert.deepEqual(socket.sent, [{ Resync: {} }]);
  socket.receive(protocol());
  assert.equal(received.length, 1);
  socket.receive(baseline(2));
  await new Promise<void>((resolve) => setImmediate(resolve));
  socket.receive({ ResyncRequired: { reason: "lagged", missed: 1 } });
  assert.deepEqual(socket.sent, [{ Resync: {} }, { Resync: {} }]);
  socket.receive(baseline(2));
  await new Promise<void>((resolve) => setImmediate(resolve));
  await host.dispose();
});

test("load 新连接的 5000ms timer 在 baseline 到达后清除，超时显式失败", { timeout: 10000 }, async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const successful = await fixture(undefined, undefined, false);
  const loaded = successful.host.load({});
  await new Promise<void>((resolve) => setImmediate(resolve));
  successful.sockets[0]!.receive(baseline(2));
  await loaded;
  context.mock.timers.tick(5000);
  assert.equal(successful.failures.length, 0);

  const expired = await fixture(undefined, undefined, false);
  const loading = expired.host.load({});
  const rejected = assert.rejects(loading, /REMOTE_BASELINE_TIMEOUT.*5000ms/);
  await new Promise<void>((resolve) => setImmediate(resolve));
  context.mock.timers.tick(5000);
  await rejected;
  assert.equal(expired.sockets[0]!.closed, 1);
  await expired.host.dispose();
  await successful.host.dispose();
});

for (const queryKind of ["orders", "diagnostics", "save"] as const) {
  test(`同 generation baseline 更新后 ${queryKind} 沿用各自原有的 query guard`, options, async () => {
    let finish: ((response: Response) => void) | null = null;
    const { host, socket } = await fixture(undefined, (url) => {
      if (url.pathname === "/api/save" || url.pathname === "/api/player-working-orders" || url.pathname.startsWith("/api/diagnostics")) {
        return new Promise<Response>((resolve) => { finish = resolve; });
      }
      return Response.json({});
    });
    socket.receive(baseline());
    const pending = queryKind === "orders" ? host.playerWorkingOrders() : queryKind === "diagnostics" ? host.npcDecisionTrace!("1") : host.save();
    const completed = queryKind === "save" ? pending : assert.rejects(pending, /已过期会话 generation/);
    socket.receive(baseline());
    finish!(Response.json(queryKind === "orders" ? { generation: "1", orders: [] } : queryKind === "diagnostics" ? { generation: "1", diagnostics: null } : { saved: true }));
    const result = await completed;
    if (queryKind === "save") assert.deepEqual(result, { saved: true });
    await host.dispose();
  });
}

test("resync send 同步失败拒绝已登记 waiter，后续可显式重试", options, async () => {
  const { host, socket } = await fixture();
  socket.receive(baseline());
  socket.sendError = new Error("resync send broke");
  await assert.rejects(host.refreshBaseline(), /resync send broke/);
  socket.sendError = null;
  const first = host.refreshBaseline();
  socket.receive(baseline());
  await first;
  await host.dispose();
});
