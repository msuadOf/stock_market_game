import { RawSaveNumber, SaveSchemaError, string } from "./primitives.ts"

export type ArchiveRootScan = Readonly<{
  rootKind: string
  rootKeys: readonly string[]
  /** 尚未经过领域 codec 的 raw 元数据；RawSaveNumber 仍需逐字段校验。 */
  rawMetadata: Readonly<Record<string, unknown>>
  keySets: Readonly<Record<string, readonly string[]>>
  keySetKinds: Readonly<Record<string, ArchiveContainerShape["kind"]>>
  utf16Characters: number
  utf8Bytes: number
  capturedContainers: number
  peakOpenContainers: number
  schemaValidated: false
}>

export type ArchiveRecord = Readonly<{ path: readonly (string | number)[]; insertion: number; value: unknown }>
export type ArchiveContainerShape = Readonly<{ path: readonly (string | number)[]; kind: "object" | "array" | "string" | "number" | "boolean" | "null"; keys: readonly string[]; length: number }>
export type ArchiveRecordScanOptions = Readonly<{
  records: readonly (readonly string[])[]
  onRecord(record: ArchiveRecord): void
  onShape?(shape: ArchiveContainerShape): void
}>

const ROOT_METADATA = new Set(["setup", "seed", "civil_clock", "market_memberships", "book_next_sequences", "receipt_watermark", "urgency_policy", "rng_state", "next_order_id", "desk_mirrored_resting", "price_history", "market_minute_closes", "npc_order_lifecycles", "pending_player", "ingress_receipt_cursors", "company_state", "report_correction_operations", "public_library", "disclosures"])
const SNAPSHOT_METADATA = new Set(["seq", "tick", "markets", "daily_candles", "active_daily_candles"])
const RUNTIME_METADATA = new Set(["poisoned", "next_receipt_base", "live_envelopes", "book_public_cutoff_sequences", "retail_projection_seen"])
const PLAN_METADATA = new Set(["policy", "owner_sequences"])
const KEY_SETS = new Set(["snapshot.accounts", "history_reads", "runtime_state.strategy_states"])

function overlaps(left: readonly string[], right: readonly string[]): boolean {
  for (let index = 0; index < Math.min(left.length, right.length); index++) {
    if (left[index] !== "*" && right[index] !== "*" && left[index] !== right[index]) return false
  }
  return true
}
function recordOptions(options: ArchiveRecordScanOptions | undefined): ArchiveRecordScanOptions | undefined {
  if (options === undefined) return undefined
  if (typeof options.onRecord !== "function" || (options.onShape !== undefined && typeof options.onShape !== "function")) throw new Error("记录 sink 必须为函数")
  const metadata = [...ROOT_METADATA].map(key => [key]).concat([...SNAPSHOT_METADATA].map(key => ["snapshot", key]), [...RUNTIME_METADATA].map(key => ["runtime_state", key]), [...PLAN_METADATA].map(key => ["plans", key]))
  const records = options.records.map((path, index) => {
    if (path.length === 0 || path.some(segment => typeof segment !== "string")) throw new Error("记录路径不能为空且必须由字符串构成")
    if (options.records.slice(0, index).some(previous => overlaps(previous, path))) throw new Error("记录路径不能前缀重叠")
    if (metadata.some(selected => overlaps(selected, path))) throw new Error("记录路径不能与保留元数据路径重叠")
    return Object.freeze([...path])
  })
  return Object.freeze({ ...options, records: Object.freeze(records) })
}

/** 与 Object.keys 相同：数组索引先升序，其余字符串保持首次出现序。 */
function enumerateKeys(keys: ReadonlySet<string>): string[] {
  const indices: string[] = [], others: string[] = []
  for (const key of keys) {
    const index = Number(key)
    if (Number.isInteger(index) && index >= 0 && index < 4294967295 && String(index) === key) indices.push(key)
    else others.push(key)
  }
  indices.sort((left, right) => Number(left) - Number(right))
  return indices.concat(others)
}

type Frame =
  | { kind: "object"; value?: Record<string, unknown>; keys: Set<string>; mode: "first" | "key" | "colon" | "value" | "after"; key?: string }
  | { kind: "array"; value?: unknown[]; length: number; mode: "first" | "value" | "after" }
type Lexeme =
  | { kind: "string"; parts: string[]; escaped: boolean; key: boolean }
  | { kind: "bare"; token: string }
  | { kind: "literal"; token: string; target: string; value: unknown }

function whitespace(character: string): boolean {
  return character === " " || character === "\t" || character === "\r" || character === "\n"
}
function delimiter(character: string): boolean {
  return whitespace(character) || character === "," || character === "]" || character === "}"
}

/** 完整词法扫描只构造白名单元数据；未捕获的值在容器结束后释放。 */
class ArchiveRootScanner {
  private options?: ArchiveRecordScanOptions
  private sinkFailure?: { error: unknown }

  constructor(options?: ArchiveRecordScanOptions) {
    this.options = recordOptions(options)
  }

  private recordPrefix(pattern: readonly string[], exact = false): boolean {
    if (exact ? this.path.length !== pattern.length : this.path.length < pattern.length) return false
    return pattern.every((segment, index) => segment === "*" || segment === this.path[index])
  }
  private emit(operation: () => void): void {
    if (this.sinkFailure !== undefined) return
    try { operation() }
    catch (error) { this.sinkFailure = { error } }
  }
  private frames: Frame[] = []
  private path: (string | number)[] = []
  private lexeme?: Lexeme
  private rootKind = "unseen"
  private rootKeys: string[] = []
  private metadata: Record<string, unknown> = {}
  private keySets: Record<string, readonly string[]> = {}
  private keySetKinds: Record<string, ArchiveContainerShape["kind"]> = {}
  private utf16Characters = 0
  private utf8Bytes = 0
  private pendingHighSurrogate = false
  private capturedContainers = 0
  private peakOpenContainers = 0

  private selectedName(): string | undefined {
    const [root, child] = this.path
    if (this.path.length === 1 && typeof root === "string" && ROOT_METADATA.has(root)) return root
    if (this.path.length === 2 && root === "snapshot" && typeof child === "string" && SNAPSHOT_METADATA.has(child)) return `snapshot.${child}`
    if (this.path.length === 2 && root === "runtime_state" && typeof child === "string" && RUNTIME_METADATA.has(child)) return `runtime_state.${child}`
    if (this.path.length === 2 && root === "plans" && typeof child === "string" && PLAN_METADATA.has(child)) return `plans.${child}`
    return undefined
  }
  private capture(): boolean {
    if (this.sinkFailure !== undefined) return false
    const [root, child] = this.path
    return (typeof root === "string" && ROOT_METADATA.has(root))
      || (root === "snapshot" && typeof child === "string" && SNAPSHOT_METADATA.has(child))
      || (root === "runtime_state" && typeof child === "string" && RUNTIME_METADATA.has(child))
      || (root === "plans" && typeof child === "string" && PLAN_METADATA.has(child))
      || (this.options?.records.some(pattern => this.recordPrefix(pattern)) === true)
  }
  private count(chunk: string): void {
    this.utf16Characters += chunk.length
    for (let index = 0; index < chunk.length; index++) {
      const unit = chunk.charCodeAt(index)
      if (this.pendingHighSurrogate) {
        this.pendingHighSurrogate = false
        if (unit >= 0xdc00 && unit <= 0xdfff) { this.utf8Bytes += 4; continue }
        this.utf8Bytes += 3
      }
      if (unit >= 0xd800 && unit <= 0xdbff) this.pendingHighSurrogate = true
      else this.utf8Bytes += unit <= 0x7f ? 1 : unit <= 0x7ff ? 2 : 3
    }
    if (!Number.isSafeInteger(this.utf8Bytes) || !Number.isSafeInteger(this.utf16Characters)) throw new Error("存档源长度超出 safe integer 范围")
  }
  private complete = false
  private terminal = false
  private readonly property = Object.assign(Object.create(null) as PropertyDescriptor, { value: undefined, writable: true, enumerable: true, configurable: true })

  private renderPath(): string {
    let path = "根节点"
    for (const segment of this.path) path += typeof segment === "number" ? `[${segment}]` : `.${segment}`
    return path
  }
  private fail(detail: string): never {
    throw new SaveSchemaError(this.renderPath(), `存档不是合法 JSON：${detail}`)
  }
  private keySetName(): string | undefined {
    const name = this.path.join(".")
    return (this.path.length === 1 && this.path[0] === "history_reads")
      || (this.path.length === 2 && KEY_SETS.has(name)) ? name : undefined
  }
  private accept(value: unknown, kind?: ArchiveContainerShape["kind"]): void {
    const keySetName = this.keySetName()
    if (keySetName !== undefined && kind !== undefined) {
      this.keySetKinds[keySetName] = kind
      // RawSaveNumber 的 enumerable 键也是旧 exact 对数字 raw 值可见的形状。
      this.keySets[keySetName] = Object.freeze(kind === "string" ? Object.keys(value as string) : kind === "number" ? ["token", "decoded"] : [])
    }
    const selected = this.selectedName()
    if (selected !== undefined) Object.defineProperty(this.metadata, selected, { value, writable: true, enumerable: true, configurable: true })
    const parent = this.frames.at(-1)
    const options = this.options
    if (kind !== undefined && options?.onShape !== undefined && this.sinkFailure === undefined && options.records.some(pattern => this.path.length < pattern.length && this.path.every((segment, index) => pattern[index] === "*" || pattern[index] === segment))) {
      const shape = Object.freeze({ path: Object.freeze([...this.path]), kind, keys: Object.freeze([] as string[]), length: 0 })
      this.emit(() => options.onShape!(shape))
    }
    if (options !== undefined && this.sinkFailure === undefined && options.records.some(pattern => this.recordPrefix(pattern, true))) {
      const insertion = parent === undefined ? 0 : parent.kind === "array" ? parent.length : parent.keys.size - 1
      const record = Object.freeze({ path: Object.freeze([...this.path]), insertion, value })
      this.emit(() => options.onRecord(record))
    }
    if (parent === undefined) { this.complete = true; return }
    if (parent.kind === "array") { parent.value?.push(value); parent.length++ }
    else if (parent.value !== undefined) {
      const key = parent.key!
      if (key in parent.value) {
        this.property.value = value
        Object.defineProperty(parent.value, key, this.property)
        this.property.value = undefined
      } else parent.value[key] = value
    }
    parent.mode = "after"
    this.path.pop()
  }
  private close(): void {
    const frame = this.frames.pop()!
    const options = this.options
    if (options?.onShape !== undefined && this.sinkFailure === undefined && options.records.some(pattern => this.path.length < pattern.length && this.path.every((segment, index) => pattern[index] === "*" || pattern[index] === segment))) {
      const shape = Object.freeze({ path: Object.freeze([...this.path]), kind: frame.kind, keys: Object.freeze(frame.kind === "object" ? enumerateKeys(frame.keys) : []), length: frame.kind === "array" ? frame.length : frame.keys.size })
      this.emit(() => options.onShape!(shape))
    }
    if (frame.kind === "object" && this.path.length === 0) this.rootKeys = enumerateKeys(frame.keys)
    const keySetName = this.keySetName()
    if (keySetName !== undefined) {
      this.keySetKinds[keySetName] = frame.kind
      this.keySets[keySetName] = Object.freeze(frame.kind === "object" ? enumerateKeys(frame.keys) : Array.from({ length: frame.length }, (_, index) => String(index)))
    }
    this.accept(frame.value)
  }
  private beginString(key: boolean): void {
    if (key) this.path.push("key")
    // 前缀使 join 分配独立 token，随后只保留小 token；不能让 slice 吊住解压 chunk。
    this.lexeme = { kind: "string", parts: [" ", '"'], escaped: false, key }
  }
  private endString(lexeme: Extract<Lexeme, { kind: "string" }>): void {
    const token = lexeme.parts.join("").slice(1)
    let decoded: string
    try { decoded = string(JSON.parse(token), this.renderPath()) }
    catch (error) {
      if (error instanceof SaveSchemaError) throw error
      throw new SaveSchemaError(this.renderPath(), `包含非法 JSON 字符串：${error instanceof Error ? error.message : String(error)}`)
    }
    this.lexeme = undefined
    if (!lexeme.key) { this.accept(decoded, "string"); return }
    this.path.pop()
    const frame = this.frames.at(-1)!
    if (frame.kind !== "object") throw new Error("增量 JSON key 缺少 object 容器")
    if (frame.keys.has(decoded)) { this.path.push(decoded); this.fail("包含重复 JSON 键") }
    frame.keys.add(decoded)
    frame.key = decoded
    frame.mode = "colon"
  }
  private endBare(token: string): void {
    if (!/^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?$/.test(token)) this.fail("包含非法 JSON 数值或值")
    this.lexeme = undefined
    this.accept(this.capture() ? new RawSaveNumber(token) : undefined, "number")
  }
  private beginValue(character: string): void {
    if (this.path.length === 0 && this.frames.length === 0) this.rootKind = character === "{" ? "object" : character === "[" ? "array" : character === '"' ? "string" : character === "n" ? "null" : character === "t" || character === "f" ? "boolean" : "number"
    if (character === '"') { this.beginString(false); return }
    if (character === "{" || character === "[") {
      const capture = this.capture()
      if (capture) this.capturedContainers++
      if (character === "{") this.frames.push({ kind: "object", value: capture ? {} : undefined, keys: new Set(), mode: "first" })
      else this.frames.push({ kind: "array", value: capture ? [] : undefined, length: 0, mode: "first" })
      this.peakOpenContainers = Math.max(this.peakOpenContainers, this.frames.length)
      return
    }
    if (delimiter(character)) this.fail("包含非法 JSON 数值或值")
    if (character === "n") this.lexeme = { kind: "literal", token: character, target: "null", value: null }
    else if (character === "t") this.lexeme = { kind: "literal", token: character, target: "true", value: true }
    else if (character === "f") this.lexeme = { kind: "literal", token: character, target: "false", value: false }
    else this.lexeme = { kind: "bare", token: character }
  }

  feed(chunk: string): void {
    if (this.terminal) throw new Error("增量 JSON parser 已结束")
    if (typeof chunk !== "string") throw new SaveSchemaError("根节点", "必须是 JSON 字符串")
    try {
      this.count(chunk)
      let cursor = 0
      while (cursor < chunk.length) {
        const lexeme = this.lexeme
        if (lexeme?.kind === "string") {
          const start = cursor
          let ended = false
          for (; cursor < chunk.length; cursor++) {
            const character = chunk[cursor]!
            if (lexeme.escaped) { lexeme.escaped = false; continue }
            if (character === "\\") { lexeme.escaped = true; continue }
            if (character === '"') { cursor++; ended = true; break }
          }
          lexeme.parts.push(chunk.slice(start, cursor))
          if (ended) this.endString(lexeme)
          continue
        }
        const character = chunk[cursor]!
        if (lexeme !== undefined) {
          if (delimiter(character)) { this.endBare(lexeme.token); continue }
          lexeme.token += character
          cursor++
          if (lexeme.kind === "literal") {
            if (!lexeme.target.startsWith(lexeme.token)) this.lexeme = { kind: "bare", token: lexeme.token }
            else if (lexeme.token === lexeme.target) { this.lexeme = undefined; this.accept(lexeme.value, lexeme.value === null ? "null" : "boolean") }
          }
          continue
        }
        if (whitespace(character)) { cursor++; continue }
        if (this.complete) this.fail("JSON 根值之后包含多余内容")
        const parent = this.frames.at(-1)
        if (parent?.kind === "object") {
          if (parent.mode === "first" || parent.mode === "key") {
            if (character === "}" && parent.mode === "first") this.close()
            else {
              if (character !== '"') { this.path.push("key"); this.fail("JSON 对象键必须是字符串") }
              this.beginString(true)
            }
          } else if (parent.mode === "colon") {
            if (character !== ":") this.fail("JSON 字段缺少冒号")
            parent.mode = "value"
          } else if (parent.mode === "after") {
            if (character === "}") this.close()
            else if (character === ",") parent.mode = "key"
            else this.fail("JSON 对象缺少逗号或结束符")
          } else { this.path.push(parent.key!); this.beginValue(character) }
        } else if (parent?.kind === "array") {
          if (parent.mode === "after") {
            if (character === "]") this.close()
            else if (character === ",") parent.mode = "value"
            else this.fail("JSON 数组缺少逗号或结束符")
          } else if (character === "]" && parent.mode === "first") this.close()
          else { this.path.push(parent.length); this.beginValue(character) }
        } else this.beginValue(character)
        cursor++
      }
    } catch (error) { this.dispose(); throw error }
  }

  finish(): ArchiveRootScan {
    if (this.terminal) throw new Error("增量 JSON parser 已结束")
    try {
      if (this.lexeme?.kind === "string") this.fail("JSON 字符串未闭合")
      if (this.lexeme !== undefined) this.endBare(this.lexeme.token)
      if (this.complete) {
        if (this.sinkFailure !== undefined) throw this.sinkFailure.error
        if (this.pendingHighSurrogate) this.utf8Bytes += 3
        return Object.freeze({ rootKind: this.rootKind, rootKeys: Object.freeze(this.rootKeys), rawMetadata: Object.freeze(this.metadata), keySets: Object.freeze(this.keySets), keySetKinds: Object.freeze(this.keySetKinds), utf16Characters: this.utf16Characters, utf8Bytes: this.utf8Bytes, capturedContainers: this.capturedContainers, peakOpenContainers: this.peakOpenContainers, schemaValidated: false as const })
      }
      const parent = this.frames.at(-1)
      if (parent?.kind === "object") {
        if (parent.mode === "first" || parent.mode === "key") { this.path.push("key"); this.fail("JSON 对象键必须是字符串") }
        if (parent.mode === "colon") this.fail("JSON 字段缺少冒号")
        if (parent.mode === "after") this.fail("JSON 对象缺少逗号或结束符")
        this.path.push(parent.key!)
      } else if (parent?.kind === "array") {
        if (parent.mode === "after") this.fail("JSON 数组缺少逗号或结束符")
        this.path.push(parent.length)
      }
      this.fail("包含非法 JSON 数值或值")
    } finally { this.dispose() }
  }

  dispose(): void {
    this.frames = []; this.path = []; this.lexeme = undefined; this.metadata = {}; this.keySets = {}; this.keySetKinds = {}; this.rootKeys = []; this.property.value = undefined; this.options = undefined; this.sinkFailure = undefined; this.terminal = true
  }
}

/** 先完成整个输入的词法扫描；流失败仍优先于已锁存的词法错误。 */
export async function scanArchiveRoot(chunks: AsyncIterable<string>, options?: ArchiveRecordScanOptions): Promise<ArchiveRootScan> {
  const scanner = new ArchiveRootScanner(options)
  let lexicalFailure: { error: unknown } | undefined
  try {
    for await (const chunk of chunks) {
      if (lexicalFailure !== undefined) continue
      try { scanner.feed(chunk) }
      catch (error) { lexicalFailure = { error }; scanner.dispose() }
    }
    if (lexicalFailure !== undefined) throw lexicalFailure.error
    return scanner.finish()
  } finally { scanner.dispose() }
}
