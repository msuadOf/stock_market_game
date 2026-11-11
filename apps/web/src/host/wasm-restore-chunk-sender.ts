/** 单次 wasm-bindgen 参数复制的最大字节数。 */
export const RESTORE_CHUNK_MAX_BYTES = 64 * 1024;
/** 为参数复制以外的分配与对齐保留空间；不是完整恢复峰值保证。 */
export const RESTORE_WASM_HEADROOM_BYTES = 1024 * 1024;

export interface RestoreChunkGlue {
  facade_restore_chunks_begin(totalBytes: number): number;
  facade_restore_chunks_push(token: number, chunk: Uint8Array): void;
  facade_restore_chunks_finish(token: number): void;
  facade_restore_chunks_clear(token: number): boolean;
}

/**
 * 调用方提供已验证的字节总数与当前 WASM 可用预算估计；预算只约束输入传输，
 * 不能证明 JSON 解码或完整恢复的峰值可容纳。源必须在取消时及时结束或抛错。
 */
export async function sendRestoreChunks(
  glue: RestoreChunkGlue,
  source: AsyncIterable<Uint8Array>,
  totalBytes: number,
  availableWasmBytes: number,
  signal?: AbortSignal,
): Promise<number> {
  let token: number | undefined;
  try {
    if (!Number.isSafeInteger(totalBytes) || totalBytes < 0 || totalBytes > 0xffff_ffff) throw new Error("恢复档声明字节数无效");
    if (!Number.isSafeInteger(availableWasmBytes) || availableWasmBytes < 0 || totalBytes + RESTORE_CHUNK_MAX_BYTES + RESTORE_WASM_HEADROOM_BYTES > availableWasmBytes) throw new Error("WASM 输入缓冲预算不足；无法接收恢复档");
    signal?.throwIfAborted();
    token = glue.facade_restore_chunks_begin(totalBytes);
    const decoder = new TextDecoder("utf-8", { fatal: true });
    let received = 0;
    for await (const chunk of source) {
      signal?.throwIfAborted();
      if (!(chunk instanceof Uint8Array)) throw new Error("恢复档分块必须是 Uint8Array");
      if (chunk.byteLength > RESTORE_CHUNK_MAX_BYTES) throw new Error("恢复档分块超过单次传输上限");
      if (chunk.byteLength > totalBytes - received) throw new Error("恢复档分块超过声明总长");
      decoder.decode(chunk, { stream: true });
      glue.facade_restore_chunks_push(token, chunk);
      received += chunk.byteLength;
    }
    signal?.throwIfAborted();
    if (received !== totalBytes) throw new Error("恢复档分块未达到声明总长");
    decoder.decode();
    glue.facade_restore_chunks_finish(token);
    return token;
  } catch (error) {
    if (token !== undefined) {
      try { glue.facade_restore_chunks_clear(token); }
      catch (cleanupError) { throw new AggregateError([error, cleanupError], "恢复档传输失败且清理失败"); }
    }
    throw error;
  }
}
