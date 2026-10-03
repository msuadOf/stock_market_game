import assert from "node:assert/strict";
import test from "node:test";
import type { Intent, SessionSetup } from "../types/engine.ts";
import type { HostFailure, HostUpdate } from "./host-update.ts";
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

async function fixture(onUpdate?: (update: HostUpdate) => void, respond?: (url: URL) => Response | Promise<Response>) {
  const sockets: FakeSocket[] = [];
  const failures: HostFailure[] = [];
  const received: HostUpdate[] = [];
  const host = await createRemoteHost({} as SessionSetup, 1n, {
    baseUrl: "https://remote.example",
    fetchFn: async (input) => {
      const url = new URL(String(input));
      if (url.pathname === "/api/new") return Response.json({ session_id: "session", session_token: "token" });
      if (url.pathname === "/api/host-capabilities") return Response.json({ npcDecisionDiagnostics: respond !== undefined });
      return respond === undefined ? Response.json({}) : respond(url);
    },
    webSocketFactory: () => {
      const socket = new FakeSocket();
      sockets.push(socket);
      return socket as unknown as WebSocket;
    },
  });
  host.start((update) => { received.push(update); onUpdate?.(update); }, (failure) => failures.push(failure));
  return { host, sockets, failures, received, socket: sockets[0]! };
}

function commandId(socket: FakeSocket, position: number): number {
  return (socket.sent[position] as { SubmitIntent: { request_id: number } }).SubmitIntent.request_id;
}

const options = { timeout: 10000, concurrency: true };

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
  socket.onclose?.();
  replacement.receive(protocol());
  assert.deepEqual(received.map((update) => update.type), ["baseline", "protocol"]);
  assert.equal(host.getDeliveryMode(), "pull");
  assert.equal(failures.length, 0);
});

test("旧 socket onerror 按现状仍使新连接 fail，并保留 cached baseline", options, async () => {
  const { host, socket, sockets, failures } = await fixture();
  assert.ok(host.setDeliveryMode);
  socket.receive(baseline());
  host.setDeliveryMode("pull");
  socket.onerror?.();
  assert.equal(sockets[1]!.closed, 1);
  assert.equal(failures[0]!.code, "REMOTE_SOCKET");
  assert.equal(host.tick(), 0);
  await assert.rejects(host.submitIntent(intent), /尚未完成权威基线同步/);
});

test("resync 按现状覆盖单槽 waiter，baseline 仅完成最新 waiter", options, async () => {
  const { host, socket } = await fixture();
  socket.receive(baseline());
  let firstSettled = false;
  void host.refreshBaseline().then(() => { firstSettled = true; });
  const second = host.refreshBaseline();
  socket.receive(baseline());
  await second;
  assert.equal(firstSettled, false);
  assert.deepEqual(socket.sent, [{ Resync: {} }, { Resync: {} }]);
  host.dispose();
});

test("baseline callback 抛错时先安装 cache，waiter 由 fail 拒绝", options, async () => {
  let shouldThrow = false;
  const { host, socket, failures } = await fixture(() => { if (shouldThrow) throw new Error("callback broke"); });
  socket.receive(baseline());
  const refresh = host.refreshBaseline();
  const rejected = assert.rejects(refresh, /REMOTE_PROTOCOL: callback broke/);
  shouldThrow = true;
  socket.receive(baseline(2));
  await rejected;
  assert.equal(host.snapshot().seq, 0);
  assert.equal(failures[0]!.code, "REMOTE_PROTOCOL");
});

test("同步 send throw 保留 command 登记，dispose 按现状不 settle pending command", options, async () => {
  const { host, socket, failures } = await fixture();
  socket.receive(baseline());
  socket.sendError = new Error("send broke");
  await assert.rejects(host.submitIntent(intent), /send broke/);
  socket.sendError = null;
  socket.receive({ CommandQueued: { request_id: 1 } });
  assert.equal(failures.length, 0);
  let settled = false;
  void host.submitIntent(intent).then(() => { settled = true; }, () => { settled = true; });
  host.dispose();
  await Promise.resolve();
  assert.equal(settled, false);
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
  socket.receive({ ResyncRequired: { reason: "lagged", missed: 1 } });
  assert.deepEqual(socket.sent, [{ Resync: {} }, { Resync: {} }]);
  socket.receive(baseline(2));
  host.dispose();
});

test("load 新连接的 5000ms timer 在 baseline 到达后清除，超时显式失败", { timeout: 10000 }, async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const successful = await fixture();
  successful.socket.receive(baseline());
  successful.host.stop();
  successful.socket.onclose?.();
  const loaded = successful.host.load({});
  await new Promise<void>((resolve) => setImmediate(resolve));
  successful.sockets[1]!.receive(baseline(2));
  await loaded;
  t.mock.timers.tick(5000);
  assert.equal(successful.failures.length, 0);

  const expired = await fixture();
  expired.host.stop();
  expired.socket.onclose?.();
  const loading = expired.host.load({});
  const rejected = assert.rejects(loading, /REMOTE_BASELINE_TIMEOUT/);
  await new Promise<void>((resolve) => setImmediate(resolve));
  t.mock.timers.tick(5000);
  await rejected;
  assert.equal(expired.failures[0]!.code, "REMOTE_BASELINE_TIMEOUT");
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
    const pending = queryKind === "orders" ? host.playerWorkingOrders() : queryKind === "diagnostics" ? host.npcDecisionTrace!(1) : host.save();
    const completed = queryKind === "save" ? pending : assert.rejects(pending, /已过期会话 generation/);
    socket.receive(baseline());
    finish!(Response.json(queryKind === "orders" ? { generation: "1", orders: [] } : queryKind === "diagnostics" ? { generation: "1", diagnostics: null } : { saved: true }));
    const result = await completed;
    if (queryKind === "save") assert.deepEqual(result, { saved: true });
    host.dispose();
  });
}

test("resync send 同步失败发生在新 waiter 登记前，既有 waiter 仍等待 baseline", options, async () => {
  const { host, socket } = await fixture();
  socket.receive(baseline());
  const first = host.refreshBaseline();
  socket.sendError = new Error("resync send broke");
  await assert.rejects(host.refreshBaseline(), /resync send broke/);
  socket.sendError = null;
  socket.receive(baseline());
  await first;
  host.dispose();
});
