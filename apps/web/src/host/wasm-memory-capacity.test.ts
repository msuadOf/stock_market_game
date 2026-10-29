import assert from "node:assert/strict";
import { test } from "node:test";
import {
  WASM_PRODUCT_MEMORY_CAPACITY,
  WASM_PRODUCT_MEMORY_CAPACITY_BYTES,
  WASM_PRODUCT_MEMORY_CAPACITY_PAGES,
  WASM_PRODUCT_MEMORY_INITIAL_BYTES,
  WASM_PRODUCT_MEMORY_INITIAL_PAGES,
} from "./wasm-memory-capacity.ts";

test("产品共享 memory 容量明确表示为可信构造上限", () => {
  assert.equal(WASM_PRODUCT_MEMORY_CAPACITY_BYTES, 1024 * 1024 * 1024);
  assert.equal(WASM_PRODUCT_MEMORY_CAPACITY_PAGES, 16384);
  assert.equal(WASM_PRODUCT_MEMORY_INITIAL_BYTES, 64 * 1024 * 1024);
  assert.equal(WASM_PRODUCT_MEMORY_INITIAL_PAGES, 1024);
  assert.deepEqual(WASM_PRODUCT_MEMORY_CAPACITY, {
    kind: "enforced-capacity",
    initialPages: 1024,
    maximumPages: 16384,
    liveUpperBoundBytes: 1024 * 1024 * 1024,
  });
  assert.equal(Object.isFrozen(WASM_PRODUCT_MEMORY_CAPACITY), true);
  assert.equal(WASM_PRODUCT_MEMORY_CAPACITY.liveUpperBoundBytes, WASM_PRODUCT_MEMORY_CAPACITY_BYTES);
});

test("产品单 Memory 上界独立固定且满足总账扣除R与双实例约束", { timeout: 10000 }, async () => {
  const { WASM_MEMORY_MAXIMUM_BYTES, WASM_MEMORY_RECLAIM_BYTES } = await import("./wasm-memory-budget.ts");
  assert.equal(WASM_MEMORY_MAXIMUM_BYTES, 3072 * 1024 * 1024);
  assert.equal(WASM_PRODUCT_MEMORY_CAPACITY_BYTES, 1024 * 1024 * 1024);
  assert.ok(WASM_PRODUCT_MEMORY_CAPACITY_BYTES <= WASM_MEMORY_MAXIMUM_BYTES - WASM_MEMORY_RECLAIM_BYTES);
  assert.ok(2 * WASM_PRODUCT_MEMORY_CAPACITY_BYTES <= WASM_MEMORY_MAXIMUM_BYTES);
});
