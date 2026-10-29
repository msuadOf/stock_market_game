const WASM_PAGE_BYTES = 64 * 1024;
const productCapacityBytes = 1024 * 1024 * 1024;

export const WASM_PRODUCT_MEMORY_INITIAL_BYTES = 64 * 1024 * 1024;
export const WASM_PRODUCT_MEMORY_CAPACITY_BYTES = productCapacityBytes;
export const WASM_PRODUCT_MEMORY_INITIAL_PAGES = WASM_PRODUCT_MEMORY_INITIAL_BYTES / WASM_PAGE_BYTES;
export const WASM_PRODUCT_MEMORY_CAPACITY_PAGES = WASM_PRODUCT_MEMORY_CAPACITY_BYTES / WASM_PAGE_BYTES;

export interface WasmEnforcedMemoryCapacity {
  readonly kind: "enforced-capacity";
  readonly initialPages: number;
  readonly maximumPages: number;
  readonly liveUpperBoundBytes: number;
}

/** 可信的产品构造上限；不代表运行时观测峰值、交易配额或恢复 ABI 能力。 */
export const WASM_PRODUCT_MEMORY_CAPACITY: WasmEnforcedMemoryCapacity = Object.freeze({
  kind: "enforced-capacity",
  initialPages: WASM_PRODUCT_MEMORY_INITIAL_PAGES,
  maximumPages: WASM_PRODUCT_MEMORY_CAPACITY_PAGES,
  liveUpperBoundBytes: WASM_PRODUCT_MEMORY_CAPACITY_BYTES,
});
