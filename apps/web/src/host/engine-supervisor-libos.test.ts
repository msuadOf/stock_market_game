import assert from "node:assert/strict";
import test from "node:test";
import { createEngineSupervisor } from "./engine-supervisor.ts";
import { CAPACITY, GEN_BITS, HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE, SlotState, encodeW, slotWordOffset } from "./facade-slot-protocol.ts";
import type { HostFailure } from "./host-update.ts";

class WorkerHandle extends EventTarget {
  postMessage(): void {}
  terminate(): void {}
}
class Port extends EventTarget {
  start(): void {}
  close(): void {}
  postMessage(): void {}
  emit(data: unknown): void { this.dispatchEvent(new MessageEvent("message", { data })); }
}
function fixture() {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true });
  const words = new Int32Array(memory.buffer);
  const layout = { baseAddr: 0, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE, joinStride: JOIN_STRIDE, capacity: 2, genBits: 29 };
  Atomics.store(words, CAPACITY, layout.capacity); Atomics.store(words, GEN_BITS, layout.genBits);
  Atomics.store(new Int32Array(memory.buffer, 8192, 1), 0, 1);
  const fault = { globalStateAddress: 8192, domainAbortAddress: 8200, hostReadyAddress: 8220, slots: [{abortAddress: 8204, waitAddress: 8208}, {abortAddress: 8212, waitAddress: 8216}] };
  const engine = new WorkerHandle(), control = new Port();
  const pending: HostFailure[] = [], fatal: HostFailure[] = [];
  let children = 0;
  const supervisor = createEngineSupervisor({
    engine: engine as unknown as Worker,
    createChildWorker: () => { children++; return new WorkerHandle() as unknown as Worker; },
    createChannel: () => ({port1: control, port2: new Port()}) as unknown as MessageChannel,
    onFatalFailure: failure => { fatal.push(failure); }, onFailurePending: failure => { pending.push(failure); }, onSpawnFailure: () => {},
  });
  supervisor.bootstrapEngine({type:"init"});
  control.emit({type:"facadeEngineMemory", allocationId:1, memory});
  const primary = { engine: {workerId:0,address:5000}, ingress: {workerId:4294967295,address:6000}, death:{address:7000,bytes:4} };
  control.emit({type:"facadeEngineDeath", allocationId:1, primary});
  control.emit({type:"facadeEngineConfiguration", allocationId:1, module:new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0])), layout, diagnostics:false,
    libosFault:fault });
  return {memory, words, layout, engine, control, supervisor, pending, fatal, children: () => children};
}

test("配置握手后的 supervisor fatal 发布宿主故障并唤醒 LibOS 等待", { timeout: 10000 }, async () => {
  const f = fixture();
  try {
    f.supervisor.getSubstrate();
    const waiting = new Int32Array(f.memory.buffer, 8208, 1);
    Atomics.store(waiting, 0, -1);
    const waitAsync = (Atomics as unknown as {waitAsync: (words: Int32Array, index: number, value: number, timeout: number) => {async: boolean; value: Promise<string>}}).waitAsync;
    const wake = waitAsync(waiting, 0, -1, 1000);
    assert.equal(wake.async, true);
    f.engine.dispatchEvent(Object.assign(new Event("error"), { error: new Error("测试宿主故障") }));
    assert.equal(await wake.value, "ok");
    assert.equal(Atomics.load(new Uint8Array(f.memory.buffer, 8200, 1), 0), 1);
    assert.equal(f.pending.length, 1);
  } finally { f.supervisor.dispose(); }
});

test("child tombstone 与 LibOS 地址冲突时不创建 child", { timeout: 10000 }, () => {
  const f = fixture();
  try {
    f.supervisor.getSubstrate();
    Atomics.store(f.words, slotWordOffset(0), encodeW(0, SlotState.Req, 29) | 0);
    f.control.emit({type:"facadeSpawnV2", slot:0, gen:0, name:"冲突", recordAddr:8192});
    assert.equal(f.children(), 0);
    assert.equal(f.pending.length, 1);
    assert.equal(f.pending[0].code, "FACADE_PROTOCOL_ERROR");
    assert.match(f.pending[0].message, /记录地址非法/);
  } finally { f.supervisor.dispose(); }
});

test("LibOS 槽故障写失败仍唤醒等待并保留具体失败原因", { timeout: 10000 }, () => {
  const f = fixture();
  const originalStore = Atomics.store;
  try {
    f.supervisor.getSubstrate();
    Atomics.store = ((array: Int32Array | Uint8Array, index: number, value: number) => {
      if (array instanceof Uint8Array && array.byteOffset === 8204) throw new Error("LibOS 槽故障标记写失败");
      return originalStore(array as Int32Array, index, value);
    }) as typeof Atomics.store;
    f.engine.dispatchEvent(Object.assign(new Event("error"), { error: new Error("初始故障") }));
    assert.equal(Atomics.load(new Uint8Array(f.memory.buffer, 8200, 1), 0), 1);
    assert.equal(f.pending.length, 1);
    Atomics.store = originalStore;
    f.supervisor.beginShutdown(performance.now());
    f.supervisor.finishFailureAtDeadline({code:"TEST", where:"test", message:"期限到达"});
    assert.equal(f.fatal.length, 1);
    assert.match(JSON.stringify(f.fatal[0]), /LibOS 槽故障标记写失败/);
  } finally { Atomics.store = originalStore; f.supervisor.dispose(); }
});

test("两条故障通知均失败时保留两个原因", { timeout: 10000 }, () => {
  const f = fixture();
  const originalStore = Atomics.store;
  try {
    f.supervisor.getSubstrate();
    Atomics.store = ((array: Int32Array | Uint8Array, index: number, value: number) => {
      if (array instanceof Uint8Array && array.byteOffset === 8204) throw new Error("LibOS 槽通知失败", {cause:{detail:"保留槽通知根因"}});
      if (array instanceof Uint8Array && array.byteOffset === 8200) throw new Error("LibOS 通知失败");
      return originalStore(array as Int32Array, index, value);
    }) as typeof Atomics.store;
    f.engine.dispatchEvent(Object.assign(new Event("error"), {error:new Error("原始故障")}));
    Atomics.store = originalStore;
    f.supervisor.beginShutdown(performance.now());
    f.supervisor.finishFailureAtDeadline({code:"TEST", where:"test", message:"期限到达"});
    assert.equal(f.fatal.length, 1);
    const report = JSON.stringify(f.fatal[0]);
    assert.match(report, /LibOS 槽通知失败/); assert.match(report, /LibOS 通知失败/); assert.match(report, /保留槽通知根因/);
  } finally { Atomics.store = originalStore; f.supervisor.dispose(); }
});

test("LibOS 域通知失败时仍保留全局死亡标记并报告错误", { timeout: 10000 }, () => {
  const f = fixture();
  const originalStore = Atomics.store;
  try {
    f.supervisor.getSubstrate();
    Atomics.store = ((array: Int32Array | Uint8Array, index: number, value: number) => {
      if (array instanceof Uint8Array && array.byteOffset === 8200) throw new Error("LibOS 写失败");
      return originalStore(array as Int32Array, index, value);
    }) as typeof Atomics.store;
    f.engine.dispatchEvent(Object.assign(new Event("error"), {error:new Error("原始故障")}));
    assert.equal(Atomics.load(new Int32Array(f.memory.buffer, 8192, 1), 0), 3);
    Atomics.store = originalStore;
    f.supervisor.beginShutdown(performance.now());
    f.supervisor.finishFailureAtDeadline({code:"TEST", where:"test", message:"期限到达"});
    assert.equal(f.fatal.length, 1);
    assert.match(JSON.stringify(f.fatal[0]), /LibOS 写失败/);
  } finally { Atomics.store = originalStore; f.supervisor.dispose(); }
});
