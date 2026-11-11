import { RawSaveNumber, SaveSchemaError, string } from "./primitives.ts"

type Frame =
  | { kind: "object"; value: Record<string, unknown>; mode: "first" | "key" | "colon" | "value" | "after"; key?: string }
  | { kind: "array"; value: unknown[]; mode: "first" | "value" | "after" }
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

/** 增量词法入口；完整图只交给既有 schema 校验。 */
export class IncrementalRawSaveParser {
  private frames: Frame[] = []
  private path: (string | number)[] = []
  private lexeme?: Lexeme
  private root: unknown
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
  private accept(value: unknown): void {
    const parent = this.frames.at(-1)
    if (parent === undefined) { this.root = value; this.complete = true; return }
    if (parent.kind === "array") parent.value.push(value)
    else {
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
    if (!lexeme.key) { this.accept(decoded); return }
    this.path.pop()
    const frame = this.frames.at(-1)!
    if (frame.kind !== "object") throw new Error("增量 JSON key 缺少 object 容器")
    if (Object.hasOwn(frame.value, decoded)) { this.path.push(decoded); this.fail("包含重复 JSON 键") }
    frame.key = decoded
    frame.mode = "colon"
  }
  private endBare(token: string): void {
    if (!/^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?$/.test(token)) this.fail("包含非法 JSON 数值或值")
    this.lexeme = undefined
    this.accept(new RawSaveNumber(token))
  }
  private beginValue(character: string): void {
    if (character === '"') { this.beginString(false); return }
    if (character === "{") { this.frames.push({ kind: "object", value: {}, mode: "first" }); return }
    if (character === "[") { this.frames.push({ kind: "array", value: [], mode: "first" }); return }
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
            else if (lexeme.token === lexeme.target) { this.lexeme = undefined; this.accept(lexeme.value) }
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
          else { this.path.push(parent.value.length); this.beginValue(character) }
        } else this.beginValue(character)
        cursor++
      }
    } catch (error) { this.dispose(); throw error }
  }

  finish(): unknown {
    if (this.terminal) throw new Error("增量 JSON parser 已结束")
    try {
      if (this.lexeme?.kind === "string") this.fail("JSON 字符串未闭合")
      if (this.lexeme !== undefined) this.endBare(this.lexeme.token)
      if (this.complete) return this.root
      const parent = this.frames.at(-1)
      if (parent?.kind === "object") {
        if (parent.mode === "first" || parent.mode === "key") { this.path.push("key"); this.fail("JSON 对象键必须是字符串") }
        if (parent.mode === "colon") this.fail("JSON 字段缺少冒号")
        if (parent.mode === "after") this.fail("JSON 对象缺少逗号或结束符")
        this.path.push(parent.key!)
      } else if (parent?.kind === "array") {
        if (parent.mode === "after") this.fail("JSON 数组缺少逗号或结束符")
        this.path.push(parent.value.length)
      }
      this.fail("包含非法 JSON 数值或值")
    } finally { this.dispose() }
  }

  dispose(): void {
    this.frames = []; this.path = []; this.lexeme = undefined; this.root = undefined; this.property.value = undefined; this.terminal = true
  }
}
