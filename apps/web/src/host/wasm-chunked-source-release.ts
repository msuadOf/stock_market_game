import type { WasmFinalRelease } from "./wasm-document-memory-owner.ts";

export type ChunkedSourceOutcome = { ok: true } | { ok: false; error: unknown };

/** 只有源清理已结束且无清理失败，才可交付控制端真实 release。 */
export async function confirmChunkedSourceRelease(release: WasmFinalRelease, outcome: Promise<ChunkedSourceOutcome>): Promise<WasmFinalRelease> {
  const source = await outcome;
  if (!source.ok && source.error instanceof AggregateError) throw source.error;
  return release;
}
