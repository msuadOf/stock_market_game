import assert from "node:assert/strict";
import test from "node:test";
import { createFacadeEngineInitializer } from "./facade-engine-initialize.ts";
import { HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE } from "./facade-slot-protocol.ts";

const WAIT_SETTINGS = { spinUs: 11, quietUs: 22, fineUs: 33, coarseUs: 44 };
const LAYOUT = { baseAddr: 0, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE, joinStride: JOIN_STRIDE, capacity: 8, genBits: 29 };
const MODULE = new WebAssembly.Module(new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0]));

function fixture(glueMethods: Record<string, unknown> = {}, onLoad: () => void = () => {}, glueGetters: Record<string, () => unknown> = {}) {
  const channel = new MessageChannel();
  channel.port1.addEventListener("message", (event) => {
    if (event.data.type === "facadeEngineMemory") channel.port1.postMessage({ type: "facadeEngineMemoryRegistered", allocationId: 1 });
  });
  channel.port1.start();
  const calls: string[] = [];
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  const initializer = createFacadeEngineInitializer({
    isolated: () => true,
    createMemory: () => { calls.push("memory"); return memory; },
    environment: {
      bindFacadeThreadEnvironment: () => { calls.push("bind"); },
      installFacadeSlotLayout: () => { calls.push("install"); },
      getArenaView: () => ({ words: new Int32Array(memory.buffer), capacity: 8, genBits: 29 }),
    },
    load: async () => {
      calls.push("load");
      onLoad();
      const glue = {
        initSync: () => { calls.push("instantiate"); return { memory, __wbindgen_thread_destroy() {} }; },
        facade_slot_init_for_setup: () => { calls.push("setup"); },
        facade_slot_layout: () => { calls.push("layout"); return LAYOUT; },
        wasm_wait_abort_layout: () => ({ address: 65000, bytes: 1 }),
        ...glueMethods,
      };
      for (const [key, getter] of Object.entries(glueGetters)) Object.defineProperty(glue, key, { get: getter });
      return { module: MODULE, diagnostics: false, glue: glue as never };
    },
    onFailure: (error) => { throw error; },
  });
  return { channel, calls, initializer, cleanup() { initializer.dispose(); channel.port1.close(); channel.port2.close(); } };
}

async function initialize(f: ReturnType<typeof fixture>, extra: Record<string, unknown> = {}) {
  const configured = new Promise<void>((resolve) => f.channel.port1.addEventListener("message", (event) => { if (event.data.type === "facadeEngineConfiguration") resolve(); }));
  f.channel.port1.start();
  const pending = f.initializer.initialize({ type: "init", setup: {}, controlPort: f.channel.port2, ...extra });
  await configured;
  f.channel.port1.postMessage({ type: "facadeEngineConfigured" });
  await pending;
}

test("默认初始化不探测 runtime API，保持既有 setup 顺序", { timeout: 10000 }, async () => {
  const f = fixture();
  try {
    await initialize(f);
    assert.deepEqual(f.calls, ["memory", "bind", "load", "instantiate", "setup", "layout", "install"]);
  } finally { f.cleanup(); }
});

test("显式 true 在实例化后按原数值设置 runtime 和 wait，再执行 setup 与配置", { timeout: 10000 }, async () => {
  const f = fixture({
    set_step_runtime_override(value: boolean) { f.calls.push(`runtime:${value}`); },
    init_wasm_wait(spinUs: number, quietUs: number, fineUs: number, coarseUs: number) {
      f.calls.push(`wait:${spinUs},${quietUs},${fineUs},${coarseUs}`);
    },
  });
  try {
    await initialize(f, { runtimeOverride: true, waitSettings: WAIT_SETTINGS });
    assert.deepEqual(f.calls, ["memory", "bind", "load", "instantiate", "runtime:true", "wait:11,22,33,44", "setup", "layout", "install"]);
  } finally { f.cleanup(); }
});

test("显式 false 仅调用一次 runtime setter", { timeout: 10000 }, async () => {
  const f = fixture({ set_step_runtime_override(value: boolean) { f.calls.push(`runtime:${value}`); } });
  try {
    await initialize(f, { runtimeOverride: false });
    assert.deepEqual(f.calls, ["memory", "bind", "load", "instantiate", "runtime:false", "setup", "layout", "install"]);
  } finally { f.cleanup(); }
});

test("显式请求缺少所需 glue 方法时 typed 拒绝且不初始化 slot 或配置", { timeout: 10000 }, async () => {
  for (const [extra, missing] of [
    [{ runtimeOverride: false }, "set_step_runtime_override"],
    [{ runtimeOverride: true, waitSettings: WAIT_SETTINGS }, "set_step_runtime_override"],
    [{ runtimeOverride: true, waitSettings: WAIT_SETTINGS }, "init_wasm_wait"],
    [{ runtimeOverride: true, waitSettings: WAIT_SETTINGS }, "wasm_wait_abort_layout"],
  ] as const) {
    const methods = missing === "wasm_wait_abort_layout" ? { set_step_runtime_override() {}, init_wasm_wait() {}, wasm_wait_abort_layout: undefined } : missing === "init_wasm_wait" ? { set_step_runtime_override() {} } : {};
    const f = fixture(methods);
    try {
      await assert.rejects(f.initializer.initialize({ type: "init", setup: {}, controlPort: f.channel.port2, ...extra }), (error: unknown) => {
        assert.equal((error as { code?: string }).code, "WASM_RUNTIME_NOT_READY");
        assert.equal((error as { where?: string }).where, "facade-engine-initialize.runtime");
        assert.equal((error as { context?: { operation?: string } }).context?.operation, missing);
        return true;
      });
      assert.equal(f.calls.includes("setup"), false);
      assert.equal(f.calls.includes("install"), false);
      assert.equal(f.calls.some((call) => call.startsWith("runtime:")), false);
    } finally { f.cleanup(); }
  }
});

test("非法 runtime flag / wait 配对与 wait 内容在 Memory 和 load 之前拒绝", { timeout: 10000 }, async () => {
  const invalid = [
    { runtimeOverride: "true" },
    { runtimeOverride: true },
    { runtimeOverride: false, waitSettings: WAIT_SETTINGS },
    { runtimeOverride: true, waitSettings: { ...WAIT_SETTINGS, fineUs: -1 } },
    { runtimeOverride: true, waitSettings: { ...WAIT_SETTINGS, quietUs: 1.5 } },
    { runtimeOverride: true, waitSettings: { ...WAIT_SETTINGS, spinUs: 2 ** 32 } },
    { runtimeOverride: true, waitSettings: { ...WAIT_SETTINGS, extra: 0 } },
    { runtimeOverride: true, waitSettings: null },
    { runtimeOverride: true, runtimeExtra: true, waitSettings: WAIT_SETTINGS },
  ];
  for (const extra of invalid) {
    const f = fixture();
    try {
      await assert.rejects(f.initializer.initialize({ type: "init", setup: {}, controlPort: f.channel.port2, ...extra }));
      assert.deepEqual(f.calls, []);
    } finally { f.cleanup(); }
  }
});

test("runtime setter 与 wait 初始化异常保持原对象传播", { timeout: 10000 }, async () => {
  for (const [methods, extra, expected] of [
    [{ set_step_runtime_override() { throw setterError; }, init_wasm_wait() {} }, { runtimeOverride: true, waitSettings: WAIT_SETTINGS }, setterError],
    [{ set_step_runtime_override() {}, init_wasm_wait() { throw waitError; } }, { runtimeOverride: true, waitSettings: WAIT_SETTINGS }, waitError],
  ] as const) {
    const f = fixture(methods);
    try {
      await assert.rejects(f.initializer.initialize({ type: "init", setup: {}, controlPort: f.channel.port2, ...extra }), (error) => error === expected);
      assert.equal(f.calls.includes("setup"), false);
      assert.equal(f.calls.includes("install"), false);
    } finally { f.cleanup(); }
  }
});

const setterError = new Error("原 setter 故障");
const waitError = new Error("原 wait 故障");


test("异步 load 期间 waitSettings 变更不能改变已校验的 FFI 快照", { timeout: 10000 }, async () => {
  const wait = { ...WAIT_SETTINGS };
  const f = fixture({
    set_step_runtime_override(value: boolean) { f.calls.push(`runtime:${value}`); },
    init_wasm_wait(spinUs: number, quietUs: number, fineUs: number, coarseUs: number) {
      f.calls.push(`wait:${spinUs},${quietUs},${fineUs},${coarseUs}`);
    },
  }, () => { wait.spinUs = -1; });
  try {
    await initialize(f, { runtimeOverride: true, waitSettings: wait });
    assert.deepEqual(f.calls, ["memory", "bind", "load", "instantiate", "runtime:true", "wait:11,22,33,44", "setup", "layout", "install"]);
  } finally { f.cleanup(); }
});

test("runtimeOverride getter 只读取一次并消费已校验原值", { timeout: 10000 }, async () => {
  let reads = 0;
  const f = fixture({
    set_step_runtime_override(value: boolean) { f.calls.push(`runtime:${value}`); },
    init_wasm_wait(spinUs: number, quietUs: number, fineUs: number, coarseUs: number) {
      f.calls.push(`wait:${spinUs},${quietUs},${fineUs},${coarseUs}`);
    },
  });
  const value = {
    type: "init", setup: {}, controlPort: f.channel.port2,
    get runtimeOverride() { reads += 1; return reads === 1; },
    waitSettings: WAIT_SETTINGS,
  };
  try {
    const configured = new Promise<void>((resolve) => f.channel.port1.addEventListener("message", (event) => { if (event.data.type === "facadeEngineConfiguration") resolve(); }));
    f.channel.port1.start();
    const pending = f.initializer.initialize(value);
    await configured;
    f.channel.port1.postMessage({ type: "facadeEngineConfigured" });
    await pending;
    assert.equal(reads, 1);
    assert.deepEqual(f.calls, ["memory", "bind", "load", "instantiate", "runtime:true", "wait:11,22,33,44", "setup", "layout", "install"]);
  } finally { f.cleanup(); }
});

test("显式 false 不读取不需要的 wait API getter", { timeout: 10000 }, async () => {
  const original = new Error("不应触发的 wait getter");
  let reads = 0;
  const f = fixture({
    set_step_runtime_override(value: boolean) { f.calls.push(`runtime:${value}`); },
  }, () => {}, { init_wasm_wait() { reads += 1; throw original; } });
  try {
    await initialize(f, { runtimeOverride: false });
    assert.equal(reads, 0);
    assert.deepEqual(f.calls, ["memory", "bind", "load", "instantiate", "runtime:false", "setup", "layout", "install"]);
  } finally { f.cleanup(); }
});

test("runtimeOverride true 要求 waitSettings 是自有字段", { timeout: 10000 }, async () => {
  const f = fixture({
    set_step_runtime_override() {},
    init_wasm_wait() {},
  });
  const value = Object.assign(Object.create({ waitSettings: WAIT_SETTINGS }), {
    type: "init", setup: {}, controlPort: f.channel.port2, runtimeOverride: true,
  });
  f.channel.port1.addEventListener("message", () => f.channel.port1.postMessage({ type: "facadeEngineConfigured" }));
  f.channel.port1.start();
  try {
    await assert.rejects(f.initializer.initialize(value), /waitSettings/);
    assert.deepEqual(f.calls, []);
  } finally { f.cleanup(); }
});

test("fineUs 为零在 Memory 与 load 之前拒绝", { timeout: 10000 }, async () => {
  const f = fixture();
  try {
    await assert.rejects(f.initializer.initialize({
      type: "init", setup: {}, controlPort: f.channel.port2,
      runtimeOverride: true, waitSettings: { ...WAIT_SETTINGS, fineUs: 0 },
    }));
    assert.deepEqual(f.calls, []);
  } finally { f.cleanup(); }
});

test("coarseUs 为零在 Memory 与 load 之前拒绝", { timeout: 10000 }, async () => {
  const f = fixture();
  try {
    await assert.rejects(f.initializer.initialize({
      type: "init", setup: {}, controlPort: f.channel.port2,
      runtimeOverride: true, waitSettings: { ...WAIT_SETTINGS, coarseUs: 0 },
    }));
    assert.deepEqual(f.calls, []);
  } finally { f.cleanup(); }
});

test("spinUs 与 quietUs 为零保留原数值传入 wait FFI", { timeout: 10000 }, async () => {
  const f = fixture({
    set_step_runtime_override(value: boolean) { f.calls.push(`runtime:${value}`); },
    init_wasm_wait(spinUs: number, quietUs: number, fineUs: number, coarseUs: number) {
      f.calls.push(`wait:${spinUs},${quietUs},${fineUs},${coarseUs}`);
    },
  });
  try {
    await initialize(f, { runtimeOverride: true, waitSettings: { ...WAIT_SETTINGS, spinUs: 0, quietUs: 0 } });
    assert.deepEqual(f.calls, ["memory", "bind", "load", "instantiate", "runtime:true", "wait:0,0,33,44", "setup", "layout", "install"]);
  } finally { f.cleanup(); }
});
