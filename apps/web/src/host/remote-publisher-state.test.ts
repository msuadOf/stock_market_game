import assert from "node:assert/strict";
import test from "node:test";
import { createBaselineUpdate } from "./host-update.ts";
import { parseProtocolSnapshot } from "./protocol/index.ts";
import { RemotePublisherState } from "./remote-publisher-state.ts";

const options = { concurrency: true, timeout: 10000 };
const snapshot = parseProtocolSnapshot({ seq: 0, tick: 0, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} }, "测试 baseline");
const baseline = (generation: string) => createBaselineUpdate(generation, snapshot, { civilDate: "2030-01-02", revision: "0" });

test("RemotePublisherState socket 移交与连接 identity 不改变 baseline gate", options, () => {
  const state = new RemotePublisherState();
  const old = {} as WebSocket;
  const next = {} as WebSocket;
  const identity = state.invalidateConnection();
  state.attachSocket(old);
  assert.equal(state.isProtocolReady(), false);
  state.installBaseline(baseline("1"), () => assert.fail("首次 baseline 不切时间线"));
  state.invalidateConnection();
  state.attachSocket(next);
  state.clearSocketIfCurrent(old);
  assert.equal(state.currentSocket(), next);
  assert.equal(state.isCurrentConnection(identity), false);
  assert.equal(state.isProtocolDeliverable("1"), true);
  assert.equal(state.detachSocket(), next);
  assert.equal(state.baselineForRead()!.generation, "1");
});

test("RemotePublisherState epoch 每次 baseline 都更新，跨 generation 先通知再安装 cache", options, () => {
  const state = new RemotePublisherState();
  assert.throws(() => state.captureQueryCursor("baseline 缺失"), /baseline 缺失/);
  state.installBaseline(baseline("1"), () => assert.fail("首次 baseline 不切时间线"));
  const cursor = state.captureQueryCursor("baseline 缺失");
  state.assertQueryCursor(cursor, "1", "响应过期");
  state.installBaseline(baseline("1"), () => assert.fail("同 generation 不切时间线"));
  assert.throws(() => state.assertQueryCursor(cursor, "1", "响应过期"), /响应过期/);
  state.installBaseline(baseline("2"), () => assert.equal(state.baselineForRead()!.generation, "1"));
  assert.equal(state.baselineForRead()!.generation, "2");
  state.beginResync();
  assert.equal(state.isProtocolDeliverable("2"), false);
});

test("RemotePublisherState waiter 保留单槽覆盖和 resolve 后清槽的重入顺序", options, () => {
  const state = new RemotePublisherState();
  const completed: string[] = [];
  state.beginBaselineWait({ resolve() { completed.push("old"); }, reject() { completed.push("old-rejected"); } });
  state.beginBaselineWait({ resolve() {
    completed.push("current");
    state.beginBaselineWait({ resolve() { completed.push("nested"); }, reject() { completed.push("nested-rejected"); } });
  }, reject() { completed.push("current-rejected"); } });
  state.resolveBaselineWaiter();
  assert.deepEqual(completed, ["current"]);
  assert.equal(state.hasBaselineWaiter(), false);
  state.beginBaselineWait({ resolve() { completed.push("taken"); }, reject() { completed.push("taken-rejected"); } });
  const taken = state.takeBaselineWaiter();
  assert.equal(state.hasBaselineWaiter(), false);
  taken!.reject(new Error("fail"));
  assert.deepEqual(completed, ["current", "taken-rejected"]);
});
