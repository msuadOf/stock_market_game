import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { useIndicatorResults } from "./useIndicatorResults.ts";
import type { IndicatorCalculator, IndicatorInput, IndicatorResults } from "./indicator-results.ts";

// 此 fixture 只驱动 hook 的 commit/cleanup 边界，不冒充浏览器或 React DOM 验收。
function hookFixture() {
  const internals = (React as unknown as {
    __SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED: { ReactCurrentDispatcher: { current: unknown } };
  }).__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED.ReactCurrentDispatcher;
  const refs: { current: unknown }[] = [];
  let state: unknown = null;
  let refIndex = 0;
  let dependencies: readonly unknown[] | undefined;
  let cleanup: (() => void) | undefined;
  let commit: (() => void) | undefined;
  const dispatcher = {
    useRef(initial: unknown) {
      const index = refIndex++;
      if (refs[index] === undefined) refs[index] = { current: initial };
      return refs[index];
    },
    useState() { return [state, (value: unknown) => { state = value; }]; },
    useEffect(effect: () => (() => void), next: readonly unknown[]) {
      if (dependencies === undefined || next.some((value, index) => value !== dependencies![index])) {
        commit = () => { cleanup?.(); dependencies = next; cleanup = effect(); };
      }
    },
  };
  return {
    render: function IndicatorFixture(calculator: IndicatorCalculator | null, input: IndicatorInput, enabled = true) {
      const previous = internals.current;
      refIndex = 0;
      try {
        internals.current = dispatcher;
        return useIndicatorResults(calculator, input, enabled);
      } finally { internals.current = previous; }
    },
    commit() { const effect = commit; commit = undefined; effect?.(); },
    unmount() { cleanup?.(); cleanup = undefined; },
  };
}

function response(length: number): IndicatorResults {
  const series = Array.from({ length }, (_, index) => index);
  return {
    macd: { dif: series, dea: series, histogram: series },
    priceKdj: { k: series, d: series, j: series },
    candleKdj: { k: [], d: [], j: [] },
  };
}

const flush = async () => { await new Promise<void>((resolve) => setImmediate(resolve)); };

test("hook 保留 microtask 调用，输入更换及 cleanup 拒绝旧结果", async () => {
  const fixture = hookFixture();
  const pending: ((value: IndicatorResults) => void)[] = [];
  const calculator: IndicatorCalculator = () => new Promise((resolve) => pending.push(resolve));
  const first = { prices: [10] };
  const second = { prices: [10, 11] };
  assert.equal(fixture.render(calculator, first).kind, "pending");
  fixture.commit();
  assert.equal(pending.length, 0);
  await flush();
  assert.equal(pending.length, 1);
  assert.equal(fixture.render(calculator, second).kind, "pending");
  fixture.commit();
  await flush();
  pending[1]!(response(2));
  await flush();
  assert.deepEqual(fixture.render(calculator, second), { kind: "ready", value: response(2) });
  pending[0]!(response(1));
  await flush();
  assert.deepEqual(fixture.render(calculator, second), { kind: "ready", value: response(2) });
  fixture.render(calculator, first);
  fixture.commit();
  await flush();
  fixture.unmount();
  pending[2]!(response(1));
  await flush();
  assert.equal(fixture.render(calculator, first).kind, "pending");
});

test("hook 保留 disabled 优先级、calculator 引用及显式 parse/reject 错误", async () => {
  const fixture = hookFixture();
  const input = { prices: [10] };
  const malformed: IndicatorCalculator = async () => response(0);
  assert.equal(fixture.render(null, input, false).kind, "idle");
  fixture.commit();
  assert.equal(fixture.render(null, input).kind, "unavailable");
  fixture.commit();
  fixture.render(malformed, input);
  fixture.commit();
  await flush();
  assert.deepEqual(fixture.render(malformed, input), { kind: "error", message: "RangeError: MACD DIF长度必须为 1" });
  const rejected: IndicatorCalculator = async () => { throw new Error("宿主计算失败"); };
  assert.equal(fixture.render(rejected, input).kind, "pending");
  fixture.commit();
  await flush();
  assert.deepEqual(fixture.render(rejected, input), { kind: "error", message: "Error: 宿主计算失败" });
  assert.equal(fixture.render(rejected, input, false).kind, "idle");
  fixture.commit();
  fixture.unmount();
});
