import { SaveSchemaError, array, exact, record, string } from "../../primitives.ts"

const PERIOD = /^\d{4}-(0[1-9]|1[0-2])$/

export type AccountingPeriod = string
export type PeriodStatus = "Open" | "Closed"
export type ClosedPeriods = readonly AccountingPeriod[]
export type PeriodStates = { readonly closed: ClosedPeriods }

export function parseAccountingPeriod(value: unknown, path: string): AccountingPeriod {
  const parsed = string(value, path)
  if (!PERIOD.test(parsed)) throw new SaveSchemaError(path, "必须是 YYYY-MM 会计期间")
  const year = Number(parsed.slice(0, 4))
  if (year < 1900 || year > 2199) throw new SaveSchemaError(path, "年份必须在 1900 至 2199 之间")
  return parsed
}

export function parsePeriodStates(value: unknown, path: string): PeriodStates {
  const parsed = record(value, path)
  exact(parsed, ["closed"], path)
  return { closed: array(parsed.closed, `${path}.closed`).map((period, index) => parseAccountingPeriod(period, `${path}.closed[${index}]`)) }
}
