import { gzipSaveCodec } from "./save-repository.ts"
import { scanArchiveRoot, type ArchiveRootScan } from "./schema/archive-root-scan.ts"

export type CapturedArchiveSource = Readonly<{ kind: "captured-gzip-source"; encodedCharacters: number; schemaValidated: false }>
export interface ArchiveChunkDecoder { decodeChunks(payload: string, signal?: AbortSignal): AsyncIterable<string> }
const payloads = new WeakMap<object, string>()
// 固定生产 codec 的函数身份，不能让外部对导出对象的修改替换验证数据。
const defaultDecodeChunks = gzipSaveCodec.decodeChunks

/** 捕获的是尚未完成 schema 校验的原始源，不能充当 Memory admission 凭证。 */
export function captureGzipArchiveSource(raw: string): CapturedArchiveSource {
  if (typeof raw !== "string") throw new Error("浏览器存档载荷必须为字符串")
  if (!raw.startsWith("gzip:")) throw new Error("浏览器存档格式无效：缺少 gzip: 前缀")
  const source = Object.freeze({ kind: "captured-gzip-source" as const, encodedCharacters: raw.length - 5, schemaValidated: false as const })
  payloads.set(source, raw.slice(5))
  return source
}

/** 仅返回本模块捕获的压缩源，用于固定已验证的重载目标。 */
export function capturedGzipPayload(source: CapturedArchiveSource): string {
  const payload = payloads.get(source)
  if (payload === undefined) throw new Error("存档源不是本模块捕获的身份")
  return `gzip:${payload}`
}

export async function* readCapturedArchiveChunks(source: CapturedArchiveSource, decoder?: ArchiveChunkDecoder, signal?: AbortSignal): AsyncIterable<string> {
  const payload = payloads.get(source)
  if (payload === undefined) throw new Error("存档源不是本模块捕获的身份")
  const selected = decoder === undefined ? gzipSaveCodec : decoder
  const decode = decoder === undefined ? defaultDecodeChunks : selected.decodeChunks
  if (typeof decode !== "function") throw new Error("存档 codec 缺少分块解码能力")
  try { yield* decode.call(selected, payload, signal) }
  catch (error) {
    if (signal?.aborted && error === signal.reason) throw error
    const message = `解压浏览器存档失败：${error instanceof Error ? error.message : String(error)}`
    if (error instanceof AggregateError) throw new AggregateError(error.errors, message, { cause: error })
    throw new Error(message, { cause: error })
  }
}

export async function scanCapturedArchive(source: CapturedArchiveSource, decoder?: ArchiveChunkDecoder): Promise<ArchiveRootScan> {
  return scanArchiveRoot(readCapturedArchiveChunks(source, decoder))
}
