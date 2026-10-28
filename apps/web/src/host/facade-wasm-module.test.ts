import assert from "node:assert/strict";
import { test } from "node:test";
import { createFacadeSharedMemory, instantiateFacadeModule } from "./facade-wasm-module.ts";

const module = new WebAssembly.Module(new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0]));

test("memory 使用 64MiB 初始档与 2GiB 上限", () => {
  const memory = createFacadeSharedMemory({ isolated: true });
  assert.ok((memory.buffer as ArrayBufferLike) instanceof SharedArrayBuffer);
  assert.equal(memory.buffer.byteLength, 64 * 1024 * 1024);
  assert.equal(memory.grow(32768 - 1024), 1024);
  assert.equal(memory.buffer.byteLength, 2 * 1024 * 1024 * 1024);
  assert.throws(() => memory.grow(1), RangeError);
});

test("非隔离 substrate 必须显式拒绝", () => {
  assert.throws(() => createFacadeSharedMemory({ isolated: false }), /隔离/);
});

test("initSync 收到准确 module/memory 且 raw memory 必须同一引用", () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  const raw = { memory, __wbindgen_thread_destroy() {} };
  assert.equal(instantiateFacadeModule({ initSync(input) { assert.deepEqual(input, { module, memory }); return raw; } }, module, memory), raw);
  assert.throws(() => instantiateFacadeModule({ initSync() { return { ...raw, memory: new WebAssembly.Memory({ initial: 1 }) }; } }, module, memory), /memory/);
});

test("实例化拒绝非共享 memory 与缺失 destroy", () => {
  const memory = new WebAssembly.Memory({ initial: 1 });
  assert.throws(() => instantiateFacadeModule({ initSync() { throw new Error("不应调用"); } }, module, memory), /共享/);
  const shared = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  assert.throws(() => instantiateFacadeModule({ initSync() { return { memory: shared } as never; } }, module, shared), /destroy/);
});
