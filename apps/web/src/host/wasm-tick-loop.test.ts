import assert from "node:assert/strict";
import test from "node:test";
import { civilUpdate, frame, snapshot, tickBatch } from "./protocol-test-fixtures.ts";
import { WasmTickLoop } from "./wasm-tick-loop.ts";

async function fixture(autoAcknowledge = true) {
  let now = 0;
  let nextTimer = 0;
  let steps = 0;
  let failure: unknown = null;
  let update: unknown = tickBatch([frame(1, 0, ["600000"])], snapshot(1, 1));
  const timers = new Map<number, { callback: () => void; delay: number }>();
  const posted: unknown[] = [];
  const failed: unknown[] = [];
  const slot = {
    requireHandle: () => [7, { step: (handle: number) => { assert.equal(handle, 7); steps++; if (failure !== null) throw failure; return update; } }] as const,
    readGeneration: () => 3,
  };
  const loop = new WasmTickLoop({
    slot,
    now: () => now,
    schedule: (callback: () => void, delay: number) => { const id = ++nextTimer; timers.set(id, { callback, delay }); return id; },
    cancel: (id: number) => { timers.delete(id); },
    post: (message: unknown) => {
      posted.push(message);
      const delivery = message as { type: string; generation: number; deliveryId: number };
      if (autoAcknowledge && ["protocol", "protocolBatch"].includes(delivery.type)) loop.acknowledge(delivery.generation, delivery.deliveryId);
    },
    failure: (where: string, error: unknown) => { failed.push({ where, error }); },
  });
  const next = () => {
    const entry = timers.entries().next().value;
    assert.ok(entry, "缺少待执行 timer");
    timers.delete(entry[0]);
    return entry[1];
  };
  return { loop, posted, failed, timers, next, clock: (value: number) => { now = value; }, steps: () => steps, update: (value: unknown) => { update = value; }, fail: (value: unknown) => { failure = value; } };
}

test("WasmTickLoop start/stop 幂等且停止后旧 timer 回调不推进市场", async () => {
  const f = await fixture();
  f.loop.start();
  f.loop.start();
  assert.equal(f.timers.size, 1);
  const queued = f.next();
  assert.equal(queued.delay, 16);
  f.loop.stop();
  f.loop.stop();
  queued.callback();
  assert.equal(f.steps(), 0);
  assert.equal(f.loop.isRunning(), false);
  assert.equal(f.timers.size, 0);
  assert.deepEqual(f.loop.readSpeedMetrics(), { requested: { mode: "fixed", multiplier: 1 }, actual_multiplier: 0, sample_duration_ms: 0, sample_ticks: 0, running: false });
  f.loop.start();
  assert.equal(f.timers.size, 1);
  f.loop.stop();
  assert.equal(f.timers.size, 0);
});

test("WasmTickLoop stop 后 restart 时晚到旧 timer 不得复活或替换新 timer", { timeout: 10000 }, async () => {
  const fixtureState = await fixture();
  fixtureState.loop.setSpeed(Infinity);
  fixtureState.loop.start();
  const old = fixtureState.next();
  fixtureState.loop.stop();
  fixtureState.loop.start();
  old.callback();
  assert.equal(fixtureState.steps(), 0);
  assert.equal(fixtureState.timers.size, 1);
  fixtureState.next().callback();
  assert.equal(fixtureState.steps(), 1);
});

test("WasmTickLoop 有限倍率按 lastStepAt 单步追赶，flush 周期限制 wake delay", async () => {
  const f = await fixture();
  f.loop.setSpeed(10);
  f.loop.setFrameRate(100);
  f.loop.start();
  f.clock(50);
  f.next().callback();
  assert.equal(f.steps(), 0);
  assert.equal(f.timers.values().next().value!.delay, 10);
  f.clock(350);
  f.next().callback();
  assert.equal(f.steps(), 1);
  assert.equal(f.timers.values().next().value!.delay, 0);
  f.next().callback();
  assert.equal(f.steps(), 2);
  f.next().callback();
  assert.equal(f.steps(), 3);
  assert.equal(f.timers.values().next().value!.delay, 10);
  f.clock(400);
  f.loop.setSpeed(20);
  f.clock(425);
  f.next().callback();
  assert.equal(f.steps(), 3);
  f.clock(450);
  f.next().callback();
  assert.equal(f.steps(), 4);
});

test("WasmTickLoop Infinity 每 timer task 仅一步并以 0 delay yield", async () => {
  const f = await fixture();
  f.loop.setSpeed(Infinity);
  f.loop.start();
  f.next().callback();
  assert.equal(f.steps(), 1);
  assert.equal(f.timers.size, 1);
  assert.equal(f.timers.values().next().value!.delay, 0);
  f.next().callback();
  assert.equal(f.steps(), 2);
  assert.equal(f.timers.size, 1);
});

test("WasmTickLoop barrier 先交付 protocol 后暂停，CivilUpdate 不计市场 tick", async () => {
  const f = await fixture();
  f.loop.start();
  f.update(civilUpdate());
  assert.equal(f.loop.stepOnce(), true);
  f.clock(500);
  assert.equal(f.loop.readSpeedMetrics().sample_ticks, 0);
  f.loop.setPausePreferences({ pause_after_close: true, pause_before_open: false });
  f.posted.length = 0;
  assert.equal(f.loop.publish(civilUpdate()), false);
  assert.equal(f.loop.isRunning(), false);
  assert.equal(f.timers.size, 0);
  assert.deepEqual(f.posted, [
    { type: "protocol", generation: 3, deliveryId: 2, update: civilUpdate(), civilDate: null, revision: null },
    { type: "barrierPaused", generation: 3 },
  ]);
  assert.equal(f.loop.readSpeedMetrics().sample_ticks, 0);
});

test("WasmTickLoop 仅已交付 TickBatch 计市场 tick，step 或协议失败停循环并报错", async () => {
  const f = await fixture();
  f.loop.start();
  assert.equal(f.loop.stepOnce(), true);
  f.clock(500);
  assert.equal(f.loop.readSpeedMetrics().sample_ticks, 1);
  const failure = new Error("step failed");
  f.fail(failure);
  assert.equal(f.loop.stepOnce(), false);
  assert.equal(f.loop.isRunning(), false);
  assert.equal(f.timers.size, 0);
  assert.deepEqual(f.failed, [{ where: "wasm-worker.step", error: failure }]);
  f.fail(null);
  f.loop.start();
  f.update({ bad: true });
  assert.equal(f.loop.stepOnce(), false);
  assert.equal(f.loop.isRunning(), false);
  assert.equal(f.failed.length, 2);
});

test("WasmTickLoop 非法速度和帧率显式拒绝且保留设定", async () => {
  const f = await fixture();
  for (const speed of [0, -1, NaN, -Infinity]) assert.throws(() => f.loop.setSpeed(speed), /非法速度/);
  for (const fps of [0, -1, NaN, Infinity]) assert.throws(() => f.loop.setFrameRate(fps), /帧率必须/);
  assert.deepEqual(f.loop.readSpeedMetrics().requested, { mode: "fixed", multiplier: 1 });
});

test("WasmTickLoop 消费者未确认时有界停产，确认后恢复且旧 generation 不能释放 credit", { timeout: 10000 }, async () => {
  const fixtureState = await fixture(false);
  fixtureState.loop.setSpeed(Infinity);
  fixtureState.loop.start();
  fixtureState.clock(16);
  fixtureState.next().callback();
  fixtureState.clock(32);
  fixtureState.next().callback();
  assert.equal(fixtureState.steps(), 2);
  assert.equal(fixtureState.timers.size, 0);
  const deliveries = fixtureState.posted as { deliveryId: number; generation: number }[];
  fixtureState.loop.acknowledge(2, deliveries[0]!.deliveryId);
  assert.equal(fixtureState.timers.size, 0);
  fixtureState.loop.acknowledge(3, deliveries[0]!.deliveryId);
  assert.equal(fixtureState.timers.size, 1);
  fixtureState.clock(48);
  fixtureState.next().callback();
  assert.equal(fixtureState.steps(), 3);
  assert.equal(fixtureState.posted.length, 3);
  assert.equal(fixtureState.timers.size, 0);
});

test("WasmTickLoop 16ms 聚合完整提交，消费者接纳确认不依赖绘制", { timeout: 10000 }, async () => {
  const fixtureState = await fixture(false);
  fixtureState.loop.setSpeed(Infinity);
  fixtureState.loop.start();
  fixtureState.clock(1);
  fixtureState.next().callback();
  fixtureState.clock(2);
  fixtureState.next().callback();
  assert.equal(fixtureState.posted.length, 0);
  fixtureState.clock(16);
  fixtureState.next().callback();
  const delivery = fixtureState.posted[0] as { type: string; updates: unknown[]; deliveryId: number };
  assert.equal(delivery.type, "protocolBatch");
  assert.equal(delivery.updates.length, 2);
  assert.deepEqual(delivery.updates[0], tickBatch([frame(1, 0, ["600000"])], snapshot(1, 1)));
  fixtureState.loop.acknowledge(3, delivery.deliveryId);
  assert.equal(fixtureState.loop.isRunning(), true);
});

test("WasmTickLoop 基线控制与 stop 先交付不足16ms尾批次，不能让旧提交跨过新基线", { timeout: 10000 }, async () => {
  const fixtureState = await fixture(false);
  fixtureState.loop.setSpeed(Infinity);
  fixtureState.loop.start();
  fixtureState.clock(1);
  fixtureState.next().callback();
  assert.equal(fixtureState.posted.length, 0);
  fixtureState.loop.flushForControl();
  assert.equal(fixtureState.posted.length, 1);
  fixtureState.loop.stop();
  assert.equal(fixtureState.posted.length, 1);
});

test("WasmTickLoop 临近 tick 时按剩余 interval 唤醒，barrier frame 不安排新 timer", async () => {
  const f = await fixture();
  f.loop.setSpeed(10);
  f.loop.start();
  f.clock(95);
  f.next().callback();
  assert.equal(f.steps(), 0);
  assert.equal(f.timers.values().next().value!.delay, 5);
  f.loop.setSpeed(Infinity);
  const preferences = { pause_after_close: true, pause_before_open: false };
  f.loop.setPausePreferences(preferences);
  preferences.pause_after_close = false;
  f.update(civilUpdate());
  f.next().callback();
  assert.equal(f.steps(), 1);
  assert.equal(f.loop.isRunning(), false);
  assert.equal(f.timers.size, 0);
  assert.deepEqual(f.posted.map((message) => (message as { type: string }).type), ["protocol", "barrierPaused"]);
});
