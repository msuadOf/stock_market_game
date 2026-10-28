import assert from "node:assert/strict";
import test from "node:test";
import { createEngineSupervisor, type FacadeSubstrate } from "./engine-supervisor.ts";
import { Clock, WorkerHandle } from "./worker-host-w1.test-support.ts";
import { CAPACITY, GEN_BITS, HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE } from "./facade-slot-protocol.ts";

test("W1 getSubstrate 在 configured 前/销毁后拒绝，只借用已 freeze 的唯一引用", { timeout: 10000 }, () => {
  const engine = new WorkerHandle(), clock = new Clock();
  const channels: MessageChannel[] = [];
  const supervisor = createEngineSupervisor({ engine: engine as unknown as Worker, createChildWorker: () => { throw new Error("本测试禁止创建 child"); }, createChannel: () => { const channel = new MessageChannel(); channels.push(channel); return channel; }, onFatalFailure: (failure) => { throw new Error(failure.message); }, onSpawnFailure: (failure) => { throw new Error(failure.message); }, timers: { now: () => clock.now, setTimeout: clock.setTimeout, clearTimeout: clock.clearTimeout } });
  // 冻结新 API 的运行时合同；pre-host 能导入/执行，缺少行为才红，禁止靠 TypeScript 报错代替红。
  const getSubstrate = () => (supervisor as unknown as { getSubstrate(): Readonly<FacadeSubstrate> }).getSubstrate();
  try {
    assert.equal(typeof (supervisor as unknown as { getSubstrate?: unknown }).getSubstrate, "function", "必须提供借用已配置 substrate 的 API");
    assert.throws(getSubstrate, /configured|装入|未配置/, "未 configured 必须有领域错误，不能是缺方法 TypeError");
    const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
    const words = new Int32Array(memory.buffer); Atomics.store(words, CAPACITY, 4); Atomics.store(words, GEN_BITS, 29);
    const module = new WebAssembly.Module(new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0]));
    const layout = { baseAddr: 0, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE, joinStride: JOIN_STRIDE, capacity: 4, genBits: 29 };
    supervisor.installSubstrate({ module, memory, layout, diagnostics: false });
    const before = supervisor.snapshot(); const first = getSubstrate(), second = getSubstrate();
    assert.equal(first, second); assert.equal(first.module, module); assert.equal(first.memory, memory);
    assert.ok(Object.isFrozen(first)); assert.ok(Object.isFrozen(first.layout));
    assert.deepEqual(first.layout, layout); assert.deepEqual(supervisor.snapshot(), before);
    supervisor.dispose(); assert.throws(getSubstrate, /dispose|销毁|configured/);
    assert.equal(engine.terminations, 1); assert.equal(clock.tasks.size, 0);
  } finally { supervisor.dispose(); for (const channel of channels) { channel.port1.close(); channel.port2.close(); } }
});
