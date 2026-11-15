import { createFacadeSharedMemory as createLibosSharedMemory } from "../../../../packages/libos/browser/facade-wasm-module.ts";
import { WASM_PRODUCT_MEMORY_CAPACITY_PAGES, WASM_PRODUCT_MEMORY_INITIAL_PAGES } from "./wasm-memory-capacity.ts";

export { instantiateFacadeModule, type FacadeRawExports, type FacadeSyncInitializer } from "../../../../packages/libos/browser/facade-wasm-module.ts";

/** 产品决定初始值和上限；LibOS 不包含 Memory 配额。 */
export function createFacadeSharedMemory(options: { readonly isolated: boolean }): WebAssembly.Memory {
  return createLibosSharedMemory({
    isolated: options.isolated,
    initialPages: WASM_PRODUCT_MEMORY_INITIAL_PAGES,
    maximumPages: WASM_PRODUCT_MEMORY_CAPACITY_PAGES,
  });
}
