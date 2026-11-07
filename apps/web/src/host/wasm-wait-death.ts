/** Rust 全局 DEAD 的共享四字节布局。 */
export interface WasmWaitDeathLayout { readonly address: number; readonly bytes: 4 }

export function parseWasmWaitDeathLayout(value: unknown, memory: WebAssembly.Memory): WasmWaitDeathLayout {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("wasm DEAD 布局必须为对象");
  const source = value as Record<string, unknown>;
  if (Object.keys(source).length !== 2 || !Object.hasOwn(source, "address") || !Object.hasOwn(source, "bytes")) throw new Error("wasm DEAD 布局只允许 address/bytes");
  const address = source.address, bytes = source.bytes;
  if (typeof address !== "number" || !Number.isInteger(address) || address <= 0 || address > 0xffff_ffff || address % 4 !== 0 || bytes !== 4) throw new Error("wasm DEAD 必须为对齐的四字节布局");
  if (!(memory instanceof WebAssembly.Memory) || typeof SharedArrayBuffer === "undefined") throw new Error("wasm DEAD 需要 shared Memory");
  const buffer: unknown = memory.buffer;
  if (!(buffer instanceof SharedArrayBuffer) || address + 4 > buffer.byteLength) throw new Error("wasm DEAD 布局超出 shared Memory");
  return Object.freeze({ address, bytes });
}

/** DEAD 先于诊断记录发布；回调不得假设记录已经完成。 */
export function watchWasmDeath(memory: WebAssembly.Memory, layout: WasmWaitDeathLayout, onDeath: () => void, onError: (error: Error) => void): () => void {
  let stopped = false;
  let cancelled = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const fail = (error: unknown) => {
    if (stopped) return;
    stopped = true;
    onError(error instanceof Error ? error : new Error(`wasm DEAD 监听失败：${String(error)}`));
  };
  const observe = () => {
    if (stopped) return;
    try {
      const words = new Int32Array(memory.buffer, layout.address, 1);
      const state = Atomics.load(words, 0);
      if (state === 1) { stopped = true; queueMicrotask(() => { if (!cancelled) onDeath(); }); return; }
      if (state !== 0) throw new Error(`wasm DEAD 状态非法：${state}`);
      const atomics = Atomics as typeof Atomics & { waitAsync?: (array: Int32Array, index: number, value: number, timeout: number) => { async: true; value: Promise<string> } | { async: false; value: string } };
      if (typeof atomics.waitAsync !== "function") {
        // 兼容缺少 waitAsync 的浏览器，最多每 100ms 检查一次。
        timer = setTimeout(observe, 100);
        return;
      }
      // 有界等待保证取消后旧 SharedArrayBuffer 不被 waiter 长期保留。
      const waiting = atomics.waitAsync(words, 0, 0, 100);
      if (waiting.async) void waiting.value.then(observe, fail);
      else queueMicrotask(observe);
    } catch (error) { fail(error); }
  };
  // 异步回调使调用者可以先保存取消函数，且覆盖订阅前已置位的竞争。
  queueMicrotask(observe);
  return () => { cancelled = true; stopped = true; if (timer !== undefined) clearTimeout(timer); };
}
