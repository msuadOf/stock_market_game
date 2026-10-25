import assert from "node:assert/strict";
import test from "node:test";
import { engine_clock_now, engineClockDegradation } from "./engine-clock-env.ts";

// engine 时钟门面 bootstrap（R4a 阶段 1，设计 §7.3.2/§8.3）：
// 唯一合法时间 import env.engine_clock_now 的 JS 侧实现合同。

test("时钟 bootstrap 返回绝对毫秒且随调用单调非递减", () => {
  const first = engine_clock_now();
  let previous = first;
  for (let index = 0; index < 32; index += 1) {
    const current = engine_clock_now();
    assert.ok(current >= previous, `时钟回退：${current} < ${previous}`);
    previous = current;
  }
  // 有 timeOrigin 的环境（Node ≥ 24 基线）返回绝对时间线值（epoch ms 量级）。
  if (engineClockDegradation === "none") {
    assert.ok(first > 1e12, `绝对时间线值应为 epoch 级毫秒，实际 ${first}`);
  }
});

test("timeOrigin 能力位显式登记，不静默", () => {
  // Node 24/浏览器基线均有 performance.timeOrigin——本仓支持面下恒 none；
  // 降级值只能是显式枚举 no-time-origin（§7.3.3 登记语义）。
  assert.ok(engineClockDegradation === "none" || engineClockDegradation === "no-time-origin");
});
