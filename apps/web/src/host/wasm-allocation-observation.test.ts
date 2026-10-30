import assert from "node:assert/strict";
import test from "node:test";
import { createEngineSupervisor } from "./engine-supervisor.ts";
import { CAPACITY, GEN_BITS, HEADER_WORDS, JOIN_STRIDE, SLOT_STRIDE, SLOT_JOIN_INDEX, SlotState, JoinState, encodeW, encodeJoin, slotFieldOffset, slotWordOffset, joinWordOffset } from "./facade-slot-protocol.ts";
import type { WasmLifecycleObservation } from "./wasm-lifecycle-observation.ts";

class OwnedWorker extends EventTarget {
  input?: Record<string, unknown>;
  postMessage(value: Record<string, unknown>) { this.input = value; }
  terminate() {}
}
const flush = async () => { for (let count = 0; count < 8; count++) await new Promise((resolve) => setImmediate(resolve)); };

test("allocation 数字观察只在真实 Memory 登记、configuration 和 child ready 控制事件后产生", { timeout: 10000 }, async () => {
  const engine = new OwnedWorker(), children: OwnedWorker[] = [], channels: MessageChannel[] = [];
  const events: WasmLifecycleObservation[] = [];
  const supervisor = createEngineSupervisor({ engine: engine as unknown as Worker,
    createChildWorker: () => { const child = new OwnedWorker(); children.push(child); return child as unknown as Worker; },
    createChannel: () => { const channel = new MessageChannel(); channels.push(channel); return channel; },
    timers: { now: () => performance.now(), setTimeout: (callback, delay) => setTimeout(callback, delay) as unknown as number, clearTimeout: (id) => clearTimeout(id) },
    onFatalFailure: (failure) => { throw new Error(failure.message); }, onSpawnFailure: (failure) => { throw new Error(failure.message); }, onLifecycleObservation: (event) => { events.push(event); },
  });
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true });
  const layout = { baseAddr: 0, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE, joinStride: JOIN_STRIDE, capacity: 4, genBits: 29 };
  const words = new Int32Array(memory.buffer); Atomics.store(words, CAPACITY, 4); Atomics.store(words, GEN_BITS, 29);
  try {
    supervisor.bootstrapEngine({ type: "init", setup: {} });
    const control = engine.input!.controlPort as MessagePort;
    control.start(); assert.deepEqual([...events], []);
    control.postMessage({ type: "facadeEngineMemory", allocationId: 1, memory }); await flush();
    assert.deepEqual(events, [{ type: "memoryRegistered", allocationId: 1, role: "engine" }]);
    control.postMessage({ type: "facadeEngineConfiguration", allocationId: 1, module: new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0])), diagnostics: false, layout }); await flush();
    assert.deepEqual(events[1], { type: "allocationConfigured", allocationId: 1, role: "engine", workerId: null, slot: null, gen: null });
    Atomics.store(words, slotFieldOffset(0, SLOT_JOIN_INDEX), 0); Atomics.store(words, joinWordOffset(0,4), encodeJoin(0, JoinState.Waiting,29)); Atomics.store(words, slotWordOffset(0), encodeW(0, SlotState.Req,29));
    control.postMessage({ type: "facadeSpawn", slot: 0, gen: 0, name: "group-0" }); await flush();
    assert.deepEqual(events[2], { type: "childSpawn", workerId: 1, slot: 0, gen: 0, name: "stock-game:0:child:1:0:0", runtimeName: "group-0" });
    assert.equal(events.filter((event) => event.type === "allocationConfigured").length, 1, "childSpawn 不是 allocationConfigured 证据");
    const childPort = children[0].input!.controlPort as MessagePort;
    childPort.postMessage({ type: "facadeChildReady", workerId: 1, slot: 0, gen: 0 }); await flush();
    assert.deepEqual(events[3], { type: "allocationConfigured", allocationId: 1, role: "child", workerId: 1, slot: 0, gen: 0 });
    for (const event of events) assert.equal(Object.values(event).some((value) => value instanceof WebAssembly.Memory || value instanceof SharedArrayBuffer || ArrayBuffer.isView(value)), false);
  } finally { supervisor.dispose(); for (const channel of channels) { channel.port1.close(); channel.port2.close(); } }
});
