import assert from "node:assert/strict";
import test from "node:test";
import { RemoteDayEndExports } from "./remote-day-end-export.ts";

function fixture() {
  const sent: Record<string, Record<string, unknown>>[] = [], requests: { path: string; body: unknown }[] = [], failures: string[] = [];
  let generation = "1", connection = {}, ready = true, requestId = 0;
  const exports = new RemoteDayEndExports({
    nextRequestId: () => ++requestId,
    current: () => { if (!ready) throw new Error("连接未就绪"); return { generation, connection }; },
    send: value => { sent.push(value); },
    request: async (path, body) => { requests.push({ path, body }); return path.endsWith("/ack") ? null : { saved: true }; },
    interruptConnection: error => { ready = false; exports.interrupt(error); },
  });
  return { exports, sent, requests, failures, changeGeneration() { generation = "2"; }, disconnect() { ready = false; connection = {}; exports.interrupt(new Error("连接中断")); }, onFailure: (error: Error) => failures.push(error.message) };
}

test("显式订阅确认后 read 与 ACK 使用原 subscription、generation 及精确候选", { timeout: 10000 }, async () => {
  const f = fixture(), pending = f.exports.subscribe(f.onFailure);
  assert.deepEqual(f.sent, [{ SubscribeDayEndExports: { request_id: 1, generation: "1" } }]);
  f.exports.subscribed(1, "sub-1", 0); const subscription = await pending;
  const candidate = { seq: 2, settledDate: "2030-01-02" };
  assert.deepEqual(await subscription.read(candidate), { saved: true });
  await subscription.acknowledge(candidate);
  assert.deepEqual(f.requests, [
    { path: "/api/day-end-export", body: { generation: "1", subscription: "sub-1", candidate } },
    { path: "/api/day-end-export/ack", body: { generation: "1", subscription: "sub-1", candidate } },
  ]);
  const closing = subscription.close();
  assert.deepEqual(f.sent[1], { UnsubscribeDayEndExports: { request_id: 2, generation: "1", subscription: "sub-1" } });
  f.exports.unsubscribed(2); await closing;
  await assert.rejects(subscription.read(candidate), /订阅已失效/);
});

test("断连取消在途订阅且迟到确认不能安装旧 token", { timeout: 10000 }, async () => {
  const f = fixture(), pending = f.exports.subscribe(f.onFailure);
  const rejected = assert.rejects(pending, /连接中断/);
  f.disconnect(); await rejected;
  f.exports.subscribed(1, "late", 0);
  assert.deepEqual(f.requests, []);
});

test("活动订阅断连、generation 变化及容量失败都显式失效，不自动重订阅", { timeout: 10000 }, async () => {
  for (const failure of ["disconnect", "generation", "capacity"] as const) {
    const f = fixture(), pending = f.exports.subscribe(f.onFailure);
    f.exports.subscribed(1, "sub-1", 0); const subscription = await pending;
    if (failure === "disconnect") f.disconnect();
    if (failure === "generation") { f.changeGeneration(); f.exports.invalidateGeneration("2"); }
    if (failure === "capacity") f.exports.failed("sub-1", "DAY_END_EXPORT_CAPACITY", "日终文件导出超过 2 GiB");
    await assert.rejects(subscription.read({ seq: 2, settledDate: "2030-01-02" }), /订阅已失效/);
    assert.equal(f.failures.length, 1);
    assert.equal(f.sent.length, 1);
    assert.deepEqual(f.requests, []);
  }
});

test("GatewayError 拒绝订阅，旧 generation 的订阅成功响应也不能生效", { timeout: 10000 }, async () => {
  const f = fixture(), pending = f.exports.subscribe(f.onFailure);
  const rejected = assert.rejects(pending, /禁止导出/);
  assert.equal(f.exports.rejectGateway(1, new Error("禁止导出")), true); await rejected;
  const next = f.exports.subscribe(f.onFailure), stale = assert.rejects(next, /generation/);
  f.changeGeneration(); f.exports.subscribed(2, "stale", 0); await stale;
});

test("订阅确认超时中断连接以清理未知 token，迟到确认不安装租约", { timeout: 10000 }, async context => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const f = fixture(), pending = f.exports.subscribe(f.onFailure), rejected = assert.rejects(pending, /确认超时/);
  context.mock.timers.tick(5000); await rejected;
  f.exports.subscribed(1, "late", 0);
  await assert.rejects(f.exports.subscribe(f.onFailure), /连接未就绪/);
  assert.deepEqual(f.requests, []);
});

test("取消订阅被拒绝时关闭原连接，不能遗留无 owner 的远程候选", { timeout: 10000 }, async () => {
  const f = fixture(), pending = f.exports.subscribe(f.onFailure);
  f.exports.subscribed(1, "sub-1", 0); const subscription = await pending;
  const closing = subscription.close(), rejected = assert.rejects(closing, /取消被拒绝/);
  f.exports.rejectGateway(2, new Error("取消被拒绝")); await rejected;
  await assert.rejects(f.exports.subscribe(f.onFailure), /连接未就绪/);
});
