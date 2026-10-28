import assert from "node:assert/strict";
import test from "node:test";
import { observeJsHeap, WasmMemoryObservationError } from "./wasm-memory-observation.ts";

const typed = (where: string) => (error: unknown) => error instanceof WasmMemoryObservationError &&
  error.code === "MEMORY_OBSERVATION_FAILED" && error.where === where && error.recoveryActions.length > 0;

test("JS观测严格读取totalJSHeapSize bytes，不拿used或heap limit替代", () => {
  assert.deepEqual(observeJsHeap({ memory: { totalJSHeapSize: 123456, usedJSHeapSize: 123, jsHeapSizeLimit: 999999 } }),
    { kind: "observed", source: "performance.memory.totalJSHeapSize", bytes: 123456 });
  assert.throws(() => observeJsHeap({ memory: { usedJSHeapSize: 123, jsHeapSizeLimit: 999999 } }), typed("wasm-memory-observation.performance.memory.totalJSHeapSize"));
});

test("API不存在显式js_face_unobservable，绝不以0作为量测结果", () => {
  for (const source of [undefined, null, {}]) {
    const result = observeJsHeap(source);
    assert.equal(result.kind, "unobservable");
    if (result.kind === "unobservable") { assert.equal(result.capability, "js_face_unobservable"); assert.ok(result.reason.length > 0); }
    assert.equal(Object.hasOwn(result, "bytes"), false);
  }
});

test("source/memory存在但形状损坏不是API缺失，不静默降级", () => {
  for (const source of [1, "performance", false]) assert.throws(() => observeJsHeap(source), typed("wasm-memory-observation.performance"));
  for (const memory of [undefined, null, 1, "memory", false, []]) assert.throws(() => observeJsHeap({ memory }), typed("wasm-memory-observation.performance.memory"));
});

test("totalJSHeapSize必须为非负safe integer bytes，允许真实0但不容错误单位/非有限值", () => {
  for (const bad of [undefined, null, -1, 1.5, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1, "256MiB", 1n]) {
    assert.throws(() => observeJsHeap({ memory: { totalJSHeapSize: bad } }), typed("wasm-memory-observation.performance.memory.totalJSHeapSize"));
  }
  assert.deepEqual(observeJsHeap({ memory: { totalJSHeapSize: 0 } }), { kind: "observed", source: "performance.memory.totalJSHeapSize", bytes: 0 });
});

test("source与total getter失败保精确where和同一原cause，不能假称unobservable", () => {
  const first = new Error("performance.memory blocked");
  assert.throws(() => observeJsHeap({ get memory() { throw first; } }),
    (error) => typed("wasm-memory-observation.performance.memory")(error) && (error as Error).cause === first);
  const second = new Error("totalJSHeapSize blocked");
  assert.throws(() => observeJsHeap({ memory: { get totalJSHeapSize() { throw second; } } }),
    (error) => typed("wasm-memory-observation.performance.memory.totalJSHeapSize")(error) && (error as Error).cause === second);
});

test("每次调用fresh读取total，采集不隐式调用主动measure或触发GC", () => {
  let reads = 0;
  const source = { memory: { get totalJSHeapSize() { reads += 1; return reads * 100; } }, measureUserAgentSpecificMemory() { throw new Error("不应主动measure"); } };
  assert.equal(observeJsHeap(source).kind, "observed");
  const result = observeJsHeap(source);
  assert.deepEqual(result, { kind: "observed", source: "performance.memory.totalJSHeapSize", bytes: 200 });
  assert.equal(reads, 2);
});


test("继承的memory API有效，Proxy presence/get失败分阶段保原cause", () => {
  const inherited = Object.create({ memory: { totalJSHeapSize: 42 } });
  assert.deepEqual(observeJsHeap(inherited), { kind: "observed", source: "performance.memory.totalJSHeapSize", bytes: 42 });
  const sourceHas = new Error("source has failed");
  assert.throws(() => observeJsHeap(new Proxy({}, { has() { throw sourceHas; } })),
    (error) => typed("wasm-memory-observation.performance.memory.has")(error) && (error as Error).cause === sourceHas);
  const totalHas = new Error("total has failed");
  const memory = new Proxy({}, { has() { throw totalHas; } });
  assert.throws(() => observeJsHeap({ memory }),
    (error) => typed("wasm-memory-observation.performance.memory.totalJSHeapSize.has")(error) && (error as Error).cause === totalHas);
});
