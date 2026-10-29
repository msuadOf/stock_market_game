import assert from "node:assert/strict";
import test from "node:test";
import { createEngineSupervisor } from "./engine-supervisor.ts";
import { CAPACITY, GEN_BITS, HEADER_WORDS, JOIN_STRIDE, SLOT_STRIDE } from "./facade-slot-protocol.ts";

class Port extends EventTarget {
  sent: unknown[] = []; start() {} close() {}
  postMessage(value: unknown) { this.sent.push(value); }
  emit(value: unknown) { this.dispatchEvent(new MessageEvent("message", { data: value })); }
}
class OwnedWorker extends EventTarget {
  terminations = 0; sent: unknown[] = [];
  postMessage(value: unknown) { this.sent.push(value); }
  terminate() { this.terminations++; }
}
function fixture() {
  const engine = new OwnedWorker();
  const channels: { port1: Port; port2: Port }[] = [];
  const releases: unknown[] = [], failures: unknown[] = [];
  const timers = new Map<number, () => void>(); let timerId = 0;
  const supervisor = createEngineSupervisor({ engine: engine as unknown as Worker,
    createChildWorker: () => new OwnedWorker() as unknown as Worker,
    createChannel: () => { const channel = { port1: new Port(), port2: new Port() }; channels.push(channel); return channel as unknown as MessageChannel; },
    onFatalFailure: (failure) => { failures.push(failure); }, onSpawnFailure() {},
    onFinalRelease: (receipt: unknown) => { releases.push(receipt); },
    timers: { now: () => 0, setTimeout: (callback) => { timers.set(++timerId, callback); return timerId; }, clearTimeout: (id) => { timers.delete(id); } },
  });
  return { supervisor, engine, channels, releases, failures };
}
function substrate(memory: WebAssembly.Memory) {
  const layout = { baseAddr: 0, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE, joinStride: JOIN_STRIDE, capacity: 4, genBits: 29 };
  const words = new Int32Array(memory.buffer); Atomics.store(words, CAPACITY, 4); Atomics.store(words, GEN_BITS, 29);
  return { module: new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0])), memory, layout, diagnostics: false };
}

test("create 途中已登记 Memory 即使未 configured，也在 terminate 后产生一次真实终值", { timeout: 10000 }, () => {
  const f = fixture(), memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  f.supervisor.bootstrapEngine({ type: "init", setup: {} });
  f.channels[0].port1.emit({ type: "facadeEngineMemory", allocationId: 1, memory });
  assert.equal(f.failures.length, 0, "EARLY_MEMORY_MUST_REGISTER");
  memory.grow(1); f.supervisor.markQuiescent(f.engine as unknown as Worker); f.supervisor.dispose(); f.supervisor.dispose();
  assert.equal(f.engine.terminations, 1);
  assert.deepEqual(f.releases, [{ kind: "released", finalBytes: 131072, growthStopped: true, referencesDropped: true }], "FINAL_BYTES_AFTER_TERMINATE_ONCE");
});

test("真实 MessageChannel 的 Memory wrapper 不同；configure 只按已登记 allocation identity 关联", { timeout: 10000 }, async () => {
  const channel = new MessageChannel();
  try {
    const original = new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true });
    const cloned = await new Promise<WebAssembly.Memory>((resolve) => {
      channel.port1.onmessage = (event) => { resolve(event.data); }; channel.port2.postMessage(original);
    });
    assert.notEqual(cloned, original);
    const data = substrate(original), f = fixture(); f.supervisor.bootstrapEngine({ type: "init", setup: {} });
    f.channels[0].port1.emit({ type: "facadeEngineMemory", allocationId: 1, memory: cloned });
    f.channels[0].port1.emit({ type: "facadeEngineConfiguration", allocationId: 1, module: data.module, layout: data.layout, diagnostics: false });
    assert.equal(f.failures.length, 0, "CLONED_MEMORY_MUST_CONFIGURE");
    assert.equal(f.supervisor.getSubstrate().memory, cloned);
    f.supervisor.dispose();
  } finally { channel.port1.close(); channel.port2.close(); }
});

test("fatal 写 1B abort 在终止前，正常 dispose 不写 abort", { timeout: 10000 }, () => {
  for (const fatal of [false, true]) {
    const f = fixture(), memory = new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true });
    const data = substrate(memory), bytes = new Uint8Array(memory.buffer); bytes.set([57,0,91], 1000);
    f.supervisor.installSubstrate({ ...data, waitAbort: { address: 1001, bytes: 1 } });
    let abortAtTerminate = -1; f.engine.terminate = () => { f.engine.terminations++; abortAtTerminate = bytes[1001]; };
    if (fatal) f.engine.dispatchEvent(Object.assign(new Event("error"), { message: "trap", error: new Error("trap") }));
    else f.supervisor.dispose();
    assert.equal(abortAtTerminate, fatal ? 1 : 0, "FATAL_ABORT_BEFORE_TERMINATE_NORMAL_STAYS_ZERO");
    assert.deepEqual([...bytes.slice(1000,1003)], [57,fatal ? 1 : 0,91]);
  }
});

test("已经发 init 却没有 Memory 事实的失败不能伪造 not-allocated 或 0 bytes", { timeout: 10000 }, () => {
  const f = fixture(); f.supervisor.bootstrapEngine({ type: "init", setup: {} }); f.supervisor.dispose();
  assert.equal(f.releases.length, 1, "UNKNOWN_ALLOCATION_MUST_SETTLE_EXPLICIT_ERROR");
  assert.ok(f.releases[0] instanceof Error);
});

test("fatal terminate 返回没有 quiescence 证明，release 必须未知", { timeout: 10000 }, () => {
  const f = fixture(); const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  f.supervisor.installSubstrate(substrate(memory));
  f.supervisor.discard({ code: "TRAP", where: "fixture", message: "仍可能正在 WASM 执行" });
  assert.equal(f.engine.terminations, 1);
  assert.equal(f.releases.length, 1);
  assert.ok(f.releases[0] instanceof Error);
  assert.match(String(f.releases[0]), /quiescence|停止 grow/);
});

test("port 清理失败仍尝试 terminate，release 保持未知并保清理错误", { timeout: 10000 }, () => {
  const f = fixture(); const memory = new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true });
  f.supervisor.installSubstrate(substrate(memory));
  f.supervisor.markQuiescent(f.engine as unknown as Worker);
  f.channels[0].port1.close = () => { throw new Error("PORT_CLOSE_ORIGINAL"); };
  const release = f.supervisor.dispose();
  assert.equal(f.engine.terminations, 1);
  assert.ok(release instanceof AggregateError);
  assert.match(String((release as AggregateError).errors[0].errors[0]), /PORT_CLOSE_ORIGINAL/);
  assert.equal(f.supervisor.snapshot().workers, 0); assert.equal(f.supervisor.snapshot().ports, 0);
});

test("terminate 抛错不能报告 growthStopped，仍只结一个未知 release", { timeout: 10000 }, () => {
  const f = fixture(); const memory = new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true });
  f.supervisor.installSubstrate(substrate(memory)); f.supervisor.markQuiescent(f.engine as unknown as Worker);
  f.engine.terminate = () => { f.engine.terminations++; throw new Error("TERMINATE_ORIGINAL"); };
  const first = f.supervisor.dispose(); const second = f.supervisor.dispose();
  assert.equal(first, second); assert.ok(first instanceof Error); assert.equal(f.releases.length, 1); assert.equal(f.engine.terminations, 1);
});

test("真实 pre-Memory 关闭证明允许 not-allocated，已登记 Memory 时拒绝矛盾证明", { timeout: 10000 }, () => {
  const f = fixture(); f.supervisor.bootstrapEngine({ type: "init", initialArchive: {} });
  f.supervisor.markEngineUnallocated(f.engine as unknown as Worker);
  assert.deepEqual(f.supervisor.dispose(), { kind: "not-allocated" });
  const allocated = fixture(); allocated.supervisor.bootstrapEngine({ type: "init", initialArchive: {} });
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true });
  allocated.channels[0].port1.emit({ type: "facadeEngineMemory", allocationId: 1, memory });
  assert.throws(() => allocated.supervisor.markEngineUnallocated(allocated.engine as unknown as Worker), /矛盾/);
  assert.ok(allocated.supervisor.dispose() instanceof Error);
});
