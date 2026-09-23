import assert from "node:assert/strict";
import test from "node:test";
import {
  HostSpeedMeter,
  SpeedMetricsRequestGate,
  assertValidSpeedMultiplier,
  parseSpeedMetrics,
  speedMetricsMatchesRequested,
  speedMetricsMatchesUiState,
} from "./speed.ts";

test("所有宿主共享的倍速边界拒绝 NaN、非正数和负无穷", () => {
  for (const speed of [Number.NaN, 0, -1, Number.NEGATIVE_INFINITY]) {
    assert.throws(() => assertValidSpeedMultiplier(speed), /非法速度倍率/);
  }
});

test("倍速边界接受有限正数和明确的最快模式", () => {
  assert.doesNotThrow(() => assertValidSpeedMultiplier(720));
  assert.doesNotThrow(() => assertValidSpeedMultiplier(Infinity));
});

test("实测倍率只匹配当前选择的速度模式", () => {
  assert.equal(speedMetricsMatchesRequested({ mode: "fastest" }, Infinity), true);
  assert.equal(speedMetricsMatchesRequested({ mode: "fastest" }, 60), false);
  assert.equal(speedMetricsMatchesRequested({ mode: "fixed", multiplier: 60 }, 60), true);
  assert.equal(speedMetricsMatchesRequested({ mode: "fixed", multiplier: 60 }, 20), false);
});

test("实测倍率必须同时匹配当前速度和运行状态", () => {
  const runningMetrics = {
    requested: { mode: "fixed" as const, multiplier: 60 },
    actual_multiplier: 59.5,
    sample_duration_ms: 1_000,
    sample_ticks: 60,
    running: true,
  };
  assert.equal(speedMetricsMatchesUiState(runningMetrics, 60, true), true);
  assert.equal(speedMetricsMatchesUiState(runningMetrics, 60, false), false);
  assert.equal(speedMetricsMatchesUiState(runningMetrics, 30, true), false);
});

test("统一测速协议拒绝暂停状态携带非零实测倍率", () => {
  assert.throws(() => parseSpeedMetrics({
    requested: { mode: "fixed", multiplier: 1 },
    actual_multiplier: 1,
    sample_duration_ms: 1_000,
    sample_ticks: 1,
    running: false,
  }), /暂停状态/);
});

test("所有本地宿主使用同一采样器按权威 tick 计算实际倍率", () => {
  let now = 1_000;
  const meter = new HostSpeedMeter(() => now);
  meter.setSpeed(60);
  meter.setRunning(true);
  meter.recordTicks(29);

  now += 499;
  assert.deepEqual(meter.read(), {
    requested: { mode: "fixed", multiplier: 60 },
    actual_multiplier: null,
    sample_duration_ms: 0,
    sample_ticks: 0,
    running: true,
  });

  meter.recordTicks(1);
  now += 1;
  assert.deepEqual(meter.read(), {
    requested: { mode: "fixed", multiplier: 60 },
    actual_multiplier: 60,
    sample_duration_ms: 500,
    sample_ticks: 30,
    running: true,
  });
});

test("本地宿主测速在暂停与切速后重置采样窗口", () => {
  let now = 0;
  const meter = new HostSpeedMeter(() => now);
  meter.setSpeed(Infinity);
  meter.setRunning(true);
  meter.recordTicks(400);

  meter.setRunning(false);
  assert.deepEqual(meter.read(), {
    requested: { mode: "fastest" },
    actual_multiplier: 0,
    sample_duration_ms: 0,
    sample_ticks: 0,
    running: false,
  });

  now = 600;
  meter.setRunning(true);
  meter.recordTicks(300);
  now = 1_100;
  assert.equal(meter.read().actual_multiplier, 600);

  meter.setSpeed(3);
  assert.deepEqual(meter.read(), {
    requested: { mode: "fixed", multiplier: 3 },
    actual_multiplier: null,
    sample_duration_ms: 0,
    sample_ticks: 0,
    running: true,
  });
});

test("本地宿主测速拒绝非法 tick 增量", () => {
  const meter = new HostSpeedMeter(() => 0);
  assert.throws(() => meter.recordTicks(-1), /tick/);
  assert.throws(() => meter.recordTicks(0.5), /tick/);
});

test("读档开始后拒绝读档前尚未完成的测速响应", async () => {
  const gate = new SpeedMetricsRequestGate();
  const requestGeneration = gate.capture();
  const delayedMetrics = Promise.resolve({ actual_multiplier: 60 });

  gate.invalidate();
  await delayedMetrics;

  assert.equal(gate.isCurrent(requestGeneration), false);
  assert.equal(gate.isCurrent(gate.capture()), true);
});
