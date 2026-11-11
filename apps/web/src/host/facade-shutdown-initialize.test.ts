import assert from "node:assert/strict";
import test from "node:test";
import { createFacadeEngineInitializer } from "./facade-engine-initialize.ts";
import { createEngineSupervisor } from "./engine-supervisor.ts";
import { HEADER_WORDS, JOIN_STRIDE, SLOT_STRIDE } from "./facade-slot-protocol.ts";

test("真实控制端先交 Memory 再读取故障布局，初始化失败仍回真实释放终值", { timeout: 10000 }, async () => {
  let initMessage!: Record<string, unknown>;
  class Engine extends EventTarget {
    terminated = false;
    postMessage(message: Record<string, unknown>) { initMessage = message; }
    terminate() { this.terminated = true; }
  }
  const engine = new Engine(), memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  const releases: unknown[] = [];
  const supervisor = createEngineSupervisor({ engine: engine as unknown as Worker, createChildWorker: () => { throw new Error("不应创建 child"); },
    onFatalFailure() {}, onSpawnFailure() {}, onFinalRelease: (result) => { releases.push(result); } });
  const module = new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0]));
  const expected = new Error("LibOS fault layout export rejected");
  const initializer = createFacadeEngineInitializer({ isolated: () => true, createMemory: () => memory,
    environment: { bindFacadeThreadEnvironment() {}, installFacadeSlotLayout() {}, getArenaView() { return { words: new Int32Array(memory.buffer), capacity: 4, genBits: 29 }; } },
    load: async () => ({ module, diagnostics: false, glue: {
      wasm_tombstone_init_primary: () => ({ engine: { workerId: 0, address: 60000 }, ingress: { workerId: 0xffffffff, address: 60256 }, death: { address: 65004, bytes: 4 } }),
      initSync: () => ({ memory, __wbindgen_thread_destroy() {} }), set_step_runtime_override() {},
      libos_host_fault_layout() { memory.grow(1); throw expected; },
      facade_slot_init_for_setup() { throw new Error("失败前不能创建 slot"); },
      facade_slot_layout: () => ({ baseAddr: 0, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE, joinStride: JOIN_STRIDE, capacity: 4, genBits: 29 }),
    } }), onFailure() {},
  });
  supervisor.bootstrapEngine({ type: "init", setup: {}, runtimeOverride: true });
  try {
    await assert.rejects(initializer.initialize(initMessage), (error) => error === expected);
    supervisor.markQuiescent(engine as unknown as Worker);
    supervisor.dispose();
    assert.equal(engine.terminated, true);
    assert.deepEqual(releases, [{ kind: "released", finalBytes: 131072, growthStopped: true, referencesDropped: true }], "INIT_FAILED_AFTER_ALLOCATION_RELEASES_REAL_BYTES");
  } finally { initializer.dispose(); supervisor.dispose(); }
});
