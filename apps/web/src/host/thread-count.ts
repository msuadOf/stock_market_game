/** Pool size is an execution setting; it never limits the number of market tasks. */
export function resolveThreadCount(reported: unknown, requested?: unknown): number {
  const selected = requested === undefined ? reported : requested;
  if (!Number.isSafeInteger(selected) || Number(selected) < 1 || Number(selected) > 0xffff_ffff) {
    const source = requested === undefined ? "navigator.hardwareConcurrency" : "配置的 WASM 线程数";
    throw new Error(`${source} 必须是正安全整数且不超过 WASM u32 上限，无法初始化 WASM 多线程池`);
  }
  return Number(selected);
}
