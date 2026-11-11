import assert from "node:assert/strict";
import test from "node:test";
import { parseLibosHostFaultLayout, abortLibosHostFault, activateLibosHostFault, overlapsLibosHostFault } from "./libos-host-fault.ts";

const protectedRanges = { arena: { start: 100, length: 16 }, engine: { start: 128, length: 8 }, ingress: { start: 144, length: 8 }, death: { start: 160, length: 4 }, abort: { start: 168, length: 1 } };
const valid = { globalStateAddress: 8, domainAbortAddress: 16, hostReadyAddress: 40, slots: [{ abortAddress: 24, waitAddress: 28 }, { abortAddress: 32, waitAddress: 36 }] };

test("宿主故障发布状态并唤醒全部登记等待字", { timeout: 10000 }, async () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  Atomics.store(new Int32Array(memory.buffer, 8, 1), 0, 1);
  const layout = parseLibosHostFaultLayout(valid, memory, { capacity: 2, protectedRanges });
  const waits = [28, 36].map(address => {
    const words = new Int32Array(memory.buffer, address, 1);
    Atomics.store(words, 0, -1);
    const waitAsync = (Atomics as unknown as { waitAsync: (array: Int32Array, index: number, expected: number, timeout: number) => { async: boolean; value: Promise<string> } }).waitAsync;
    const pending = waitAsync(words, 0, -1, 1000);
    assert.equal(pending.async, true);
    return pending.value;
  });
  memory.grow(1);
  abortLibosHostFault(memory, layout);
  assert.deepEqual(await Promise.all(waits), ["ok", "ok"]);
  assert.equal(Atomics.load(new Int32Array(memory.buffer, 8, 1), 0), 3);
  for (const address of [16, 24, 32]) assert.equal(Atomics.load(new Uint8Array(memory.buffer, address, 1), 0), 1);
  for (const address of [28, 36]) assert.equal(Atomics.load(new Int32Array(memory.buffer, address, 1), 0), 1);
});

test("布局非法时拒绝且不修改共享状态", { timeout: 10000 }, () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true });
  Atomics.store(new Int32Array(memory.buffer, 8, 1), 0, 1);
  for (const layout of [
    { ...valid, globalStateAddress: 9 },
    { ...valid, domainAbortAddress: 28 },
    { ...valid, slots: [{ abortAddress: 24, waitAddress: 28 }, { abortAddress: 32, waitAddress: 65536 }] },
    { ...valid, slots: [{ abortAddress: 24, waitAddress: 28 }] },
    { ...valid, slots: [{ abortAddress: 24, waitAddress: 28 }, { abortAddress: 24, waitAddress: 36 }] },
    { ...valid, extra: 1 },
  ]) assert.throws(() => parseLibosHostFaultLayout(layout, memory, { capacity: 2, protectedRanges }));
  assert.equal(Atomics.load(new Int32Array(memory.buffer, 8, 1), 0), 1);
  assert.equal(Atomics.load(new Uint8Array(memory.buffer, 16, 1), 0), 0);
});


test("稀疏槽与跨 memory 误用必须拒绝", { timeout: 10000 }, () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true });
  const other = new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true });
  Atomics.store(new Int32Array(memory.buffer, 8, 1), 0, 1);
  const sparse = Array(1);
  assert.throws(() => parseLibosHostFaultLayout({ ...valid, slots: sparse }, memory, { capacity: 1, protectedRanges }));
  const layout = parseLibosHostFaultLayout(valid, memory, { capacity: 2, protectedRanges });
  assert.throws(() => abortLibosHostFault(other, layout));
  assert.equal(Atomics.load(new Int32Array(other.buffer, 8, 1), 0), 0);
});

test("不得与 arena 或 tombstone 等现有共享区域重叠", { timeout: 10000 }, () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true });
  Atomics.store(new Int32Array(memory.buffer, 8, 1), 0, 1);
  assert.throws(() => parseLibosHostFaultLayout(valid, memory, { capacity: 2, protectedRanges: { ...protectedRanges, arena: { start: 28, length: 4 } } }));
  assert.equal(Atomics.load(new Int32Array(memory.buffer, 8, 1), 0), 1);
});

test("缺少必需的宿主保护区域时拒绝", { timeout: 10000 }, () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true });
  Atomics.store(new Int32Array(memory.buffer, 8, 1), 0, 1);
  assert.throws(() => parseLibosHostFaultLayout(valid, memory, { capacity: 2, protectedRanges: {} as typeof protectedRanges }));
});


test("动态 tombstone 不得与已登记故障字重叠", { timeout: 10000 }, () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true });
  Atomics.store(new Int32Array(memory.buffer, 8, 1), 0, 1);
  const layout = parseLibosHostFaultLayout(valid, memory, { capacity: 2, protectedRanges });
  assert.equal(overlapsLibosHostFault(layout, 26, 4), true);
  assert.equal(overlapsLibosHostFault(layout, 44, 4), false);
});


test("supervisor 接受布局后显式发布 host-ready", { timeout: 10000 }, () => {
  const memory = new WebAssembly.Memory({ initial:1, maximum:1, shared:true });
  Atomics.store(new Int32Array(memory.buffer, 8, 1), 0, 1);
  const layout = parseLibosHostFaultLayout(valid, memory, { capacity:2, protectedRanges });
  activateLibosHostFault(memory, layout);
  assert.equal(Atomics.load(new Int32Array(memory.buffer, 40, 1), 0), 1);
});

test("host-ready 不得重复发布或在故障后发布", { timeout: 10000 }, () => {
  const memory = new WebAssembly.Memory({ initial:1, maximum:1, shared:true });
  Atomics.store(new Int32Array(memory.buffer, 8, 1), 0, 1);
  const layout = parseLibosHostFaultLayout(valid, memory, {capacity:2, protectedRanges});
  activateLibosHostFault(memory, layout);
  assert.throws(() => activateLibosHostFault(memory, layout), /已发布/);
  const second = new WebAssembly.Memory({ initial:1, maximum:1, shared:true });
  Atomics.store(new Int32Array(second.buffer, 8, 1), 0, 1);
  const other = parseLibosHostFaultLayout(valid, second, {capacity:2, protectedRanges});
  abortLibosHostFault(second, other);
  assert.throws(() => activateLibosHostFault(second, other), /故障/);
});
