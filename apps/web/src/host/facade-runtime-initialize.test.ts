import assert from "node:assert/strict";
import test from "node:test";
import { createFacadeEngineInitializer } from "./facade-engine-initialize.ts";
import { HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE } from "./facade-slot-protocol.ts";

const LAYOUT = { baseAddr: 0, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE, joinStride: JOIN_STRIDE, capacity: 8, genBits: 29 };
const LIBOS_LAYOUT = { globalStateAddress: 50000, domainAbortAddress: 50004, hostReadyAddress: 50008, slots: [] };
const MODULE = new WebAssembly.Module(new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0]));

function fixture(glueMethods: Record<string, unknown> = {}, glueGetters: Record<string, () => unknown> = {}) {
  const channel = new MessageChannel();
  const calls: string[] = [];
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  channel.port1.addEventListener("message", (event) => {
    if (event.data.type === "facadeEngineMemory") channel.port1.postMessage({ type: "facadeEngineMemoryRegistered", allocationId: 1 });
    if (event.data.type === "facadeEngineDeath") { calls.push("death-register"); channel.port1.postMessage({ type: "facadeEngineDeathRegistered", allocationId: 1 }); }
  });
  channel.port1.start();
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
      const glue = {
        initSync: () => { calls.push("instantiate"); return { memory, __wbindgen_thread_destroy() {} }; },
        facade_slot_init_for_setup: () => { calls.push("setup"); },
        facade_slot_layout: () => { calls.push("layout"); return LAYOUT; },
        wasm_tombstone_init_primary: () => { calls.push("primary"); return { engine: { workerId: 0, address: 60000 }, ingress: { workerId: 0xffffffff, address: 60256 }, death: { address: 65004, bytes: 4 } }; },
        libos_host_fault_layout: () => LIBOS_LAYOUT,
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
  const configured = new Promise<Record<string, unknown>>((resolve) => f.channel.port1.addEventListener("message", (event) => { if (event.data.type === "facadeEngineConfiguration") resolve(event.data); }));
  f.channel.port1.start();
  const pending = f.initializer.initialize({ type: "init", setup: {}, controlPort: f.channel.port2, ...extra });
  const configuration = await configured;
  f.channel.port1.postMessage({ type: "facadeEngineConfigured" });
  await pending;
  return configuration;
}

test("默认初始化不探测 runtime API，保持既有 setup 顺序", { timeout: 10000 }, async () => {
  const f = fixture();
  try {
    await initialize(f);
    assert.deepEqual(f.calls, ["memory", "bind", "load", "instantiate", "primary", "death-register", "setup", "layout", "install"]);
  } finally { f.cleanup(); }
});

test("显式启用 runtime 时设置 runtime 并传递正式 LibOS 故障布局", { timeout: 10000 }, async () => {
  const f = fixture({ set_step_runtime_override(value: boolean) { f.calls.push(`runtime:${value}`); } });
  try {
    const configuration = await initialize(f, { runtimeOverride: true });
    assert.deepEqual(configuration.libosFault, LIBOS_LAYOUT);
    assert.deepEqual(f.calls, ["memory", "bind", "load", "instantiate", "primary", "death-register", "runtime:true", "setup", "layout", "install"]);
  } finally { f.cleanup(); }
});

test("显式关闭 runtime 仅调用一次 runtime setter", { timeout: 10000 }, async () => {
  const f = fixture({ set_step_runtime_override(value: boolean) { f.calls.push(`runtime:${value}`); } });
  try {
    await initialize(f, { runtimeOverride: false });
    assert.deepEqual(f.calls, ["memory", "bind", "load", "instantiate", "primary", "death-register", "runtime:false", "setup", "layout", "install"]);
  } finally { f.cleanup(); }
});

test("显式请求缺少 runtime setter 时 typed 拒绝且不初始化 slot 或配置", { timeout: 10000 }, async () => {
  const f = fixture();
  try {
    await assert.rejects(f.initializer.initialize({ type: "init", setup: {}, controlPort: f.channel.port2, runtimeOverride: true }), (error: unknown) => {
      assert.equal((error as { code?: string }).code, "WASM_RUNTIME_NOT_READY");
      assert.equal((error as { where?: string }).where, "facade-engine-initialize.runtime");
      assert.equal((error as { context?: { operation?: string } }).context?.operation, "set_step_runtime_override");
      return true;
    });
    assert.equal(f.calls.includes("setup"), false);
    assert.equal(f.calls.includes("install"), false);
  } finally { f.cleanup(); }
});

test("非法 runtime flag 和未知 init 字段在分配 Memory 前拒绝", { timeout: 10000 }, async () => {
  for (const extra of [{ runtimeOverride: "true" }, { runtimeExtra: true }]) {
    const f = fixture();
    try {
      await assert.rejects(f.initializer.initialize({ type: "init", setup: {}, controlPort: f.channel.port2, ...extra }));
      assert.deepEqual(f.calls, []);
    } finally { f.cleanup(); }
  }
});

test("runtime setter 异常保持原对象传播", { timeout: 10000 }, async () => {
  const setterError = new Error("原 setter 故障");
  const f = fixture({ set_step_runtime_override() { throw setterError; } });
  try {
    await assert.rejects(f.initializer.initialize({ type: "init", setup: {}, controlPort: f.channel.port2, runtimeOverride: true }), (error) => error === setterError);
    assert.equal(f.calls.includes("setup"), false);
    assert.equal(f.calls.includes("install"), false);
  } finally { f.cleanup(); }
});

test("runtimeOverride getter 只读取一次并消费已校验原值", { timeout: 10000 }, async () => {
  let reads = 0;
  const f = fixture({ set_step_runtime_override(value: boolean) { f.calls.push(`runtime:${value}`); } });
  const value = { type: "init", setup: {}, controlPort: f.channel.port2, get runtimeOverride() { reads += 1; return reads === 1; } };
  try {
    const configured = new Promise<void>((resolve) => f.channel.port1.addEventListener("message", (event) => { if (event.data.type === "facadeEngineConfiguration") resolve(); }));
    f.channel.port1.start();
    const pending = f.initializer.initialize(value);
    await configured;
    f.channel.port1.postMessage({ type: "facadeEngineConfigured" });
    await pending;
    assert.equal(reads, 1);
    assert.ok(f.calls.includes("runtime:true"));
  } finally { f.cleanup(); }
});

test("未请求 runtime 时不探测 LibOS 故障布局 getter", { timeout: 10000 }, async () => {
  const expected = new Error("不应读取的 LibOS 布局");
  let reads = 0;
  const f = fixture({}, { libos_host_fault_layout() { reads += 1; throw expected; } });
  try {
    await initialize(f);
    assert.equal(reads, 0);
  } finally { f.cleanup(); }
});
