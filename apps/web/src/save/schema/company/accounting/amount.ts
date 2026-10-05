import { SaveSchemaError } from "../../primitives.ts"

export function accountingMinorUnits(value: string, path = "AccountingAmount"): bigint {
  if (!/^\s*[+-]?\d+(?:\.\d{1,2})?\s*$/.test(value)) throw new SaveSchemaError(path, "必须为会计元字符串")
  const text = value.trim()
  const [whole, fraction = ""] = text.replace(/^[+-]/, "").split(".")
  const magnitude = BigInt(whole) * 100n + BigInt(fraction.padEnd(2, "0"))
  if (magnitude > (1n << 127n) - 1n) throw new SaveSchemaError(path, "会计元文本幅值超出Engine i128解码范围")
  const parsed = text.startsWith("-") ? -magnitude : magnitude
  return parsed
}
