import assert from "node:assert/strict";
import test from "node:test";
import { WasmAuthorityRuntime, type MarketBindings } from "./wasm-authority-runtime.ts";

function fixture() {
  let id = 0;
  let turns = 0;
  let result: unknown = { delayMs: 5.25, running: true, committedTicks: 2 };
  const work = new Map<number, { callback: () => void; delay: number }>();
  const errors: Array<[string, unknown]> = [];
  const controls: string[] = [];
  const schedule = {
    schedule: (callback: () => void, delay: number) => { work.set(++id, { callback, delay }); return id; },
    cancel: (timer: number) => { work.delete(timer); },
  };
  const runtime = new WasmAuthorityRuntime(schedule, (where, error) => errors.push([where, error]));
  const bindings: MarketBindings = {
    market_runner_enable: (handle) => controls.push(`bind:${handle}`),
    market_runner_set_running: (handle, running) => controls.push(`run:${handle}:${running}`),
    market_runner_set_speed: (handle, speed) => controls.push(`speed:${handle}:${speed}`),
    market_runner_set_pause: (handle, after, before) => controls.push(`pause:${handle}:${after}:${before}`),
    market_runner_speed_metrics: () => ({ requested: { mode: "fixed", multiplier: 1 }, actual_multiplier: 0, sample_duration_ms: 0, sample_ticks: 0, running: false }),
    market_runner_turn: () => { turns++; return result; },
    market_runner_step_once: (handle) => controls.push(`step:${handle}`),
    market_runner_end_civil: (handle) => controls.push(`civil:${handle}`),
    market_runner_observer_baseline: () => ({}),
    market_runner_control_signal: () => 4,
    market_runner_command_consumed: (handle) => controls.push(`admitted:${handle}`),
  };
  runtime.bind(7, bindings);
  const next = () => {
    const item = work.entries().next().value;
    assert.ok(item, "必须有 Rust 驱动安排的后续唤醒");
    work.delete(item[0]);
    return item[1];
  };
  return { runtime, bindings, work, errors, controls, next, turns: () => turns, result: (next: unknown) => { result = next; } };
}

test("权威 worker 只执行 Rust WakePlan，多个已提交 tick 无需任何 UI 确认", () => {
  const f = fixture();
  f.runtime.start();
  f.runtime.start();
  assert.equal(f.work.size, 1);
  f.next().callback();
  assert.equal(f.next().delay, 5.25);
  assert.equal(f.turns(), 1);
  assert.deepEqual(f.controls.filter((control) => control.startsWith("run:")), ["run:7:true"]);
});

test("停机并换代后，即使陈旧 timer 回调抵达也不能推进新 authority", () => {
  const f = fixture();
  f.runtime.start();
  const old = f.next();
  f.runtime.stop();
  f.runtime.bind(8, f.bindings);
  f.runtime.start();
  old.callback();
  assert.equal(f.turns(), 0);
  assert.equal(f.work.size, 1);
  f.next().callback();
  assert.equal(f.turns(), 1);
});

test("日界暂停和推进失败分别停止主动唤醒，非法 WakePlan 必须上报", () => {
  const f = fixture();
  f.runtime.start();
  f.result({ delayMs: null, running: false, committedTicks: 0 });
  f.next().callback();
  assert.equal(f.runtime.isRunning(), false);
  assert.equal(f.work.size, 0);
  assert.deepEqual(f.errors, []);
  f.runtime.start();
  f.result({ delayMs: NaN, running: true, committedTicks: 0 });
  f.next().callback();
  assert.equal(f.runtime.isRunning(), false);
  assert.equal(f.work.size, 0);
  assert.equal(f.errors[0]?.[0], "wasm-worker.market-runner");
});

test("手工控制和改速交给 Rust，过期会话控制不得推进序号", () => {
  const f = fixture();
  f.runtime.setSpeed(Infinity);
  f.runtime.setPausePreferences({ pause_after_close: true, pause_before_open: true });
  f.runtime.stepOnce();
  f.runtime.endCivil();
  f.runtime.confirmControl(7);
  assert.throws(() => f.runtime.confirmControl(8), /过期/);
  assert.deepEqual(f.controls.slice(-5), ["speed:7:Infinity", "pause:7:true:true", "step:7", "civil:7", "admitted:7"]);
});
