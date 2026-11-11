/**
 * 有界归档内容摘要，不等同于标准 SHA-256(file)。D 为下方带 NUL 的 ASCII 域：
 * H0 = SHA256(D || 0x00)
 * Hi+1 = SHA256(D || 0x01 || Hi || u64be(i) || u32be(len(Ci)) || Ci)
 * result = SHA256(D || 0x02 || Hn || u64be(n) || u64be(totalBytes))
 * Ci 按原始字节顺序固定分为 64KiB，只有末块可短；空流不生成数据块。
 */
export const SHA256_CHAIN_ALGORITHM = "sha256-chain-v1" as const
export const SHA256_CHAIN_BLOCK_BYTES = 65536
export const SHA256_CHAIN_MAX_BYTES = (1n << 64n) - 1n
export type Sha256ChainResult = Readonly<{ algorithm: typeof SHA256_CHAIN_ALGORITHM; blockBytes: number; byteLength: bigint; blocks: bigint; digest: string }>
export interface Sha256Chain {
  readonly byteLength: bigint
  /** 单次输入最多 64KiB，调用完成后才允许下一次 update/finish。 */
  update(bytes: Uint8Array): Promise<void>
  /** 一次性结束；之后 update/finish 均显式拒绝。 */
  finish(): Promise<Sha256ChainResult>
}
export interface Utf8Sha256Chain {
  /** 尚未配对的尾 high surrogate 要到后续字符或 finish 时才产生字节。 */
  readonly byteLength: bigint
  update(text: string): Promise<void>
  finish(): Promise<Sha256ChainResult>
}

const DOMAIN = new TextEncoder().encode("stock-market-game/archive-digest/sha256-chain-v1\0")
const TAG_OFFSET = DOMAIN.length
const STATE_OFFSET = TAG_OFFSET + 1
const INDEX_OFFSET = STATE_OFFSET + 32
const LENGTH_OFFSET = INDEX_OFFSET + 8
const DATA_OFFSET = LENGTH_OFFSET + 4
// 固定原生函数身份；调用者不能提供 digest 函数或替换模块已取得的实现。
const platformSubtle = globalThis.crypto?.subtle
const platformDigest = platformSubtle?.digest.bind(platformSubtle)

async function sha256(bytes: Uint8Array<ArrayBuffer>): Promise<Uint8Array<ArrayBuffer>> {
  if (platformDigest === undefined) throw new Error("Web Crypto SHA-256 不可用，不能生成归档摘要")
  const digest = await platformDigest("SHA-256", bytes)
  if (digest.byteLength !== 32) throw new Error("Web Crypto SHA-256 返回长度无效")
  return new Uint8Array(digest)
}

/** 单独保留精确长度运算，边界可测试而无需伪造内部摘要状态或创建超大输入。 */
export function addSha256ChainByteLength(current: bigint, additional: number): bigint {
  if (typeof current !== "bigint" || current < 0n || current > SHA256_CHAIN_MAX_BYTES) throw new Error("归档摘要当前长度必须为 u64 字节数")
  if (!Number.isSafeInteger(additional) || additional < 0) throw new Error("归档摘要新增长度必须为非负 safe integer")
  const next = current + BigInt(additional)
  if (next > SHA256_CHAIN_MAX_BYTES) throw new Error("归档摘要总长度超过 u64 上限")
  return next
}

class Chain implements Sha256Chain {
  // 唯一 canonical 数据块与帧头共用缓冲；Web Crypto 单次取副本也至多一个有界块。
  #frame = new Uint8Array(DATA_OFFSET + SHA256_CHAIN_BLOCK_BYTES)
  #view = new DataView(this.#frame.buffer)
  #state: Uint8Array<ArrayBuffer>
  #used = 0
  #length = 0n
  #blocks = 0n
  #busy = false
  #finished = false
  #failure?: { error: unknown }
  constructor(state: Uint8Array<ArrayBuffer>) { this.#state = state; this.#frame.set(DOMAIN) }
  get byteLength(): bigint { return this.#length }
  #assertOpen(): void {
    if (this.#failure !== undefined) throw new Error("归档摘要此前失败，必须创建新实例", { cause: this.#failure.error })
    if (this.#busy) throw new Error("归档摘要操作进行中，禁止并发调用")
    if (this.#finished) throw new Error("归档摘要已经结束，禁止继续使用")
  }
  #dataFrame(length: number): Uint8Array<ArrayBuffer> {
    this.#frame[TAG_OFFSET] = 1
    this.#frame.set(this.#state, STATE_OFFSET)
    this.#view.setBigUint64(INDEX_OFFSET, this.#blocks, false)
    this.#view.setUint32(LENGTH_OFFSET, length, false)
    return this.#frame.subarray(0, DATA_OFFSET + length)
  }
  async update(bytes: Uint8Array): Promise<void> {
    this.#assertOpen()
    if (!(bytes instanceof Uint8Array) || !ArrayBuffer.isView(bytes)) throw new Error("归档摘要分块必须为 Uint8Array")
    if (!(bytes.buffer instanceof ArrayBuffer)) throw new Error("归档摘要不接受可并发修改的 SharedArrayBuffer")
    if (bytes.byteLength > SHA256_CHAIN_BLOCK_BYTES) throw new Error("归档摘要单次分块超过 64KiB 上限")
    const next = addSha256ChainByteLength(this.#length, bytes.byteLength)
    this.#busy = true
    try {
      const first = Math.min(bytes.length, SHA256_CHAIN_BLOCK_BYTES - this.#used)
      this.#frame.set(bytes.subarray(0, first), DATA_OFFSET + this.#used)
      this.#used += first
      this.#length = next
      if (this.#used === SHA256_CHAIN_BLOCK_BYTES) {
        // Web Crypto 规范在 digest 调用中复制 BufferSource；其后可复用本缓冲保存尾部。
        // 单次输入上限保证至多产生一个完整块，所有 caller 字节均在首个 await 前消费。
        const pending = sha256(this.#dataFrame(this.#used))
        const remaining = bytes.length - first
        this.#frame.set(bytes.subarray(first), DATA_OFFSET)
        this.#used = remaining
        this.#state = await pending
        this.#blocks++
      }
    } catch (error) { this.#failure = { error }; throw error }
    finally { this.#busy = false }
  }
  async finish(): Promise<Sha256ChainResult> {
    this.#assertOpen()
    this.#busy = true; this.#finished = true
    try {
      if (this.#used > 0) {
        this.#state = await sha256(this.#dataFrame(this.#used))
        this.#blocks++
        this.#used = 0
      }
      const final = new Uint8Array(DOMAIN.length + 1 + 32 + 16)
      final.set(DOMAIN); final[TAG_OFFSET] = 2; final.set(this.#state, STATE_OFFSET)
      const view = new DataView(final.buffer)
      view.setBigUint64(INDEX_OFFSET, this.#blocks, false)
      view.setBigUint64(INDEX_OFFSET + 8, this.#length, false)
      const digest = [...await sha256(final)].map(byte => byte.toString(16).padStart(2, "0")).join("")
      return Object.freeze({ algorithm: SHA256_CHAIN_ALGORITHM, blockBytes: SHA256_CHAIN_BLOCK_BYTES, byteLength: this.#length, blocks: this.#blocks, digest })
    } catch (error) { this.#failure = { error }; throw error }
    finally { this.#busy = false; this.#frame.fill(0); this.#state.fill(0) }
  }
}

/** 工厂先取得固定 H0；不允许注入调用者提供的初始 hash 或 digest。 */
export async function createSha256Chain(): Promise<Sha256Chain> {
  const initial = new Uint8Array(DOMAIN.length + 1)
  initial.set(DOMAIN)
  return new Chain(await sha256(initial))
}

class Utf8Chain implements Utf8Sha256Chain {
  #chain: Sha256Chain
  #encoder = new TextEncoder()
  #pendingHigh = ""
  #busy = false
  #finished = false
  #failure?: { error: unknown }
  constructor(chain: Sha256Chain) { this.#chain = chain }
  get byteLength(): bigint { return this.#chain.byteLength }
  #assertOpen(): void {
    if (this.#failure !== undefined) throw new Error("UTF-8 归档摘要此前失败，必须创建新实例", { cause: this.#failure.error })
    if (this.#busy) throw new Error("UTF-8 归档摘要操作进行中，禁止并发调用")
    if (this.#finished) throw new Error("UTF-8 归档摘要已经结束，禁止继续使用")
  }
  async update(text: string): Promise<void> {
    this.#assertOpen()
    if (typeof text !== "string") throw new Error("UTF-8 归档摘要分块必须为字符串")
    this.#busy = true
    try {
      // 最坏每 code unit 3 字节；一段至多 16385 units，编码缓冲始终小于 64KiB。
      for (let offset = 0; offset < text.length; offset += 16384) {
        let part = this.#pendingHigh + text.slice(offset, offset + 16384)
        this.#pendingHigh = ""
        const tail = part.charCodeAt(part.length - 1)
        if (tail >= 0xd800 && tail <= 0xdbff) { this.#pendingHigh = part.slice(-1); part = part.slice(0, -1) }
        await this.#chain.update(this.#encoder.encode(part))
      }
    } catch (error) { this.#failure = { error }; throw error }
    finally { this.#busy = false }
  }
  async finish(): Promise<Sha256ChainResult> {
    this.#assertOpen()
    this.#busy = true; this.#finished = true
    try {
      // 与整体 TextEncoder 相同：最终未配对 surrogate 编码为 U+FFFD，不静默删除。
      if (this.#pendingHigh !== "") { await this.#chain.update(this.#encoder.encode(this.#pendingHigh)); this.#pendingHigh = "" }
      return await this.#chain.finish()
    } catch (error) { this.#failure = { error }; throw error }
    finally { this.#busy = false; this.#pendingHigh = "" }
  }
}

export async function createUtf8Sha256Chain(): Promise<Utf8Sha256Chain> { return new Utf8Chain(await createSha256Chain()) }
