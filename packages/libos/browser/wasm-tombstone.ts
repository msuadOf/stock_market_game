export type WasmTombstone =
  | { readonly state: "absent" | "writing" }
  | { readonly state: "live"; readonly kind: "panic" | "hardtrap"; readonly phase: number; readonly message: string; readonly truncated: boolean }
  | { readonly state: "invalid-utf8"; readonly kind: "panic" | "hardtrap"; readonly phase: number; readonly bytes: readonly number[]; readonly truncated: boolean };

/** 只读取已发布记录；WRITING 立即返回，不能等待已经死亡的 Worker。 */
export function readWasmTombstone(memory: WebAssembly.Memory, address: number, workerId: number): WasmTombstone {
  const buffer: unknown = memory.buffer;
  if (!(buffer instanceof SharedArrayBuffer) || !Number.isSafeInteger(address) || address <= 0 || address % 4 !== 0 || address + 256 > buffer.byteLength) {
    throw new Error("WASM tombstone 地址未对齐或超出共享 memory");
  }
  const words = new Uint32Array(buffer, address, 4);
  const magic = Atomics.load(words, 0);
  if (magic === 0) return { state: "absent" };
  if (magic === 1) return { state: "writing" };
  if (magic !== 2) throw new Error(`WASM tombstone magic 非法：${magic}`);
  if (Atomics.load(words, 1) !== workerId) throw new Error("WASM tombstone worker 身份不匹配");
  const halves = new Uint16Array(buffer, address, 8);
  const bytes = new Uint8Array(buffer, address, 256);
  const phase = Atomics.load(halves, 4);
  const kind = Atomics.load(bytes, 10);
  if (kind !== 1 && kind !== 2) throw new Error(`WASM tombstone kind 非法：${kind}`);
  const truncated = Atomics.load(bytes, 11);
  const length = Atomics.load(halves, 6);
  if (truncated > 1 || length > 240 || (truncated === 1 && length !== 240) || Atomics.load(halves, 7) !== 0) throw new Error("WASM tombstone 头部非法");
  const payload = Uint8Array.from({ length }, (_, index) => Atomics.load(bytes, 16 + index));
  let message: string;
  try { message = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(payload, { stream: truncated === 1 }); }
  catch { return { state: "invalid-utf8", kind: kind === 1 ? "panic" : "hardtrap", phase, bytes: [...payload], truncated: truncated === 1 }; }
  return { state: "live", kind: kind === 1 ? "panic" : "hardtrap", phase, message, truncated: truncated === 1 };
}
