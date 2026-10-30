import { parseMoney } from "../../utils/money.ts"

export type JsonRecord = Record<string, unknown>

/** 原始数值在对应 integer/finite codec 消费前不得先经 JSON.parse 舍入。 */
export class RawSaveNumber {
  readonly token: string
  decoded: number | undefined
  constructor(token: string) { this.token = token }
}

/** 校验后返回原始 opaque 子树的叶也必须消费过数值 codec，不能漏出 token。 */
export function materializeSaveValue<T>(value: T, path = "根节点"): T {
  const descriptor = Object.assign(Object.create(null) as PropertyDescriptor, {
    value: undefined,
    writable: true,
    enumerable: true,
    configurable: true,
  })
  const visit = (current: unknown, currentPath: string): unknown => {
    if (current instanceof RawSaveNumber) {
      if (current.decoded === undefined) throw new SaveSchemaError(currentPath, "数值未经过字段 codec 校验")
      return current.decoded
    }
    if (Array.isArray(current)) return current.map((item, index) => visit(item, `${currentPath}[${index}]`))
    if (current !== null && typeof current === "object") {
      const result: Record<string, unknown> = {}
      for (const key of Object.keys(current)) {
        const nested = visit((current as Record<string, unknown>)[key], `${currentPath}.${key}`)
        if (key in result) {
          descriptor.value = nested
          Object.defineProperty(result, key, descriptor)
        } else {
          result[key] = nested
        }
      }
      return result
    }
    return current
  }
  return visit(value, path) as T
}

export function validateSaveValue(value: unknown, path = "根节点", ancestors = new Set<object>()): void {
  if (value instanceof RawSaveNumber) return
  if (typeof value === "string") { string(value, path); return }
  if (value === null || typeof value === "number" || typeof value === "boolean") return
  if (typeof value !== "object") throw new SaveSchemaError(path, "不是 JSON 值")
  if (ancestors.has(value)) throw new SaveSchemaError(path, "包含循环引用")
  ancestors.add(value)
  if (Array.isArray(value)) {
    for (let index = 0; index < value.length; index++) {
      if (!Object.hasOwn(value, index)) throw new SaveSchemaError(`${path}[${index}]`, "数组元素缺失")
      validateSaveValue(value[index], `${path}[${index}]`, ancestors)
    }
  } else {
    for (const [key, nested] of Object.entries(value)) {
      string(key, `${path}.key`)
      validateSaveValue(nested, `${path}.${key}`, ancestors)
    }
  }
  ancestors.delete(value)
}

const UNSIGNED_DECIMAL = /^\d+$/
const ACCOUNTING_AMOUNT = /^-?\d+\.\d{2}$/
const ISO_CIVIL_DATE = /^\d{4}-\d{2}-\d{2}$/
const U64_MAX = 18_446_744_073_709_551_615n
const JAVASCRIPT_SAFE_INTEGER_MAX = 9_007_199_254_740_991n

export class SaveSchemaError extends Error {
  readonly name = "SaveSchemaError"
  readonly path: string

  constructor(path: string, detail: string) {
    super(`存档 ${path} ${detail}`)
    this.path = path
  }
}

export function record(value: unknown, path: string): JsonRecord {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new SaveSchemaError(path, "必须是对象")
  }
  return { ...value }
}

export function exact(value: JsonRecord, keys: readonly string[], path: string): void {
  const expected = new Set(keys)
  const missing = keys.filter((key) => !Object.hasOwn(value, key))
  const extra = Object.keys(value).filter((key) => !expected.has(key))
  if (missing.length > 0) throw new SaveSchemaError(`${path}.${missing[0]}`, "是必填字段")
  if (extra.length > 0) throw new SaveSchemaError(`${path}.${extra[0]}`, "不是允许字段")
}

export function array(value: unknown, path: string): readonly unknown[] {
  if (!Array.isArray(value)) throw new SaveSchemaError(path, "必须是数组")
  for (let index = 0; index < value.length; index++) if (!Object.hasOwn(value, index)) throw new SaveSchemaError(`${path}[${index}]`, "数组元素缺失")
  return value
}

export function string(value: unknown, path: string): string {
  if (typeof value !== "string") throw new SaveSchemaError(path, "必须是字符串")
  for (let index = 0; index < value.length; index++) {
    const unit = value.charCodeAt(index)
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(++index)
      if (!(next >= 0xdc00 && next <= 0xdfff)) throw new SaveSchemaError(path, "包含孤立 Unicode surrogate")
    } else if (unit >= 0xdc00 && unit <= 0xdfff) throw new SaveSchemaError(path, "包含孤立 Unicode surrogate")
  }
  return value
}

export function boolean(value: unknown, path: string): boolean {
  if (typeof value !== "boolean") throw new SaveSchemaError(path, "必须是布尔值")
  return value
}

export function integer(value: unknown, path: string, minimum?: number): number {
  const source = value instanceof RawSaveNumber ? value : undefined
  if (value instanceof RawSaveNumber) {
    if (!/^(?:0|-?[1-9][0-9]*)$/.test(value.token)) throw new SaveSchemaError(path, "整数 token 不接受负零、小数或指数")
    value = Number(value.token)
  }
  if (typeof value !== "number" || !Number.isSafeInteger(value) || Object.is(value, -0) || (minimum !== undefined && value < minimum)) {
    throw new SaveSchemaError(path, minimum === undefined ? "必须是安全整数" : `必须是不小于 ${minimum} 的安全整数`)
  }
  if (source !== undefined) source.decoded = value
  return value
}

export function finite(value: unknown, path: string): number {
  const source = value instanceof RawSaveNumber ? value : undefined
  if (value instanceof RawSaveNumber) value = Number(value.token)
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new SaveSchemaError(path, "必须是有限数值")
  }
  if (source !== undefined) source.decoded = value
  return value
}

export function money(value: unknown, path: string): string {
  try {
    return parseMoney(value, path)
  } catch (error) {
    if (!(error instanceof Error)) throw error
    throw new SaveSchemaError(path, error.message)
  }
}

export function decimal(value: unknown, path: string): string {
  const parsed = string(value, path)
  if (!UNSIGNED_DECIMAL.test(parsed) || BigInt(parsed) > U64_MAX) throw new SaveSchemaError(path, "必须是 u64 范围内的无损非负十进制字符串")
  return parsed
}

export function safeIntegerKey(value: string, path: string): void {
  if (!/^(0|[1-9]\d*)$/.test(value) || BigInt(value) > JAVASCRIPT_SAFE_INTEGER_MAX) {
    throw new SaveSchemaError(path, "必须是 JavaScript 安全整数范围内的规范非负十进制键")
  }
}

export function accountingAmount(value: unknown, path: string): string {
  const parsed = string(value, path)
  if (!ACCOUNTING_AMOUNT.test(parsed)) throw new SaveSchemaError(path, "必须是两位小数的十进制会计金额")
  return parsed
}

export function civilDate(value: unknown, path: string): string {
  const parsed = string(value, path)
  if (!ISO_CIVIL_DATE.test(parsed)) throw new SaveSchemaError(path, "必须是 YYYY-MM-DD 日期字符串")
  const [yearText, monthText, dayText] = parsed.split("-")
  if (yearText === undefined || monthText === undefined || dayText === undefined) throw new SaveSchemaError(path, "必须是有效公历日期")
  const year = Number(yearText)
  const month = Number(monthText)
  const day = Number(dayText)
  const maximum = new Date(Date.UTC(year, month, 0)).getUTCDate()
  if (month < 1 || month > 12 || day < 1 || day > maximum) throw new SaveSchemaError(path, "必须是有效公历日期")
  return parsed
}

export function nullable<T>(value: unknown, path: string, parser: (nested: unknown, nestedPath: string) => T): T | null {
  return value === null ? null : parser(value, path)
}

export function oneOf<T extends string>(value: unknown, path: string, variants: readonly T[]): T {
  const parsed = string(value, path)
  const match = variants.find((variant) => variant === parsed)
  if (match === undefined) throw new SaveSchemaError(path, "包含无效枚举值")
  return match
}

export function map<T>(value: unknown, path: string, parseKey: (key: string, keyPath: string) => void, parseValue: (nested: unknown, nestedPath: string) => T): Record<string, T> {
  const parsed = record(value, path)
  const result: [string, T][] = []
  for (const [key, nested] of Object.entries(parsed)) {
    parseKey(key, `${path}.${key}`)
    result.push([key, parseValue(nested, `${path}.${key}`)])
  }
  return Object.fromEntries(result)
}
