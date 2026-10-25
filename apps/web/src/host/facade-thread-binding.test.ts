import assert from "node:assert/strict";
import { it } from "node:test";
import { once } from "node:events";
import { Worker } from "node:worker_threads";
import { createFacadeThreadBinding } from "./facade-thread-binding.ts";
import { CAPACITY, GEN_BITS, HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE } from "./facade-slot-protocol.ts";

// 真实 i64 import 的调用边界；不依赖生成 glue 的 number 类型桩。
export function importWait(wait: (address: number, expected: number, timeout: bigint) => number) {
  const text = new TextEncoder();
  const name = (value: string) => [value.length, ...text.encode(value)];
  const section = (id: number, bytes: number[]) => [id, bytes.length, ...bytes];
  const bytes = new Uint8Array([
    0, 97, 115, 109, 1, 0, 0, 0,
    ...section(1, [1, 96, 3, 127, 127, 126, 1, 127]),
    ...section(2, [1, ...name("env"), ...name("facade_futex_wait"), 0, 0]),
    ...section(7, [1, ...name("wait"), 0, 0]),
  ]);
  return new WebAssembly.Instance(new WebAssembly.Module(bytes), {
    env: { facade_futex_wait: wait },
  }).exports.wait as (address: number, expected: number, timeout: bigint) => number;
}

it("真实 WASM import 保留 i64 BigInt 与最高位为 1 的完整 W", { timeout: 10_000 }, () => {
  let observed: unknown[] = [];
  const wait = importWait((address, expected, timeout) => {
    observed = [address, expected, timeout];
    return 1;
  });
  assert.equal(wait(4, 0x80000001, -1n), 1);
  assert.deepEqual(observed, [4, -2147483647, -1n]);
  assert.throws(() => wait(4, 1, 0 as unknown as bigint), TypeError);
});


function fixture(role: "engine" | "child" | "ingress" = "engine", worker = true) {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 3, shared: true });
  const messages: unknown[] = [];
  const binding = createFacadeThreadBinding({ isWorker: () => worker });
  const layout = { baseAddr: 64, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE,
    joinStride: JOIN_STRIDE, capacity: 2, genBits: 29 };
  const words = new Int32Array(memory.buffer, layout.baseAddr);
  Atomics.store(words, CAPACITY, layout.capacity);
  Atomics.store(words, GEN_BITS, layout.genBits);
  const bootstrap = role === "child" ? { workerId: 0x80000001, slot: 1, gen: 7 } : undefined;
  binding.bindFacadeThreadEnvironment({ memory, role, postControl: (message) => messages.push(message), bootstrap });
  return { memory, messages, binding, layout, bootstrap };
}

it("未绑定调用显式报错，shared memory 和一次绑定是硬合同", { timeout: 10_000 }, () => {
  const binding = createFacadeThreadBinding({ isWorker: () => true });
  assert.throws(() => binding.facade_futex_wait(0, 0, 0n), /未绑定/);
  assert.throws(() => binding.facade_futex_notify(0), /未绑定/);
  assert.throws(() => binding.facade_spawn_notify(0, 0, 0, 0), /未绑定/);
  assert.throws(() => binding.facade_worker_exit_notify(0, 0, 0), /未绑定/);
  assert.throws(() => binding.facade_terminate_child(0), /未绑定/);
  assert.throws(() => binding.bindFacadeThreadEnvironment({
    memory: new WebAssembly.Memory({ initial: 1 }), role: "engine", postControl() {},
  }), /shared|共享/);
  const f = fixture();
  assert.throws(() => f.binding.bindFacadeThreadEnvironment({
    memory: f.memory, role: "engine", postControl() {},
  }), /重复/);
});

it("wait 实际按完整 W 和 BigInt 超时比较，并拒绝非法转换", { timeout: 10_000 }, () => {
  const { binding, memory } = fixture();
  const wait = importWait(binding.facade_futex_wait);
  Atomics.store(new Int32Array(memory.buffer), 0, -2147483647);
  assert.equal(wait(0, 0x80000001, 0n), 2);
  assert.equal(wait(0, 0x80000002, -1n), 1);
  assert.equal(wait(0, 0x80000002, BigInt(Number.MAX_SAFE_INTEGER)), 1);
  for (const timeout of [-2n, BigInt(Number.MAX_SAFE_INTEGER) + 1n, 1 as unknown as bigint]) {
    assert.throws(() => binding.facade_futex_wait(0, -2147483647, timeout), /timeout|超时|BigInt/);
  }
  for (const expected of [0.1, NaN, Infinity, -2147483649, 4294967296]) {
    assert.throws(() => binding.facade_futex_wait(0, expected, 0n), /expected|W/);
  }
});

it("地址必须覆盖完整对齐共享字，grow 后 wait/notify 读取新 buffer", { timeout: 10_000 }, () => {
  const { binding, memory } = fixture();
  for (const address of [1, 65534, 65536, -1, 2 ** 32, NaN, 0.5]) {
    assert.throws(() => binding.facade_futex_wait(address, 0, 0n), /地址|wordAddr/);
    assert.throws(() => binding.facade_futex_notify(address), /地址|wordAddr/);
  }
  memory.grow(1);
  assert.equal(binding.facade_futex_wait(65536, 0, 0n), 2);
  assert.doesNotThrow(() => binding.facade_futex_notify(65536));
});

it("main 不可 wait，ingress 可 futex 但不可发线程控制", { timeout: 10_000 }, () => {
  assert.throws(() => fixture("engine", false).binding.facade_futex_wait(0, 0, 0n), /Worker|主线程/);
  assert.throws(() => fixture("engine", false).binding.facade_futex_notify(0), /Worker|主线程/);
  const { binding } = fixture("ingress");
  assert.equal(binding.facade_futex_wait(0, 1, 0n), 1);
  assert.doesNotThrow(() => binding.facade_futex_notify(0));
  assert.throws(() => binding.facade_spawn_notify(0, 0, 0, 0), /ingress|角色/);
  assert.throws(() => binding.facade_terminate_child(1), /ingress|角色/);
  assert.throws(() => binding.facade_worker_exit_notify(1, 0, 0), /ingress|角色/);
});

it("layout 来自导出且一次装入，校验跨语言布局、header 和当前范围", { timeout: 10_000 }, () => {
  const { binding, layout, memory } = fixture();
  assert.throws(() => binding.getArenaView(), /layout|布局/);
  for (const invalid of [
    { ...layout, baseAddr: 65 }, { ...layout, baseAddr: 65532 },
    { ...layout, slotStride: SLOT_STRIDE + 1 }, { ...layout, joinStride: JOIN_STRIDE + 1 },
    { ...layout, headerWords: HEADER_WORDS + 1 }, { ...layout, capacity: 0 },
    { ...layout, capacity: 3 }, { ...layout, genBits: 30 },
    { ...layout, genBits: 28 }, { ...layout, baseAddr: NaN },
  ]) assert.throws(() => binding.installFacadeSlotLayout(invalid), /layout|布局|header|capacity|genBits/);
  binding.installFacadeSlotLayout(layout);
  const before = binding.getArenaView();
  assert.equal(before.capacity, 2);
  assert.equal(before.genBits, 29);
  assert.equal(before.words.byteOffset, layout.baseAddr);
  assert.equal(before.words.length, HEADER_WORDS + 2 * (SLOT_STRIDE + JOIN_STRIDE));
  assert.throws(() => binding.installFacadeSlotLayout(layout), /重复/);
  memory.grow(1);
  assert.notEqual(binding.getArenaView().words.buffer, before.words.buffer);
  assert.equal(binding.getArenaView().words.buffer, memory.buffer);
});

it("H5 同步复制 fatal UTF-8 名称，保留消息 slot/gen 且不猜当前槽", { timeout: 10_000 }, () => {
  const { binding, memory, layout, messages } = fixture();
  binding.installFacadeSlotLayout(layout);
  memory.grow(1);
  const name = "\ufeff交易部中证";
  const bytes = new TextEncoder().encode(name);
  new Uint8Array(memory.buffer, 65536, bytes.length).set(bytes);
  binding.facade_spawn_notify(1, 7, 65536, bytes.length);
  new Uint8Array(memory.buffer, 65536, bytes.length).fill(0);
  assert.deepEqual(messages, [{ type: "facadeSpawn", slot: 1, gen: 7, name }]);
  new Uint8Array(memory.buffer, 65536, 2).set([0xc3, 0x28]);
  assert.throws(() => binding.facade_spawn_notify(1, 7, 65536, 2), /UTF-8|name/);
  for (const args of [[2, 7, 0, 0], [1, 2 ** 29, 0, 0], [1, 7, 131072, 1], [1, 7, 0, -1]]) {
    assert.throws(() => binding.facade_spawn_notify(...args as [number, number, number, number]), /slot|gen|name/);
  }
  assert.equal(messages.length, 1);
});

it("exit 仅记录不可变身份匹配意图，重复/缺失/角色错误均拒绝", { timeout: 10_000 }, () => {
  const { binding, layout, messages, bootstrap } = fixture("child");
  binding.installFacadeSlotLayout(layout);
  assert.throws(() => binding.requireExitIntent(), /缺失|未记录/);
  assert.throws(() => binding.facade_worker_exit_notify(2, 1, 7), /身份|bootstrap/);
  assert.throws(() => binding.facade_worker_exit_notify(-2147483647, 0, 7), /身份|bootstrap/);
  assert.throws(() => binding.facade_worker_exit_notify(-2147483647, 1, 8), /身份|bootstrap/);
  bootstrap!.gen = 9;
  binding.facade_worker_exit_notify(-2147483647, 1, 7);
  assert.deepEqual(binding.requireExitIntent(), { workerId: 0x80000001, slot: 1, gen: 7 });
  assert.equal(Object.isFrozen(binding.requireExitIntent()), true);
  assert.deepEqual(messages, []);
  assert.throws(() => binding.facade_worker_exit_notify(-2147483647, 1, 7), /重复/);
  assert.throws(() => fixture().binding.facade_worker_exit_notify(1, 0, 0), /child|角色/);
});

it("terminate 只向主线程发控制且 postControl 原始错误不丢失", { timeout: 10_000 }, () => {
  const { binding, messages } = fixture();
  binding.facade_terminate_child(-2147483647);
  assert.deepEqual(messages, [{ type: "facadeTerminateChild", workerId: 0x80000001 }]);
  const failure = new Error("clone 原因");
  const failed = createFacadeThreadBinding({ isWorker: () => true });
  failed.bindFacadeThreadEnvironment({ memory: new WebAssembly.Memory({ initial: 1, shared: true, maximum: 1 }),
    role: "engine", postControl() { throw failure; } });
  assert.throws(() => failed.facade_terminate_child(1), (error) => error === failure);
});


it("notify 唤醒同一共享字的全部等待者", { timeout: 10_000 }, async () => {
  const { binding, memory } = fixture();
  const words = new Int32Array(memory.buffer);
  const waitAsync = (Atomics as unknown as {
    waitAsync(array: Int32Array, index: number, expected: number, timeout: number):
      { async: boolean; value: string | Promise<string> };
  }).waitAsync;
  const first = waitAsync(words, 0, 0, 500);
  const second = waitAsync(words, 0, 0, 500);
  assert.equal(first.async, true);
  assert.equal(second.async, true);
  binding.facade_futex_notify(0);
  assert.deepEqual(await Promise.all([first.value, second.value]), ["ok", "ok"]);
});

it("绑定拒绝未知角色和无效 child 身份，layout 使用装入时的值快照", { timeout: 10_000 }, () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true });
  for (const invalid of [
    { role: "main" }, { role: "child" },
    { role: "child", bootstrap: { workerId: NaN, slot: 0, gen: 0 } },
    { role: "child", bootstrap: { workerId: 1, slot: -1, gen: 0 } },
    { role: "engine", bootstrap: { workerId: 1, slot: 0, gen: 0 } },
  ]) {
    const binding = createFacadeThreadBinding({ isWorker: () => true });
    assert.throws(() => binding.bindFacadeThreadEnvironment({ memory, postControl() {}, ...invalid } as
      Parameters<typeof binding.bindFacadeThreadEnvironment>[0]), /角色|bootstrap|workerId|slot/);
  }
  const { binding, layout } = fixture();
  binding.installFacadeSlotLayout(layout);
  layout.baseAddr = 1024;
  layout.capacity = 100;
  assert.equal(binding.getArenaView().words.byteOffset, 64);
  assert.equal(binding.getArenaView().capacity, 2);
});


it("wait 把真实共享字唤醒映射为 0", { timeout: 10_000 }, async () => {
  const { binding, memory } = fixture();
  const worker = new Worker(`
    const { parentPort, workerData } = require("node:worker_threads");
    const words = new Int32Array(workerData);
    parentPort.postMessage("ready");
    const end = Date.now() + 1000;
    while (Atomics.notify(words, 0) === 0 && Date.now() < end) {
      Atomics.wait(words, 1, 0, 1);
    }
  `, { eval: true, workerData: memory.buffer });
  try {
    await once(worker, "message");
    assert.equal(binding.facade_futex_wait(0, 0, 500n), 0);
  } finally {
    await worker.terminate();
  }
});
