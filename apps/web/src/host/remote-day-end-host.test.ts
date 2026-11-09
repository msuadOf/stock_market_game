import assert from "node:assert/strict";
import test from "node:test";
import { createRemoteHost } from "./remote-host.ts";
import { catalogWireFixture } from "./catalog-test-fixture.ts";
import { snapshot } from "./protocol-test-fixtures.ts";
import { DEFAULT_SETUP } from "../config/defaults.ts";

async function fixture() {
  const messages: Record<string, Record<string, unknown>>[] = [], requests: { url: string; body: unknown }[] = [], failures: string[] = [];
  const socket = { readyState: 1, close() {}, send(raw: string) { messages.push(JSON.parse(raw)); }, onmessage: null, onerror: null, onclose: null } as unknown as WebSocket;
  const host = await createRemoteHost(DEFAULT_SETUP, 1n, { webSocketFactory: () => socket, fetchFn: (async (input, init) => {
    const url = String(input), body = typeof init?.body === "string" ? JSON.parse(init.body) : null;
    requests.push({ url, body });
    if (url.endsWith("/api/new")) return Response.json({ session_id: "session-1", session_token: "owner" });
    if (url.includes("host-capabilities")) return Response.json({ npcDecisionDiagnostics: false });
    if (url.endsWith("/api/day-end-export")) return Response.json({ candidate: true });
    return new Response(null, { status: 204 });
  }) as typeof fetch });
  const receive = (value: unknown) => socket.onmessage!({ data: JSON.stringify(value) } as MessageEvent);
  const baseline = (generation = 1) => receive({ Baseline: { ...catalogWireFixture(snapshot(0, 0)), timeline_generation: generation, snapshot: snapshot(0, 0), civil_date: "2030-01-02", public_revision: 0, public_report_ids: [] } });
  await host.start(() => {}, failure => failures.push(failure.message)); baseline();
  return { host, socket, messages, requests, receive, baseline, failures };
}

test("RemoteHost 显式文件协议通过 WS 订阅并使用专用 REST read/ACK，不调用 /api/save", { timeout: 10000 }, async () => {
  const f = await fixture();
  try {
    assert.equal(f.messages.length, 0);
    assert.ok(f.host.subscribeDayEndExports);
    const pending = f.host.subscribeDayEndExports(error => f.failures.push(error.message));
    const request = f.messages[0].SubscribeDayEndExports;
    assert.deepEqual(request, { request_id: 1, generation: "1" });
    f.receive({ DayEndExportSubscribed: { request_id: request.request_id, subscription: "export-1", afterSeq: 0 } });
    const subscription = await pending, candidate = { seq: 2, settledDate: "2030-01-02" };
    assert.deepEqual(await subscription.read(candidate), { candidate: true }); await subscription.acknowledge(candidate);
    assert.deepEqual(f.requests.filter(({ url }) => url.includes("day-end-export")).map(({ url, body }) => ({ path: new URL(url).pathname, body })), [
      { path: "/api/day-end-export", body: { session_id: "session-1", generation: "1", subscription: "export-1", candidate } },
      { path: "/api/day-end-export/ack", body: { session_id: "session-1", generation: "1", subscription: "export-1", candidate } },
    ]);
    assert.equal(f.requests.some(({ url }) => url.endsWith("/api/save")), false);
    const close = subscription.close();
    f.receive({ DayEndExportUnsubscribed: { request_id: f.messages[1].UnsubscribeDayEndExports.request_id } }); await close;
    assert.deepEqual(f.failures, []);
  } finally { await f.host.dispose(); }
});

test("服务端容量取消显式通知文件 owner，市场协议仍健康且 generation 替换撤销旧订阅", { timeout: 10000 }, async () => {
  const f = await fixture();
  try {
    assert.ok(f.host.subscribeDayEndExports);
    let pending = f.host.subscribeDayEndExports(error => f.failures.push(error.message));
    f.receive({ DayEndExportSubscribed: { request_id: 1, subscription: "export-1", afterSeq: 0 } });
    const subscription = await pending;
    f.receive({ DayEndExportFailed: { subscription: "export-1", code: "DAY_END_EXPORT_CAPACITY", message: "2 GiB 已满" } });
    await assert.rejects(subscription.read({ seq: 2, settledDate: "2030-01-02" }), /订阅已失效/);
    assert.equal(f.failures.length, 1); assert.match(f.failures[0], /2 GiB/);
    pending = f.host.subscribeDayEndExports(error => f.failures.push(error.message));
    f.receive({ DayEndExportSubscribed: { request_id: 2, subscription: "export-2", afterSeq: 0 } });
    const next = await pending; f.baseline(2);
    await assert.rejects(next.read({ seq: 2, settledDate: "2030-01-02" }), /订阅已失效/);
    assert.equal(f.failures.length, 2);
  } finally { await f.host.dispose(); }
});

test("RemoteHost 断连使在途订阅失败，迟到的旧 WS 确认不会重新授权", { timeout: 10000 }, async () => {
  const f = await fixture();
  try {
    assert.ok(f.host.subscribeDayEndExports);
    const pending = f.host.subscribeDayEndExports(error => f.failures.push(error.message));
    const rejected = assert.rejects(pending, /连接中断/);
    f.socket.onerror!({} as Event); await rejected;
    f.receive({ DayEndExportSubscribed: { request_id: 1, subscription: "late", afterSeq: 0 } });
    assert.equal(f.requests.some(({ url }) => url.includes("day-end-export")), false);
  } finally { await f.host.dispose(); }
});

test("同 generation Resync 显式撤销文件租约，重新安装 baseline 后不自动订阅", { timeout: 10000 }, async () => {
  const f = await fixture();
  try {
    assert.ok(f.host.subscribeDayEndExports);
    const pending = f.host.subscribeDayEndExports(error => f.failures.push(error.message));
    f.receive({ DayEndExportSubscribed: { request_id: 1, subscription: "export-1", afterSeq: 0 } });
    const subscription = await pending;
    const resync = f.host.refreshBaseline();
    assert.equal(f.failures.length, 1); assert.match(f.failures[0], /重新授权/);
    f.baseline(); await resync;
    await assert.rejects(subscription.read({ seq: 2, settledDate: "2030-01-02" }), /订阅已失效/);
    assert.equal(f.messages.filter(message => "SubscribeDayEndExports" in message).length, 1);
  } finally { await f.host.dispose(); }
});
