import assert from "node:assert/strict";
import test from "node:test";
import { assertValidSpeedMultiplier } from "./speed.ts";

test("所有宿主共享的倍速边界拒绝 NaN、非正数和负无穷", () => {
  for (const speed of [Number.NaN, 0, -1, Number.NEGATIVE_INFINITY]) {
    assert.throws(() => assertValidSpeedMultiplier(speed), /非法速度倍率/);
  }
});

test("倍速边界接受有限正数和明确的最快模式", () => {
  assert.doesNotThrow(() => assertValidSpeedMultiplier(720));
  assert.doesNotThrow(() => assertValidSpeedMultiplier(Infinity));
});
