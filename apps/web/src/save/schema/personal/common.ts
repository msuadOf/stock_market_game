import { SaveSchemaError, decimal, exact, integer, record, safeIntegerKey, string } from "../primitives.ts"

export type StringMap<T> = { readonly [key: string]: T }

const ACCOUNT_ID_MAX = 9_007_199_254_740_991n
const U32_MAX = 4_294_967_295n
const U16_MAX = 65_535n
const U64_MAX = 18_446_744_073_709_551_615n

export function accountKey(key: string, path: string): void {
  boundedDecimalKey(key, path, ACCOUNT_ID_MAX)
}

export function planKey(key: string, path: string): void {
  boundedDecimalKey(key, path, ACCOUNT_ID_MAX)
}

export function stockKey(key: string, path: string): void {
  if (!/^\d{6}$/.test(key)) throw new SaveSchemaError(path, "必须是六码股票代码")
}

export function companyKey(key: string, path: string): void {
  if (key.length === 0) throw new SaveSchemaError(path, "必须是非空公司标识")
}

export function publicationId(value: unknown, path: string): number {
  return u32(value, path)
}

export function orderId(value: unknown, path: string): number {
  return integer(value, path, 1)
}

export function marketMinute(value: unknown, path: string): string {
  const parsed = decimal(value, path)
  if (BigInt(parsed) > U64_MAX) throw new SaveSchemaError(path, "超出 u64 范围")
  return parsed
}

export function tradingDay(value: unknown, path: string): number {
  return integer(value, path, 0)
}

export function money(value: unknown, path: string): number {
  return integer(value, path)
}

export function nullable<T>(value: unknown, path: string, parser: (nested: unknown, nestedPath: string) => T): T | null {
  return value === null ? null : parser(value, path)
}

export function tagged(value: unknown, path: string): readonly [string, ReturnType<typeof record>] {
  const parsed = record(value, path)
  const names = Object.keys(parsed)
  if (names.length !== 1) throw new SaveSchemaError(path, "必须恰有一个枚举标签")
  const name = names[0]
  if (name === undefined) throw new SaveSchemaError(path, "必须恰有一个枚举标签")
  return [name, record(parsed[name], `${path}.${name}`)]
}

export function externalTag(value: unknown, path: string): readonly [string, unknown] {
  const parsed = record(value, path)
  const names = Object.keys(parsed)
  if (names.length !== 1) throw new SaveSchemaError(path, "必须恰有一个枚举标签")
  const name = names[0]
  if (name === undefined) throw new SaveSchemaError(path, "必须恰有一个枚举标签")
  return [name, parsed[name]]
}

export function unitOrTagged(value: unknown, path: string): string | readonly [string, ReturnType<typeof record>] {
  return typeof value === "string" ? string(value, path) : tagged(value, path)
}

export function exactString(value: unknown, path: string): string {
  return string(value, path)
}

function boundedDecimalKey(key: string, path: string, maximum: bigint): void {
  safeIntegerKey(key, path)
  if (BigInt(key) > maximum) throw new SaveSchemaError(path, "超出 JavaScript 安全整数范围")
}

export function exactObject(value: unknown, keys: readonly string[], path: string): ReturnType<typeof record> {
  const parsed = record(value, path)
  exact(parsed, keys, path)
  return parsed
}

export function u32(value: unknown, path: string): number {
  const parsed = integer(value, path, 0)
  if (BigInt(parsed) > U32_MAX) throw new SaveSchemaError(path, "超出 u32 范围")
  return parsed
}

export function u16(value: unknown, path: string): number {
  const parsed = integer(value, path, 0)
  if (BigInt(parsed) > U16_MAX) throw new SaveSchemaError(path, "超出 u16 范围")
  return parsed
}

export function i32(value: unknown, path: string): number {
  const parsed = integer(value, path)
  if (parsed < -2_147_483_648 || parsed > 2_147_483_647) throw new SaveSchemaError(path, "超出 i32 范围")
  return parsed
}
