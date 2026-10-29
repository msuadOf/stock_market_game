import assert from "node:assert/strict";
import test from "node:test";
import { abortWasmWait, parseWasmWaitAbortLayout } from "./wasm-wait-abort.ts";

test("fatal 只写同 Memory 的一字节，不覆盖左右哨兵且 grow 后使用当前 buffer", { timeout: 10000 }, () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  const bytes = new Uint8Array(memory.buffer);
  bytes.set([71, 0, 93], 7);
  const layout = parseWasmWaitAbortLayout({ address: 8, bytes: 1 }, memory);
  memory.grow(1);
  abortWasmWait(memory, layout);
  assert.deepEqual([...new Uint8Array(memory.buffer, 7, 3)], [71, 1, 93], "ABORT_ONE_BYTE_ONLY");
  abortWasmWait(memory, layout);
  assert.equal(new Uint8Array(memory.buffer)[8], 1);
});

test("布局一次快照，禁止四字节、越界、非整数和已置位实例", { timeout: 10000 }, () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true });
  let reads = 0;
  const layout = parseWasmWaitAbortLayout({ get address() { reads++; return 12; }, bytes: 1 }, memory);
  assert.equal(reads, 1, "ABORT_LAYOUT_READ_ONCE");
  assert.deepEqual(layout, { address: 12, bytes: 1 });
  for (const value of [{ address: 12, bytes: 4 }, { address: 65536, bytes: 1 }, { address: 1.2, bytes: 1 }, { address: -1, bytes: 1 }, { address: 0, bytes: 1, extra: true }]) {
    assert.throws(() => parseWasmWaitAbortLayout(value, memory));
  }
  Atomics.store(new Uint8Array(memory.buffer), 12, 1);
  assert.throws(() => parseWasmWaitAbortLayout({ address: 12, bytes: 1 }, memory), /abort|终止/);
});
