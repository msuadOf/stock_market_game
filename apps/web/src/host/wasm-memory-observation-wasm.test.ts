import assert from "node:assert/strict";
import test from "node:test";
import { runInNewContext } from "node:vm";
import { observeWasmMemory, WasmMemoryObservationError } from "./wasm-memory-observation.ts";

const PAGE_BYTES = 64 * 1024;
const typed = (where: string) => (error: unknown) => error instanceof WasmMemoryObservationError &&
  error.code === "MEMORY_OBSERVATION_FAILED" && error.where === where && error.recoveryActions.length > 0;

test("读取真实shared Memory当前逻辑长度，grow后fresh读取且不改旧收据", () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 3, shared: true });
  const first = observeWasmMemory(memory, 3 * PAGE_BYTES);
  assert.deepEqual(first, { kind: "observed", source: "WebAssembly.Memory.buffer.byteLength", bytes: PAGE_BYTES });
  memory.grow(2);
  assert.equal(observeWasmMemory(memory, 3 * PAGE_BYTES).bytes, 3 * PAGE_BYTES);
  assert.equal(first.bytes, PAGE_BYTES);
});

test("maximum是caller显式构造约束，当前长度不自动成为maxLive或maximum实测", () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  const result = observeWasmMemory(memory, 2 * PAGE_BYTES);
  assert.equal(result.bytes, PAGE_BYTES);
  assert.equal(Object.hasOwn(result, "liveUpperBoundBytes"), false);
  assert.equal(Object.hasOwn(result, "maximumBytes"), false);
  assert.equal(Object.hasOwn(result, "releasedBytes"), false);
  memory.grow(1);
  assert.throws(() => observeWasmMemory(memory, PAGE_BYTES), typed("wasm-memory-observation.maximumBytes"));
});

test("caller maximum必须为正safe integer bytes且整页，不以单位字符串或默认值掩盖错误", () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  for (const bad of [undefined, null, 0, -1, NaN, Infinity, 1.5, PAGE_BYTES + 1, Number.MAX_SAFE_INTEGER + 1, "64KiB"]) {
    assert.throws(() => observeWasmMemory(memory, bad as number), typed("wasm-memory-observation.maximumBytes"));
  }
});

test("伪造buffer形状不能冒充Memory，非shared memory显式拒绝", () => {
  let fakeReads = 0;
  const fake = { get buffer() { fakeReads += 1; return new SharedArrayBuffer(PAGE_BYTES); } };
  for (const input of [fake, {}, null, undefined, 1, new SharedArrayBuffer(PAGE_BYTES)]) {
    assert.throws(() => observeWasmMemory(input, PAGE_BYTES), typed("wasm-memory-observation.memory"));
  }
  assert.equal(fakeReads, 0);
  const unshared = new WebAssembly.Memory({ initial: 1, maximum: 2 });
  assert.throws(() => observeWasmMemory(unshared, 2 * PAGE_BYTES), typed("wasm-memory-observation.shared"));
});

test("原生brand getter支持跨realm真实shared Memory，不信任实例伪造getter", () => {
  const crossRealm: unknown = runInNewContext("new WebAssembly.Memory({initial:1,maximum:2,shared:true})");
  assert.equal(observeWasmMemory(crossRealm, 2 * PAGE_BYTES).bytes, PAGE_BYTES);
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  Object.defineProperty(memory, "buffer", { get() { throw new Error("不能读取这个伪造getter"); } });
  assert.equal(observeWasmMemory(memory, 2 * PAGE_BYTES).bytes, PAGE_BYTES);
});

test("真实零页实例可观测为0，不把未分配逻辑长度伪装成缺失或峰值标定", () => {
  const memory = new WebAssembly.Memory({ initial: 0, maximum: 1, shared: true });
  const result = observeWasmMemory(memory, PAGE_BYTES);
  assert.equal(result.kind, "observed"); assert.equal(result.bytes, 0);
});


test("导入后Memory与SAB prototype覆写不改变已捕获native getter的字节结果", () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  const memoryDescriptor = Object.getOwnPropertyDescriptor(WebAssembly.Memory.prototype, "buffer")!;
  const bufferDescriptor = Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype, "byteLength")!;
  try {
    Object.defineProperty(WebAssembly.Memory.prototype, "buffer", { configurable: true, get() { throw new Error("伪造Memory getter"); } });
    Object.defineProperty(SharedArrayBuffer.prototype, "byteLength", { configurable: true, get() { return 1; } });
    assert.equal(observeWasmMemory(memory, 2 * PAGE_BYTES).bytes, PAGE_BYTES);
    memory.grow(1);
    assert.equal(observeWasmMemory(memory, 2 * PAGE_BYTES).bytes, 2 * PAGE_BYTES);
  } finally {
    Object.defineProperty(WebAssembly.Memory.prototype, "buffer", memoryDescriptor);
    Object.defineProperty(SharedArrayBuffer.prototype, "byteLength", bufferDescriptor);
  }
});

test("缺少SAB或WebAssembly全局时模块仍可导入，WASM调用typed错误而JS采集独立可用", async () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  for (const name of ["SharedArrayBuffer", "WebAssembly"] as const) {
    const descriptor = Object.getOwnPropertyDescriptor(globalThis, name)!;
    try {
      Object.defineProperty(globalThis, name, { configurable: true, writable: true, value: undefined });
      const url = new URL("./wasm-memory-observation.ts", import.meta.url); url.searchParams.set("missing", name);
      const isolated: typeof import("./wasm-memory-observation.ts") = await import(url.href);
      assert.deepEqual(isolated.observeJsHeap({ memory: { totalJSHeapSize: 42 } }), { kind: "observed", source: "performance.memory.totalJSHeapSize", bytes: 42 });
      assert.throws(() => isolated.observeWasmMemory(memory, 2 * PAGE_BYTES), (error) => error instanceof isolated.WasmMemoryObservationError && error.code === "MEMORY_OBSERVATION_FAILED");
    } finally { Object.defineProperty(globalThis, name, descriptor); }
  }
});
