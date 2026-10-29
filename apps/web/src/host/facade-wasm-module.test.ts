import assert from "node:assert/strict";
import { test } from "node:test";
import { createFacadeSharedMemory, instantiateFacadeModule } from "./facade-wasm-module.ts";
import { WASM_PRODUCT_MEMORY_CAPACITY_BYTES, WASM_PRODUCT_MEMORY_CAPACITY_PAGES, WASM_PRODUCT_MEMORY_INITIAL_BYTES, WASM_PRODUCT_MEMORY_INITIAL_PAGES } from "./wasm-memory-capacity.ts";

const module = new WebAssembly.Module(new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0]));

test("产品共享 memory 构造使用同一容量契约的初始页数与上限页数", () => {
  const memoryDescriptor = Object.getOwnPropertyDescriptor(WebAssembly, "Memory");
  assert.ok(memoryDescriptor?.configurable);
  const calls: WebAssembly.MemoryDescriptor[] = [];
  const NativeMemory = WebAssembly.Memory;
  Object.defineProperty(WebAssembly, "Memory", {
    configurable: true,
    value: class extends NativeMemory {
      constructor(descriptor: WebAssembly.MemoryDescriptor) {
        calls.push(descriptor);
        super({ initial: 1, maximum: 2, shared: descriptor.shared });
      }
    },
  });
  let memory: WebAssembly.Memory;
  try {
    memory = createFacadeSharedMemory({ isolated: true });
  } finally {
    Object.defineProperty(WebAssembly, "Memory", memoryDescriptor);
  }
  assert.deepEqual(calls, [{ initial: WASM_PRODUCT_MEMORY_INITIAL_PAGES, maximum: WASM_PRODUCT_MEMORY_CAPACITY_PAGES, shared: true }]);
  assert.equal(WASM_PRODUCT_MEMORY_INITIAL_BYTES, WASM_PRODUCT_MEMORY_INITIAL_PAGES * 64 * 1024);
  assert.equal(WASM_PRODUCT_MEMORY_CAPACITY_BYTES, WASM_PRODUCT_MEMORY_CAPACITY_PAGES * 64 * 1024);
  assert.ok((memory.buffer as ArrayBufferLike) instanceof SharedArrayBuffer);
  assert.equal(memory.buffer.byteLength, 64 * 1024);
  assert.equal(memory.grow(1), 1);
  assert.throws(() => memory.grow(1), RangeError);
});

function moduleImportingSharedMemory(maximumPages: number): WebAssembly.Module {
  const maxBytes = maximumPages === 32768 ? [128, 128, 2] : [maximumPages];
  const body = [1, 3, 101, 110, 118, 6, 109, 101, 109, 111, 114, 121, 2, 3, 1, ...maxBytes];
  return new WebAssembly.Module(new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0, 2, body.length, ...body]));
}

test("1GiB 产品 shared memory 可链接到2GiB import，上界不兼容时显式 LinkError", () => {
  const productMemory = new WebAssembly.Memory({ initial: 1, maximum: WASM_PRODUCT_MEMORY_CAPACITY_PAGES, shared: true });
  const productModule = moduleImportingSharedMemory(32768);
  assert.doesNotThrow(() => new WebAssembly.Instance(productModule, { env: { memory: productMemory } }));

  const smallerImportModule = moduleImportingSharedMemory(2);
  const largerActualMemory = new WebAssembly.Memory({ initial: 1, maximum: 3, shared: true });
  assert.throws(() => new WebAssembly.Instance(smallerImportModule, { env: { memory: largerActualMemory } }), WebAssembly.LinkError);
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
