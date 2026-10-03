import assert from "node:assert/strict";
import test from "node:test";
import { createSpeedMetricsPolling } from "./useSpeedMetricsPolling.ts";
import { SpeedMetricsRequestGate } from "../host/speed.ts";
import type { SpeedMetrics } from "../host/engine-host.ts";

const metrics: SpeedMetrics = { requested: { mode: "fixed", multiplier: 1 }, actual_multiplier: 2, sample_duration_ms: 500, sample_ticks: 1, running: true };
function fixture(read = async () => metrics) {
  const updates: unknown[] = [];
  const timers = new Map<number, () => void>();
  let reads = 0;
  const gate = new SpeedMetricsRequestGate();
  const loadInProgress = { current: false };
  const runtime = createSpeedMetricsPolling({ host: { readSpeedMetrics: () => { reads++; return read(); } }, ready: true,
    requestGate: gate, loadInProgress, onMetrics: (value) => updates.push(value), onError: (value) => updates.push(value),
    schedule: (callback, delay) => { assert.equal(delay, 1000); timers.set(1, callback); return 1; }, cancel: (id) => { timers.delete(id); } });
  return { runtime, updates, timers, gate, loadInProgress, reads: () => reads };
}
const flush = async () => { await Promise.resolve(); await Promise.resolve(); };

test("测速立即读取，并且成功后只安排一个一秒重试；清理取消 timer", async () => {
  const f = fixture(); f.runtime.start(); await flush();
  assert.equal(f.reads(), 1); assert.deepEqual(f.updates, [metrics, null]); assert.equal(f.timers.size, 1);
  f.runtime.dispose(); assert.equal(f.timers.size, 0);
});
test("测速失败明确展示错误，并在下一次采样恢复", async () => {
  let fail = true;
  const f = fixture(async () => { if (fail) throw new Error("通信中断"); return metrics; });
  f.runtime.start(); await flush(); assert.match(String(f.updates[0]), /通信中断.*1 秒后自动重试/);
  fail = false; f.timers.get(1)!(); await flush(); assert.deepEqual(f.updates.slice(-2), [metrics, null]);
});
test("读档期间不启动采样；失效令牌或 cleanup 丢弃晚到采样且不重试", async () => {
  let resolve!: (value: SpeedMetrics) => void;
  const f = fixture(() => new Promise((done) => { resolve = done; }));
  f.loadInProgress.current = true; f.runtime.start(); assert.equal(f.reads(), 0);
  f.loadInProgress.current = false; f.runtime.start(); f.gate.invalidate(); resolve(metrics); await flush();
  assert.deepEqual(f.updates, []); assert.equal(f.timers.size, 0);
  const g = fixture(() => new Promise((done) => { resolve = done; }));
  g.runtime.start(); g.runtime.dispose(); resolve(metrics); await flush(); assert.deepEqual(g.updates, []); assert.equal(g.timers.size, 0);
});
