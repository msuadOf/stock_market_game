import assert from "node:assert/strict";
import test from "node:test";
import { createEngineSupervisor } from "./engine-supervisor.ts";
import { CAPACITY, GEN_BITS, FREE_HEAD, FREE_HEAD_EMPTY, HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE, SLOT_JOIN_INDEX, SlotState, JoinState, encodeW, encodeJoin, slotWordOffset, slotFieldOffset, joinWordOffset } from "./facade-slot-protocol.ts";

class Port extends EventTarget {
  sent: unknown[] = []; closes = 0; postError?: Error;
  start() {}
  close() { this.closes++; }
  postMessage(value: unknown) { if (this.postError !== undefined) throw this.postError; this.sent.push(value); }
  emit(value: unknown) { this.dispatchEvent(new MessageEvent("message", { data: value })); }
}
class WorkerHandle extends EventTarget { terminations = 0; postMessage() {} terminate() { this.terminations++; } }
function fixture() {
  const engine = new WorkerHandle(), children: WorkerHandle[] = [], channels: { port1: Port; port2: Port }[] = [], failures: unknown[] = [];
  const timers = new Map<number, () => void>(); let id = 0;
  const supervisor = createEngineSupervisor({ engine: engine as unknown as Worker, createChildWorker: () => { const child = new WorkerHandle(); children.push(child); return child as unknown as Worker; }, createChannel: () => { const channel = { port1: new Port(), port2: new Port() }; channels.push(channel); return channel as unknown as MessageChannel; }, onFatalFailure: (failure) => { failures.push(failure); }, onFailurePending: (failure) => { failures.push(failure); }, onSpawnFailure: (failure) => { failures.push(failure); }, timers: { now: () => 0, setTimeout: (callback) => { timers.set(++id, callback); return id; }, clearTimeout: (timer) => { timers.delete(timer); } } });
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true }), words = new Int32Array(memory.buffer);
  Atomics.store(words, CAPACITY, 4); Atomics.store(words, GEN_BITS, 29); Atomics.store(words, FREE_HEAD, FREE_HEAD_EMPTY | 0);
  const message = { type: "facadeEngineSubstrate", module: new WebAssembly.Module(new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0])), memory, layout: { baseAddr: 0, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE, joinStride: JOIN_STRIDE, capacity: 4, genBits: 29 }, diagnostics: false };
  supervisor.bootstrapEngine({ type: "init", setup: {} });
  function configure() { channels[0].port1.emit(message); }
  return { engine, children, channels, failures, supervisor, memory, words, message, timers, configure };
}

test("仅 engine 自身控制端 substrate 成功后回一次 configured", { timeout: 10000 }, () => {
  const f = fixture(); assert.throws(() => f.supervisor.assertConfigured(), /configured/);
  f.configure(); assert.deepEqual(f.channels[0].port1.sent, [{ type: "facadeEngineConfigured" }]);
  assert.doesNotThrow(() => f.supervisor.assertConfigured()); assert.equal(f.failures.length, 0);
  f.supervisor.dispose();
});

test("重复 substrate 或 global/child/ingress 伪来源均清全员而不确认", { timeout: 10000 }, () => {
  const global = fixture(); global.supervisor.handleGlobalMessage(global.engine as unknown as Worker, global.message);
  assert.equal(global.failures.length, 1); assert.equal(global.channels[0].port1.sent.length, 0);
  const repeated = fixture(); repeated.configure(); repeated.configure();
  assert.equal(repeated.failures.length, 1); assert.equal(repeated.supervisor.snapshot().workers, 0);
  const child = fixture(); child.configure();
  Atomics.store(child.words, slotFieldOffset(0, SLOT_JOIN_INDEX), 0);
  Atomics.store(child.words, joinWordOffset(0, 4), encodeJoin(0, JoinState.Waiting, 29) | 0);
  Atomics.store(child.words, slotWordOffset(0), encodeW(0, SlotState.Req, 29) | 0);
  child.channels[0].port1.emit({ type: "facadeSpawn", slot: 0, gen: 0, name: "child" });
  child.channels[1].port1.emit(child.message);
  assert.equal(child.failures.length, 1); assert.equal(child.supervisor.snapshot().workers, 0);
  const ingress = fixture(), handle = new WorkerHandle(); ingress.configure(); ingress.supervisor.attachIngress(handle as unknown as Worker);
  ingress.supervisor.handleGlobalMessage(handle as unknown as Worker, ingress.message);
  assert.equal(ingress.failures.length, 1); assert.equal(handle.terminations, 1);
});

test("configured post 抛错与 header 漂移均显式清端/Worker/计时器", { timeout: 10000 }, () => {
  for (const kind of ["post", "header"]) {
    const f = fixture();
    if (kind === "post") f.channels[0].port1.postError = new Error("configured post failed");
    else Atomics.store(f.words, CAPACITY, 3);
    f.configure(); assert.equal(f.failures.length, 1); assert.equal(f.supervisor.snapshot().workers, 0);
    assert.equal(f.timers.size, 0); assert.equal(f.channels[0].port1.closes, 1);
  }
});

test("dispose 后旧端 substrate 零共享写且无确认", { timeout: 10000 }, () => {
  const f = fixture(); f.supervisor.dispose(); const before = [...f.words]; f.configure();
  assert.deepEqual([...f.words], before); assert.equal(f.channels[0].port1.sent.length, 0);
});

test("Memory 后登记 primary 并确认，配置前已有 DEAD 仍报告记录", { timeout: 10000 }, async () => {
  const f = fixture();
  f.channels[0].port1.emit({ type: "facadeEngineMemory", allocationId: 1, memory: f.memory });
  const primary = { engine: { workerId: 0, address: 8192 }, ingress: { workerId: 0xffffffff, address: 8704 }, death: { address: 16384, bytes: 4 } };
  f.channels[0].port1.emit({ type: "facadeEngineDeath", allocationId: 1, primary });
  assert.deepEqual(f.channels[0].port1.sent.at(-1), { type: "facadeEngineDeathRegistered", allocationId: 1 });
  const death = new Int32Array(f.memory.buffer, 16384, 1); Atomics.store(death, 0, 1); Atomics.notify(death, 0);
  await new Promise<void>((resolve) => setTimeout(resolve, 30));
  assert.equal(f.failures.length, 1);
  const words = new Uint32Array(f.memory.buffer, 8192, 4), bytes = new Uint8Array(f.memory.buffer, 8192, 256);
  words[1] = 0; bytes[10] = 1; Atomics.store(words, 0, 2);
  f.supervisor.setDeadline(0);
  for (const callback of [...f.timers.values()]) callback();
  const terminal = f.failures[1] as { context: { records: Array<{ identity: { role: string }; tombstone: { state: string } }> } };
  assert.equal(terminal.context.records[0].identity.role, "engine");
  assert.equal(terminal.context.records[0].tombstone.state, "live");
});
