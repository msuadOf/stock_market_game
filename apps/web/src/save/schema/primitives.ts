export type JsonRecord = Record<string, unknown>

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
  const missing = keys.filter((key) => !(key in value))
  const extra = Object.keys(value).filter((key) => !expected.has(key))
  if (missing.length > 0) throw new SaveSchemaError(`${path}.${missing[0]}`, "是必填字段")
  if (extra.length > 0) throw new SaveSchemaError(`${path}.${extra[0]}`, "不是允许字段")
}

export function array(value: unknown, path: string): readonly unknown[] {
  if (!Array.isArray(value)) throw new SaveSchemaError(path, "必须是数组")
  return value
}

export function string(value: unknown, path: string): string {
  if (typeof value !== "string") throw new SaveSchemaError(path, "必须是字符串")
  return value
}

export function boolean(value: unknown, path: string): boolean {
  if (typeof value !== "boolean") throw new SaveSchemaError(path, "必须是布尔值")
  return value
}

export function integer(value: unknown, path: string, minimum?: number): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || (minimum !== undefined && value < minimum)) {
    throw new SaveSchemaError(path, minimum === undefined ? "必须是安全整数" : `必须是不小于 ${minimum} 的安全整数`)
  }
  return value
}

export function finite(value: unknown, path: string): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new SaveSchemaError(path, "必须是有限数值")
  }
  return value
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
  const result: Record<string, T> = {}
  for (const [key, nested] of Object.entries(parsed)) {
    parseKey(key, `${path}.${key}`)
    result[key] = parseValue(nested, `${path}.${key}`)
  }
  return result
}
