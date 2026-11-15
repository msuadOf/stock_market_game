import assert from "node:assert/strict";
import test from "node:test";
import {
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

test("统一测速协议接受故障停机之前完整的采样窗口", () => {
  assert.doesNotThrow(() => parseSpeedMetrics({
    requested: { mode: "fixed", multiplier: 1 },
    actual_multiplier: 1,
    sample_duration_ms: 1_000,
    sample_ticks: 1,
    running: false,
  }));
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
