import {
  WASM_PRODUCT_MEMORY_CAPACITY_PAGES,
  WASM_PRODUCT_MEMORY_INITIAL_PAGES,
} from "./wasm-memory-capacity.ts";

/** 所有角色复用同一 module/memory；本 helper 不创建 WASM 实例以外的 Worker。 */
export interface FacadeRawExports {
  readonly memory: WebAssembly.Memory;
  readonly __wbindgen_thread_destroy: (tls: number, stack: number, stackSize: number) => void;
}

export interface FacadeSyncInitializer {
  readonly initSync: (options: { module: WebAssembly.Module; memory: WebAssembly.Memory }) => FacadeRawExports;
}

export function createFacadeSharedMemory(options: { readonly isolated: boolean }): WebAssembly.Memory {
  if (options.isolated !== true || typeof SharedArrayBuffer !== "function") {
    throw new Error("facade WASM 需要 crossOriginIsolated 隔离环境及 SharedArrayBuffer；请检查 COOP/COEP 与安全上下文");
  }
  return new WebAssembly.Memory({
    initial: WASM_PRODUCT_MEMORY_INITIAL_PAGES,
    maximum: WASM_PRODUCT_MEMORY_CAPACITY_PAGES,
    shared: true,
  });
}

export function instantiateFacadeModule(glue: FacadeSyncInitializer, module: WebAssembly.Module, memory: WebAssembly.Memory): FacadeRawExports {
  if (!(module instanceof WebAssembly.Module)) throw new Error("facade module 必须为已编译 WebAssembly.Module");
  if (!(memory instanceof WebAssembly.Memory) || !(memory.buffer instanceof SharedArrayBuffer)) throw new Error("facade memory 必须为共享 WebAssembly.Memory");
  const raw = glue.initSync({ module, memory });
  if (raw.memory !== memory) throw new Error("facade raw memory 与传入 memory 不一致，禁止创建第二份内存");
  if (typeof raw.__wbindgen_thread_destroy !== "function") throw new Error("facade raw exports 缺少 __wbindgen_thread_destroy");
  return raw;
}
