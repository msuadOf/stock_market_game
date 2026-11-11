import { parseStrictSaveEnvelope, type StrictSaveEnvelope } from "./schema/root.ts"
import { RawSaveNumber, SaveSchemaError, validateSaveValue, withSaveGraphOwnership } from "./schema/primitives.ts"
import { parseRawSaveJson } from "./schema/raw-json.ts"
import { IncrementalRawSaveParser } from "./schema/incremental-raw-json.ts"

export function parseSaveSlot(value: unknown): StrictSaveEnvelope {
  return withSaveGraphOwnership(false, () => {
    validateSaveValue(value)
    return parseStrictSaveEnvelope(value)
  })
}

// 仅接收本次 JSON 解析独占的规范图；公共 materializeSaveValue 的复制契约不变。
function materializeOwnedJson<Value>(value: Value, path = "根节点"): Value {
  const descriptor = Object.assign(Object.create(null) as PropertyDescriptor, { value: undefined, writable: true, enumerable: true, configurable: true })
  const visit = (current: unknown, currentPath: string): unknown => {
    if (current instanceof RawSaveNumber) {
      if (current.decoded === undefined) throw new SaveSchemaError(currentPath, "数值未经过字段 codec 校验")
      return current.decoded
    }
    if (Array.isArray(current)) {
      for (let index = 0; index < current.length; index++) current[index] = visit(current[index], `${currentPath}[${index}]`)
    } else if (current !== null && typeof current === "object") {
      for (const key of Object.keys(current)) {
        const record = current as Record<string, unknown>
        descriptor.value = visit(record[key], `${currentPath}.${key}`)
        Object.defineProperty(record, key, descriptor)
      }
    }
    return current
  }
  return visit(value, path) as Value
}

export function parseSaveJson(text: string): StrictSaveEnvelope {
  try {
    // raw 图由本次调用独占；codec 成功后立即消费 map 行，避免同时保留整档双图。
    const raw = parseRawSaveJson(text)
    return withSaveGraphOwnership(true, () => materializeOwnedJson(parseStrictSaveEnvelope(raw)))
  } catch (error) {
    if (error instanceof SyntaxError) throw new Error(`存档不是合法 JSON：${error.message}`)
    throw error
  }
}

// 只有本模块从 JSON 新建、完整校验并深冻结的图可以跨准入阶段共享。
const frozenJsonArchives = new WeakSet<object>()

/** 从 JSON 构造并冻结独占存档快照，供快速槽读取复用。 */
export function parseFrozenSaveJson(text: string): StrictSaveEnvelope {
  return freezeJsonArchive(parseSaveJson(text))
}

function freezeJsonArchive(archive: StrictSaveEnvelope): StrictSaveEnvelope {
  const freeze = (value: unknown): void => {
    if (value === null || typeof value !== "object") return
    if (Array.isArray(value)) {
      for (const child of value) freeze(child)
    } else {
      for (const key of Object.keys(value)) freeze((value as Record<string, unknown>)[key])
    }
    Object.freeze(value)
  }
  freeze(archive)
  frozenJsonArchives.add(archive)
  return archive
}

/** 流式读取入口同样必须完成完整词法与 schema 校验。 */
export async function parseFrozenSaveJsonChunks(chunks: AsyncIterable<string>): Promise<StrictSaveEnvelope> {
  const parser = new IncrementalRawSaveParser()
  let lexicalFailure: { error: unknown } | undefined
  try {
    for await (const chunk of chunks) {
      if (lexicalFailure !== undefined) continue
      try { parser.feed(chunk) }
      catch (error) {
        // 完整解压原本先于 JSON；锁存词法首错并排空流，让后到的 gzip 错误保持优先。
        lexicalFailure = { error }
        parser.dispose()
      }
    }
    if (lexicalFailure !== undefined) throw lexicalFailure.error
    const raw = parser.finish()
    const archive = withSaveGraphOwnership(true, () => materializeOwnedJson(parseStrictSaveEnvelope(raw)))
    return freezeJsonArchive(archive)
  } finally { parser.dispose() }
}

/** 不接受外部标记或普通冻结对象冒充可信快照。 */
export function isFrozenJsonArchive(value: unknown): value is StrictSaveEnvelope {
  return value !== null && typeof value === "object" && frozenJsonArchives.has(value)
}
