import assert from "node:assert/strict";
import test from "node:test";
import { createRemoteHost } from "./remote-host.ts";
import type { SessionSetup, Intent } from "../types/engine.ts";

const options = { timeout: 10000, concurrency: true };
const baseline = JSON.stringify({ Baseline: { timeline_generation: 1, snapshot: { seq: 0, tick: 0, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} }, civil_date: "2030-01-02", public_revision: 0, public_report_ids: [] } });

async function fixture(loadResponse?: () => Promise<Response>) {
  const sockets: WebSocket[] = [];
  const sent: string[][] = [];
  const requests: { url: string; init: RequestInit | undefined }[] = [];
  const protocols: string[][] = [];
  const fetchFn = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = String(input);
    requests.push({ url, init });
    if (url.endsWith("/api/load") && loadResponse !== undefined) return loadResponse();
    if (url.endsWith("/api/new")) return Response.json({ session_id: "s", session_token: "owner-token" });
    if (url.includes("host-capabilities")) return Response.json({ npcDecisionDiagnostics: false });
    if (url.includes("/api/speed?")) return Response.json({ requested: { mode: "fixed", multiplier: 1 }, running: false, actual_multiplier: 0, sample_duration_ms: 0, sample_ticks: 0 });
    return new Response(null);
  }) as typeof fetch;
  const host = await createRemoteHost({} as SessionSetup, 1n, { fetchFn, webSocketFactory(url, offered) {
    assert.equal(new URL(url).searchParams.has("token"), false);
    protocols.push(offered);
    const messages: string[] = [];
    sent.push(messages);
    const socket = { readyState: 1, close() {}, send(message: string) { messages.push(message); }, onmessage: null, onclose: null, onerror: null } as unknown as WebSocket;
    sockets.push(socket);
    return socket;
  } });
  const receive = (index: number, data: string) => sockets[index].onmessage!({ data } as MessageEvent);
  return { host, sockets, sent, requests, protocols, receive };
}

test("浏览器 WS 凭据不进入 URL，speed GET 携带同一会话授权", options, async () => {
  const context = await fixture();
  await context.host.start(() => {});
  assert.deepEqual(context.protocols[0], ["stock-game", "stock-game.auth.6f776e65722d746f6b656e"]);
  await context.host.readSpeedMetrics();
  const speed = context.requests.find(({ url }) => url.includes("/api/speed?"));
  assert.equal(new Headers(speed!.init!.headers).get("authorization"), "Bearer owner-token");
  await context.host.dispose();
});

test("暂停继续不重送旧 baseline，pull 持续请求并忽略旧 socket error", options, async () => {
  const context = await fixture();
  const updates: unknown[] = [];
  await context.host.start((update) => { updates.push(update); });
  context.receive(0, baseline);
  await context.host.stop();
  await context.host.start((update) => { updates.push(update); });
  assert.equal(updates.length, 1);
  context.host.setDeliveryMode!("pull");
  context.sockets[0].onerror!({} as Event);
  context.receive(1, baseline);
  await new Promise((resolve) => setTimeout(resolve, 30));
  assert.equal(context.sent[1].filter((message) => message.includes("GetFrame")).length, 1);
  context.receive(1, JSON.stringify({ FrameEmpty: {} }));
  await new Promise((resolve) => setTimeout(resolve, 25));
  assert.ok(context.sent[1].filter((message) => message.includes("GetFrame")).length >= 2);
  await context.host.dispose();
});

test("dispose 与切换 delivery 为已发送写请求提供结果未知出口", options, async () => {
  for (const action of ["dispose", "delivery"] as const) {
    const context = await fixture();
    await context.host.start(() => {});
    context.receive(0, baseline);
    const pending = context.host.submitIntent({} as Intent);
    const rejection = assert.rejects(pending, /结果未知/);
    if (action === "dispose") await context.host.dispose();
    else context.host.setDeliveryMode!("pull");
    await rejection;
    await context.host.dispose();
  }
});

test("并发 refresh 与内部 ResyncRequired 共享请求，不遗失原 waiter", options, async () => {
  const context = await fixture();
  await context.host.start(() => {});
  context.receive(0, baseline);
  const first = context.host.refreshBaseline!();
  const second = context.host.refreshBaseline!();
  context.receive(0, JSON.stringify({ ResyncRequired: { reason: "lagged", missed: null } }));
  assert.equal(context.sent[0].filter((message) => message.includes("Resync")).length, 1);
  context.receive(0, baseline);
  await Promise.all([first, second]);
  await context.host.dispose();
});

test("意外断线自动取新权威 baseline，迟到旧 socket 消息不能污染恢复", options, async () => {
  const context = await fixture();
  const updates: unknown[] = [];
  await context.host.start((update) => { updates.push(update); });
  context.receive(0, baseline);
  context.sockets[0].onclose!({} as CloseEvent);
  await new Promise((resolve) => setTimeout(resolve, 150));
  assert.equal(context.sockets.length, 2);
  context.sockets[0].onerror!({} as Event);
  context.receive(0, baseline);
  assert.equal(updates.length, 1);
  context.receive(1, baseline);
  assert.equal(updates.length, 2);
  await context.host.dispose();
});

test("已中断的写请求迟到 queued 不冒充完成，不关闭同 socket 的重同步连接", options, async () => {
  const context = await fixture();
  const failures: unknown[] = [];
  await context.host.start(() => {}, (failure) => failures.push(failure));
  context.receive(0, baseline);
  const command = context.host.submitIntent({} as Intent);
  const rejected = assert.rejects(command, /结果未知/);
  const refreshing = context.host.refreshBaseline!();
  await rejected;
  context.receive(0, JSON.stringify({ CommandQueued: { request_id: 1 } }));
  assert.equal(failures.length, 0);
  context.receive(0, baseline);
  await refreshing;
  await context.host.dispose();
});

test("无入队确认的写请求在 5000ms 结果未知，真实未发送的 ID 仍协议失败", { timeout: 10000 }, async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const fixtureContext = await fixture();
  const failures: unknown[] = [];
  await fixtureContext.host.start(() => {}, (failure) => failures.push(failure));
  fixtureContext.receive(0, baseline);
  const command = fixtureContext.host.submitIntent({} as Intent);
  const rejected = assert.rejects(command, /5000ms.*结果未知/);
  context.mock.timers.tick(5000);
  await rejected;
  fixtureContext.receive(0, JSON.stringify({ CommandQueued: { request_id: 1 } }));
  assert.equal(failures.length, 0);
  fixtureContext.receive(0, JSON.stringify({ CommandQueued: { request_id: 999 } }));
  assert.equal(failures.length, 1);
  await fixtureContext.host.dispose();
});

test("连续三次自动恢复失败后显式 fatal，不无限重连", { timeout: 10000 }, async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const fixtureContext = await fixture();
  const failures: unknown[] = [];
  await fixtureContext.host.start(() => {}, (failure) => failures.push(failure));
  fixtureContext.receive(0, baseline);
  for (const [index, delay] of [[0, 100], [1, 200], [2, 400]] as const) {
    fixtureContext.sockets[index].onclose!({} as CloseEvent);
    context.mock.timers.tick(delay);
  }
  assert.equal(fixtureContext.sockets.length, 4);
  fixtureContext.sockets[3].onclose!({} as CloseEvent);
  assert.equal(failures.length, 1);
  context.mock.timers.tick(10000);
  assert.equal(fixtureContext.sockets.length, 4);
  await fixtureContext.host.dispose();
});

test("应用层同步拒绝 baseline 后关闭当前连接并拒绝 waiter，不覆盖原 failure", options, async () => {
  const context = await fixture();
  const failures: unknown[] = [];
  await context.host.start(() => false, (failure) => failures.push(failure));
  const refreshing = context.host.refreshBaseline!();
  const rejected = assert.rejects(refreshing, /应用层拒绝.*结果未知/);
  context.receive(0, baseline);
  await rejected;
  assert.equal(failures.length, 0);
  await assert.rejects(context.host.submitIntent({} as Intent), /尚未完成权威基线同步/);
  await context.host.dispose();
});

test("启动拒绝读档缓存 baseline 时返回失败且不请求 running true", options, async () => {
  const context = await fixture();
  const loaded = context.host.load({});
  await new Promise((resolve) => setImmediate(resolve));
  context.receive(0, baseline);
  await loaded;
  await assert.rejects(context.host.start(() => false), /应用层拒绝.*启动未确认/);
  assert.equal(context.requests.some(({ url }) => url.endsWith("/api/running")), false);
  await context.host.dispose();
});

test("远程新 generation baseline 先于HTTP读档响应时，先通知恢复配置再交付", options, async () => {
  let acknowledge!: (response: Response) => void;
  const response = new Promise<Response>(resolve => { acknowledge = resolve; });
  const context = await fixture(() => response);
  let restored = false;
  const observations: boolean[] = [];
  await context.host.start(update => { if (update.generation === "2") observations.push(restored); });
  context.receive(0, baseline);
  const loaded = context.host.load({}, () => { restored = true; });
  const newBaseline = baseline.replace('"timeline_generation":1', '"timeline_generation":2');
  context.receive(0, newBaseline);
  acknowledge(new Response(null));
  await new Promise(resolve => setImmediate(resolve));
  context.receive(0, newBaseline);
  try {
    await loaded;
    assert.deepEqual(observations, [true, true]);
  } finally { await context.host.dispose(); }
});

test("远程读档HTTP响应丢失后，重同步新generation仍先确认已提交配置", options, async () => {
  let rejectResponse!: (error: Error) => void;
  const response = new Promise<Response>((_resolve, reject) => { rejectResponse = reject; });
  const context = await fixture(() => response);
  let restored = false;
  const observations: boolean[] = [];
  await context.host.start(update => { if (update.generation === "2") observations.push(restored); });
  context.receive(0, baseline);
  const rejected = assert.rejects(context.host.load({}, () => { restored = true; }), /响应丢失/);
  rejectResponse(new Error("响应丢失"));
  await rejected;
  assert.equal(restored, false);
  const refreshing = context.host.refreshBaseline();
  context.receive(0, baseline.replace('"timeline_generation":1', '"timeline_generation":2'));
  try {
    await refreshing;
    assert.deepEqual(observations, [true]);
  } finally { await context.host.dispose(); }
});

test("远程读档失败后旧generation确认不改变配置并允许重试", options, async () => {
  const context = await fixture(async () => { throw new Error("请求未受理"); });
  let restores = 0;
  await context.host.start(() => {});
  context.receive(0, baseline);
  await assert.rejects(context.host.load({}, () => { restores++; }), /请求未受理/);
  await assert.rejects(context.host.load({}), /先刷新权威基线/);
  const refreshing = context.host.refreshBaseline();
  context.receive(0, baseline);
  try {
    await refreshing;
    assert.equal(restores, 0);
    await assert.rejects(context.host.load({}), /请求未受理/);
  } finally { await context.host.dispose(); }
});
