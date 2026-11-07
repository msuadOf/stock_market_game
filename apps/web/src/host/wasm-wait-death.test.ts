import assert from "node:assert/strict";
import test from "node:test";
import { parseWasmWaitDeathLayout, watchWasmDeath } from "./wasm-wait-death.ts";

test("真实 futex 通知唤醒监听，且只报告一次", { timeout: 10000 }, async () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  const layout = parseWasmWaitDeathLayout({ address: 8, bytes: 4 }, memory);
  let calls = 0;
  let fail!: (error: Error) => void;
  const detected = new Promise<void>((resolve, reject) => {
    fail = reject;
    watchWasmDeath(memory, layout, () => { calls++; resolve(); }, reject);
  });
  await Promise.resolve();
  memory.grow(1);
  const words = new Int32Array(memory.buffer, 8, 1);
  Atomics.store(words, 0, 1);
  assert.equal(Atomics.notify(words, 0), 1);
  const timer = setTimeout(() => fail(new Error("DEAD 未唤醒")), 1000);
  try { await detected; } finally { clearTimeout(timer); }
  assert.equal(calls, 1);
});

test("监听前已死亡仍报告，取消后不报告；非法布局与状态显式失败", { timeout: 10000 }, async () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true });
  for (const value of [{ address: 0, bytes: 4 }, { address: 1, bytes: 4 }, { address: 65536, bytes: 4 }, { address: 8, bytes: 1 }, { address: 8, bytes: 4, extra: 1 }]) assert.throws(() => parseWasmWaitDeathLayout(value, memory));
  const layout = parseWasmWaitDeathLayout({ address: 8, bytes: 4 }, memory);
  const words = new Int32Array(memory.buffer, 8, 1);
  Atomics.store(words, 0, 1);
  await new Promise<void>((resolve, reject) => watchWasmDeath(memory, layout, resolve, reject));
  Atomics.store(words, 0, 0);
  let called = false;
  const stop = watchWasmDeath(memory, layout, () => { called = true; }, () => { called = true; });
  stop();
  Atomics.store(words, 0, 1);
  Atomics.notify(words, 0);
  await new Promise((resolve) => setTimeout(resolve, 10));
  assert.equal(called, false);
  Atomics.store(words, 0, 2);
  await assert.rejects(new Promise<void>((resolve, reject) => watchWasmDeath(memory, layout, resolve, reject)), /DEAD/);
});

 test("取消后真实 futex waiter 在有界时间内释放", { timeout: 10000 }, async () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true });
  const layout = parseWasmWaitDeathLayout({ address: 8, bytes: 4 }, memory);
  const stop = watchWasmDeath(memory, layout, () => assert.fail("取消后不得报告死亡"), (error) => assert.fail(error.message));
  await Promise.resolve();
  stop();
  await new Promise((resolve) => setTimeout(resolve, 250));
  assert.equal(Atomics.notify(new Int32Array(memory.buffer, 8, 1), 0), 0, "取消后 waiter 必须退出");
});

test("缺少 waitAsync 时有界轮询死亡、支持取消并报告读取失败", { timeout: 10000 }, async () => {
  const descriptor = Object.getOwnPropertyDescriptor(Atomics, "waitAsync");
  const originalLoad = Atomics.load;
  Object.defineProperty(Atomics, "waitAsync", { configurable: true, value: undefined });
  try {
    const memory = new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true });
    const layout = parseWasmWaitDeathLayout({ address: 8, bytes: 4 }, memory);
    const words = new Int32Array(memory.buffer, 8, 1);
    const detected = new Promise<void>((resolve, reject) => watchWasmDeath(memory, layout, resolve, reject));
    await Promise.resolve();
    Atomics.store(words, 0, 1);
    await detected;
    Atomics.store(words, 0, 0);
    let called = false;
    const stop = watchWasmDeath(memory, layout, () => { called = true; }, () => { called = true; });
    await Promise.resolve();
    stop();
    Atomics.store(words, 0, 1);
    await new Promise((resolve) => setTimeout(resolve, 150));
    assert.equal(called, false);
    Atomics.store(words, 0, 0);
    const failure = new Promise<void>((resolve, reject) => watchWasmDeath(memory, layout, resolve, reject));
    await Promise.resolve();
    Atomics.load = (() => { throw new Error("读取失败"); }) as typeof Atomics.load;
    await assert.rejects(failure, /读取失败/);
  } finally {
    Atomics.load = originalLoad;
    if (descriptor) Object.defineProperty(Atomics, "waitAsync", descriptor);
    else Reflect.deleteProperty(Atomics, "waitAsync");
  }
});
