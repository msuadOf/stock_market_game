import assert from "node:assert/strict";
import test from "node:test";
import { createFacadeEngineInitializer } from "./facade-engine-initialize.ts";
import { HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE } from "./facade-slot-protocol.ts";

test("engine bind→实例化→Rust setup arena/clock→layout→main配置确认后才可 create", { timeout: 10000 }, async () => {
  const channel = new MessageChannel(), calls: string[] = [], setup = { stocks: ["fixture"] };
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  const layout = { baseAddr: 0, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE, joinStride: JOIN_STRIDE, capacity: 8, genBits: 29 };
  const module = new WebAssembly.Module(new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0]));
  const initializer = createFacadeEngineInitializer({
    isolated: () => true, createMemory: () => memory,
    environment: {
      bindFacadeThreadEnvironment: (input) => { assert.equal(input.memory, memory); assert.equal(input.role, "engine"); calls.push("bind"); },
      installFacadeSlotLayout: () => { calls.push("install"); },
      getArenaView: () => ({ words: new Int32Array(memory.buffer), capacity: 8, genBits: 29 }),
    },
    load: async () => { calls.push("load"); return { module, diagnostics: false, glue: {
      initSync: (input) => { calls.push("instantiate"); assert.equal(input.memory, memory); return { memory, __wbindgen_thread_destroy() {} }; },
      facade_slot_init_for_setup: (input) => { calls.push("clock+arena"); assert.deepEqual(input, setup); },
      facade_slot_layout: () => { calls.push("layout"); return layout; },
    } }; },
    onFailure: (error) => { throw error; },
  });
  try {
    assert.throws(() => initializer.requireConfigured(), /configured/);
    const received = new Promise<Record<string, unknown>>((resolve) => channel.port1.addEventListener("message", (event) => {
      if (event.data.type === "facadeEngineMemory") channel.port1.postMessage({ type: "facadeEngineMemoryRegistered", allocationId: 1 });
      else resolve(event.data);
    }));
    channel.port1.start();
    const initializing = initializer.initialize({ type: "init", controlPort: channel.port2, setup });
    const substrate = await received;
    assert.equal(substrate.type, "facadeEngineConfiguration"); assert.deepEqual(substrate.layout, layout);
    assert.deepEqual(calls, ["bind", "load", "instantiate", "clock+arena", "layout", "install"]);
    assert.throws(() => initializer.requireConfigured(), /configured/);
    channel.port1.postMessage({ type: "facadeEngineConfigured" });
    const actual = await initializing; assert.equal(actual.memory, memory);
    assert.deepEqual(initializer.requireConfigured().setup, setup);
    const duplicate = new MessageChannel();
    await assert.rejects(initializer.initialize({ type: "init", controlPort: duplicate.port2, setup }), /重复/);
    duplicate.port1.close(); duplicate.port2.close();
  } finally { initializer.dispose(); channel.port1.close(); channel.port2.close(); }
});

test("engine 非隔离初始化显式拒绝，不实例化WASM也不留下收到的端", { timeout: 10000 }, async () => {
  const channel = new MessageChannel(); let loaded = false;
  const initializer = createFacadeEngineInitializer({ isolated: () => false,
    environment: { bindFacadeThreadEnvironment() {}, installFacadeSlotLayout() {}, getArenaView() { throw new Error("不可达"); } },
    load: async () => { loaded = true; throw new Error("不可达"); }, onFailure: () => {},
  });
  try {
    await assert.rejects(initializer.initialize({ type: "init", setup: {}, controlPort: channel.port2 }), /crossOriginIsolated/);
    assert.equal(loaded, false); assert.throws(() => initializer.requireConfigured());
  } finally { initializer.dispose(); channel.port1.close(); channel.port2.close(); }
});

test("加载中重复 init 关闭新收到端与原端各一次，原初始化不得继续实例化", { timeout: 10000 }, async () => {
  const original = new MessageChannel(), duplicate = new MessageChannel();
  let originalCloses = 0, duplicateCloses = 0;
  const closeOriginal = original.port2.close.bind(original.port2), closeDuplicate = duplicate.port2.close.bind(duplicate.port2);
  original.port2.close = () => { originalCloses++; closeOriginal(); };
  duplicate.port2.close = () => { duplicateCloses++; closeDuplicate(); };
  let release!: () => void;
  const loading = new Promise<void>((resolve) => { release = resolve; });
  let markLoaded!: () => void;
  const loaded = new Promise<void>((resolve) => { markLoaded = resolve; });
  original.port1.addEventListener("message", (event) => {
    if (event.data.type === "facadeEngineMemory") original.port1.postMessage({ type: "facadeEngineMemoryRegistered", allocationId: 1 });
  });
  original.port1.start();
  const initializer = createFacadeEngineInitializer({ isolated: () => true,
    createMemory: () => new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true }),
    environment: { bindFacadeThreadEnvironment() {}, installFacadeSlotLayout() {}, getArenaView() { throw new Error("不可达"); } },
    load: async () => { markLoaded(); await loading; throw new Error("load interrupted"); }, onFailure: () => {},
  });
  try {
    const pending = initializer.initialize({ type: "init", setup: {}, controlPort: original.port2 });
    await loaded;
    await assert.rejects(initializer.initialize({ type: "init", setup: {}, controlPort: duplicate.port2 }), /重复/);
    assert.equal(originalCloses, 1); assert.equal(duplicateCloses, 1);
    release(); await assert.rejects(pending, /load interrupted/);
    assert.equal(originalCloses, 1); assert.equal(duplicateCloses, 1);
  } finally { release(); initializer.dispose(); original.port1.close(); duplicate.port1.close(); }
});
