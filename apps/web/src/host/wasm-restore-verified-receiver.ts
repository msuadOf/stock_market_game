import { createSha256Chain, SHA256_CHAIN_ALGORITHM, SHA256_CHAIN_BLOCK_BYTES, type Sha256ChainResult } from "../save/sha256-chain.ts";
import { receiveRestoreChunks } from "./restore-chunk-port.ts";
import { RESTORE_CHUNK_MAX_BYTES, RESTORE_WASM_HEADROOM_BYTES, type RestoreChunkGlue } from "./wasm-restore-chunk-sender.ts";

/**
 * 接收端对实际收到的字节重新计算摘要；只有核对成功才 finish。
 * expected 必须由调用方绑定已验证租约，此助手本身不签发 admission。
 */
export async function receiveVerifiedRestore(port: MessagePort, generation: number, expected: Sha256ChainResult, availableWasmBytes: number, glue: RestoreChunkGlue, signal?: AbortSignal): Promise<number> {
  let token: number | undefined;
  try {
    const claim = structuredClone(expected);
    if (claim === null || typeof claim !== "object" || Object.keys(claim).sort().join(",") !== "algorithm,blockBytes,blocks,byteLength,digest"
      || claim.algorithm !== SHA256_CHAIN_ALGORITHM || claim.blockBytes !== SHA256_CHAIN_BLOCK_BYTES
      || typeof claim.byteLength !== "bigint" || claim.byteLength < 0n || claim.byteLength > 0xffff_ffffn
      || typeof claim.blocks !== "bigint" || claim.blocks !== (claim.byteLength + BigInt(SHA256_CHAIN_BLOCK_BYTES - 1)) / BigInt(SHA256_CHAIN_BLOCK_BYTES)
      || typeof claim.digest !== "string" || !/^[a-f0-9]{64}$/.test(claim.digest)) throw new Error("恢复档摘要 descriptor 无效");
    const length = Number(claim.byteLength);
    if (!Number.isSafeInteger(availableWasmBytes) || availableWasmBytes < 0 || length + 2 * RESTORE_CHUNK_MAX_BYTES + RESTORE_WASM_HEADROOM_BYTES > availableWasmBytes) throw new Error("WASM 输入缓冲预算不足；无法接收恢复档");
    signal?.throwIfAborted();
    const hash = await createSha256Chain();
    signal?.throwIfAborted();
    token = glue.facade_restore_chunks_begin(length);
    const current = token;
    await receiveRestoreChunks(port, generation, length, async (bytes, isActive) => {
      signal?.throwIfAborted();
      if (!(bytes.buffer instanceof ArrayBuffer)) throw new Error("恢复档分块不允许共享内存");
      // 固定摘要与 WASM 输入的同一字节副本，防止异步摘要等待期间被外部修改。
      const stable = bytes.slice();
      await hash.update(stable);
      signal?.throwIfAborted();
      if (!isActive()) throw new Error("分块传输已结束，拒绝写入旧 token");
      glue.facade_restore_chunks_push(current, stable);
    }, signal, async (isActive) => {
      signal?.throwIfAborted();
      if (!isActive()) throw new Error("分块传输已结束，拒绝完成旧 token");
      const actual = await hash.finish();
      signal?.throwIfAborted();
      if (!isActive()) throw new Error("分块传输已结束，拒绝完成旧 token");
      if (actual.algorithm !== claim.algorithm || actual.blockBytes !== claim.blockBytes || actual.byteLength !== claim.byteLength || actual.blocks !== claim.blocks || actual.digest !== claim.digest) throw new Error("实际收到的恢复档摘要与验证源不一致");
      glue.facade_restore_chunks_finish(current);
    });
    return current;
  } catch (error) {
    const failures: unknown[] = [];
    try { port.close(); } catch (cleanupError) { failures.push(cleanupError); }
    if (token !== undefined) {
      try { glue.facade_restore_chunks_clear(token); } catch (cleanupError) { failures.push(cleanupError); }
    }
    if (failures.length > 0) throw new AggregateError([error, ...failures], "恢复档接收失败且清理失败");
    throw error;
  }
}
