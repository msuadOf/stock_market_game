import { SaveSchemaError, civilDate, exact, integer, record, string } from "../primitives.ts"

export type U64 = string
export type I128 = string
export type DecimalAmount = string
export type CivilDateValue = string
export type CivilInstantValue = { readonly date: CivilDateValue; readonly second_of_day: number }

const U64_DECIMAL = /^\d+$/
const I128_DECIMAL = /^-?\d+$/
const AMOUNT = /^-?\d+\.\d{2}$/
const PERIOD = /^\d{4}-(0[1-9]|1[0-2])$/
const U64_MAX = 18_446_744_073_709_551_615n
const I128_MIN = -(1n << 127n)
const I128_MAX = (1n << 127n) - 1n

export function u64(value: unknown, path: string): U64 {
  const parsed = string(value, path)
  if (!U64_DECIMAL.test(parsed) || BigInt(parsed) > U64_MAX) throw new SaveSchemaError(path, "必须是无损 u64 十进制字符串")
  return parsed
}

export function i128(value: unknown, path: string): I128 {
  const parsed = string(value, path)
  if (!I128_DECIMAL.test(parsed)) throw new SaveSchemaError(path, "必须是无损 i128 十进制字符串")
  const numeric = BigInt(parsed)
  if (numeric < I128_MIN || numeric > I128_MAX) throw new SaveSchemaError(path, "超出 i128 范围")
  return parsed
}

export function amount(value: unknown, path: string): DecimalAmount {
  const parsed = string(value, path)
  if (!AMOUNT.test(parsed)) throw new SaveSchemaError(path, "必须是两位小数的会计金额")
  return parsed
}

export function period(value: unknown, path: string): string {
  const parsed = string(value, path)
  if (!PERIOD.test(parsed)) throw new SaveSchemaError(path, "必须是 YYYY-MM 会计期间")
  return parsed
}

export function instant(value: unknown, path: string): CivilInstantValue {
  const parsed = record(value, path)
  exact(parsed, ["date", "second_of_day"], path)
  const secondOfDay = integer(parsed.second_of_day, `${path}.second_of_day`, 0)
  if (secondOfDay >= 86_400) throw new SaveSchemaError(`${path}.second_of_day`, "必须小于 86400")
  return { date: civilDate(parsed.date, `${path}.date`), second_of_day: secondOfDay }
}

export function nullableDate(value: unknown, path: string): CivilDateValue | null {
  return value === null ? null : civilDate(value, path)
}
