/** Rayon needs a pool capacity; it never controls business task count or order. */
export function resolveThreadCount(reported: unknown): number {
  if (!Number.isSafeInteger(reported) || Number(reported) < 1 || Number(reported) > 0xffff_ffff) {
    throw new Error("navigator.hardwareConcurrency 必须是正安全整数且不超过 WASM u32 上限，无法初始化 WASM 多线程池");
  }
  return Number(reported);
}
