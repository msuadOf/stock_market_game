/** Rust GLOBAL_ABORT 的同 Memory 单字节布局；与 tick Abort 分开。 */
export interface WasmWaitAbortLayout { readonly address: number; readonly bytes: 1 }

export function parseWasmWaitAbortLayout(value: unknown, memory: WebAssembly.Memory): WasmWaitAbortLayout {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("wasm abort 布局必须为对象");
  const source = value as Record<string, unknown>;
  if (Object.keys(source).length !== 2 || !Object.hasOwn(source, "address") || !Object.hasOwn(source, "bytes")) throw new Error("wasm abort 布局只允许 address/bytes");
  const address = source.address, bytes = source.bytes;
  if (typeof address !== "number" || !Number.isInteger(address) || address < 0 || address > 0xffff_ffff || bytes !== 1) throw new Error("wasm abort 必须为 u32 地址的一字节布局");
  if (typeof SharedArrayBuffer === "undefined" || !(memory instanceof WebAssembly.Memory)) throw new Error("wasm abort 需要 shared Memory");
  const buffer: unknown = memory.buffer;
  if (!(buffer instanceof SharedArrayBuffer) || address + 1 > buffer.byteLength) throw new Error("wasm abort 布局超出当前 shared Memory");
  if (Atomics.load(new Uint8Array(buffer, address, 1), 0) !== 0) throw new Error("wasm abort 已置位，实例已永久终止，禁止复位");
  return Object.freeze({ address, bytes });
}

export function abortWasmWait(memory: WebAssembly.Memory, layout: WasmWaitAbortLayout): void {
  // 每次取当前 buffer；不能保留 grow 前视图，更不能以 Int32 覆盖邻字节。
  Atomics.store(new Uint8Array(memory.buffer, layout.address, 1), 0, 1);
}
