import type { ValidatedArchiveLease } from "./validated-archive-lease.ts";
import { parseSaveSlot } from "./save-schema.ts";
import type { StrictSaveEnvelope } from "./schema/root.ts";
import { parseFrozenSaveJson, parseFrozenSaveJsonChunks, parseSaveJson } from "./save-schema.ts";
import { readDecodedText } from "./archive-decoded-reader.ts";

interface KeyValueStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export interface AsyncSaveStorage {
  getItem(key: string): string | null | Promise<string | null>;
  setItem(key: string, value: string, isCurrent: () => boolean): boolean | Promise<boolean>;
}

export interface SaveCompressionCodec {
  encode(text: string): Promise<string>
  decode(text: string): Promise<string>
  decodeChunks?(text: string, signal?: AbortSignal): AsyncIterable<string>
}

function toBase64(bytes: Uint8Array): string {
  // 有界字符块避免为每个 gzip 字节保留一段字符串链，也不展开超长参数列表。
  const chunks: string[] = []
  for (let offset = 0; offset < bytes.length; offset += 16384) {
    chunks.push(String.fromCharCode(...bytes.subarray(offset, offset + 16384)))
  }
  return btoa(chunks.join(""))
}

function fromBase64(value: string): Uint8Array<ArrayBuffer> {
  const binary = atob(value)
  // 字符串 iterable 的 Uint8Array.from 会先汇集全部字符；直接填充唯一缓冲区。
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index++) bytes[index] = binary.charCodeAt(index)
  return bytes
}

export const gzipSaveCodec: SaveCompressionCodec = Object.freeze({
  async encode(text: string) {
    const input = new Blob([text]).stream().pipeThrough(new CompressionStream("gzip"))
    return toBase64(new Uint8Array(await new Response(input).arrayBuffer()))
  },
  async decode(text: string) {
    const bytes = fromBase64(text)
    const input = new Blob([bytes]).stream().pipeThrough(new DecompressionStream("gzip"))
    // 由流直接解码文本，不在 JS 中同时保留所有解压块及其完整合并副本。
    return new Response(input).text()
  },
  async *decodeChunks(text: string, signal?: AbortSignal) {
    signal?.throwIfAborted()
    const bytes = fromBase64(text)
    const input = new Blob([bytes]).stream()
      .pipeThrough(new DecompressionStream("gzip"))
      .pipeThrough(new TextDecoderStream())
    yield* readDecodedText(input.getReader(), signal)
  },
})

/** 存储读和模块装载不可撤销，但取消后仍观察其晚到结果，不启动后续验证。 */
async function awaitLeaseRead<Value>(pending: Promise<Value>, signal?: AbortSignal): Promise<Value> {
  if (signal === undefined) return pending
  let remove: (() => void) | undefined
  try {
    return await new Promise<Value>((resolve, reject) => {
      const abort = () => { reject(signal.reason) }
      remove = () => { signal.removeEventListener("abort", abort) }
      signal.addEventListener("abort", abort, { once: true })
      pending.then(resolve, reject)
      if (signal.aborted) abort()
    })
  } finally { remove?.() }
}

export class CompressedSaveRepository {
  private readonly storage: AsyncSaveStorage
  private readonly key: string
  private readonly codec: SaveCompressionCodec

  constructor(storage: AsyncSaveStorage, key = "stock-game-save", codec = gzipSaveCodec) {
    this.storage = storage
    this.key = key
    this.codec = codec
  }

  async save(slot: unknown, isCurrent: () => boolean = () => true): Promise<boolean> {
    try {
      const compressed = await this.codec.encode(JSON.stringify(parseSaveSlot(slot)))
      if (!isCurrent()) return false;
      return await this.storage.setItem(this.key, `gzip:${compressed}`, isCurrent)
    } catch (error) {
      throw new Error(`写入浏览器存档失败：${error instanceof Error ? error.message : String(error)}`)
    }
  }

  async load(): Promise<StrictSaveEnvelope | null> {
    return this.loadWith(parseSaveJson)
  }

  /** 本地 WASM 准入需要唯一可信的不可变快照。 */
  async loadFrozen(): Promise<StrictSaveEnvelope | null> {
    return this.loadWith(parseFrozenSaveJson)
  }

  /** 显式单次捕获入口；不被 load/loadFrozen 自动选择，也不是生产 Memory 准入。 */
  async loadValidatedLease(signal?: AbortSignal): Promise<ValidatedArchiveLease | null> {
    signal?.throwIfAborted()
    if (this.codec !== gzipSaveCodec) throw new Error("已验证存档租约只支持固定 gzipSaveCodec，不接受自定义 codec 或测试 mock")
    const readFailure = (error: unknown) => new Error(`读取浏览器存档失败：${error instanceof Error ? error.message : String(error)}`, { cause: error })
    let pendingRead: Promise<string | null>
    try { pendingRead = Promise.resolve(this.storage.getItem(this.key)).catch(error => { throw readFailure(error) }) }
    catch (error) { throw readFailure(error) }
    const raw = await awaitLeaseRead(pendingRead, signal)
    signal?.throwIfAborted()
    if (raw === null) return null
    // archive-source 的固定生产 codec 来自本模块；延迟装载避免静态依赖环和旧入口额外工作。
    const [{ captureGzipArchiveSource }, { createValidatedArchiveLease }] = await awaitLeaseRead(Promise.all([
      import("./archive-source.ts"), import("./validated-archive-lease.ts"),
    ]), signal)
    signal?.throwIfAborted()
    const captured = captureGzipArchiveSource(raw)
    return createValidatedArchiveLease(captured, signal)
  }

  private async loadWith(parse: (text: string) => StrictSaveEnvelope): Promise<StrictSaveEnvelope | null> {
    let raw: string | null
    try {
      raw = await this.storage.getItem(this.key)
    } catch (error) {
      throw new Error(`读取浏览器存档失败：${error instanceof Error ? error.message : String(error)}`)
    }
    if (raw === null) return null
    if (!raw.startsWith("gzip:")) throw new Error("浏览器存档格式无效：缺少 gzip: 前缀")
    const decodeChunks = this.codec.decodeChunks
    if (parse === parseFrozenSaveJson && decodeChunks !== undefined) {
      const codec = this.codec
      const payload = raw.slice("gzip:".length)
      const decoded = async function* (): AsyncIterable<string> {
        try { yield* decodeChunks.call(codec, payload) }
        catch (error) { throw new Error(`解压浏览器存档失败：${error instanceof Error ? error.message : String(error)}`) }
      }
      return parseFrozenSaveJsonChunks(decoded())
    }
    let json: string
    try {
      json = await this.codec.decode(raw.slice("gzip:".length))
    } catch (error) {
      throw new Error(`解压浏览器存档失败：${error instanceof Error ? error.message : String(error)}`)
    }
    return parse(json)
  }
}

export class CompressedLocalStorageSaveRepository extends CompressedSaveRepository {
  constructor(storage: KeyValueStorage, key = "stock-game-save", codec = gzipSaveCodec) {
    super({
      getItem: (name) => storage.getItem(name),
      setItem: (name, value, isCurrent) => {
        if (!isCurrent()) return false;
        storage.setItem(name, value);
        return true;
      },
    }, key, codec);
  }
}
