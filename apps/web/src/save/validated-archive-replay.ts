import { readCapturedArchiveChunks } from "./archive-source.ts"
import { createSha256Chain } from "./sha256-chain.ts"
import { inspectValidatedArchiveLease, type ValidatedArchiveLease } from "./validated-archive-lease.ts"

/**
 * 同源第三遍按 UTF-8 输出；仅在消费到 EOF 且长度和摘要匹配后正常完成。
 * 字节由消费者持有，跨线程传递后仍须在接收端验证，不能把本地摘要视为对端证明。
 */
export function replayValidatedArchive(lease: ValidatedArchiveLease, signal?: AbortSignal): AsyncIterable<Uint8Array> {
  const details = inspectValidatedArchiveLease(lease)
  let used = false
  return {
    async *[Symbol.asyncIterator]() {
      if (used) throw new Error("已验证存档重放只能消费一次")
      used = true
      signal?.throwIfAborted()
      const chain = await createSha256Chain()
      const encoder = new TextEncoder()
      let pendingHigh = ""
      let total = 0
      async function encode(part: string): Promise<Uint8Array> {
        const bytes = encoder.encode(part)
        if (bytes.byteLength > 65536 || bytes.byteLength > details.utf8Bytes - total) throw new Error("存档重放长度超过已验证上限")
        await chain.update(bytes)
        total += bytes.byteLength
        signal?.throwIfAborted()
        return bytes
      }
      for await (const text of readCapturedArchiveChunks(details.source, undefined, signal)) {
        signal?.throwIfAborted()
        for (let offset = 0; offset < text.length; offset += 16384) {
          let part = pendingHigh + text.slice(offset, offset + 16384)
          pendingHigh = ""
          const last = part.charCodeAt(part.length - 1)
          if (last >= 0xd800 && last <= 0xdbff) { pendingHigh = part.slice(-1); part = part.slice(0, -1) }
          if (part.length > 0) yield await encode(part)
        }
      }
      if (pendingHigh !== "") yield await encode(pendingHigh)
      signal?.throwIfAborted()
      const actual = await chain.finish()
      signal?.throwIfAborted()
      const expected = details.contentDigest
      if (total !== details.utf8Bytes || actual.algorithm !== expected.algorithm || actual.blockBytes !== expected.blockBytes || actual.byteLength !== expected.byteLength || actual.blocks !== expected.blocks || actual.digest !== expected.digest) throw new Error("存档重放长度或摘要与已验证源不一致")
    },
  }
}
