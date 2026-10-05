import { SaveSchemaError, exact, integer, oneOf, record } from "./primitives.ts"
import type { MonthlyReportDelay } from "../../types/generated/MonthlyReportDelay"
import type { MonthlyReportSchedule } from "../../types/generated/MonthlyReportSchedule"
import type { ReportFrequency } from "../../types/generated/ReportFrequency"

function variant(value: unknown, path: string): readonly [string, unknown] {
  const item = record(value, path)
  const entries = Object.entries(item)
  if (entries.length !== 1) throw new SaveSchemaError(path, "必须为单一排期变体")
  return entries[0]
}

export function parseMonthlyReportDelay(value: unknown, path: string): MonthlyReportDelay {
  if (value === "None") return value
  const [tag, body] = variant(value, path)
  if (tag !== "Uniform") throw new SaveSchemaError(path, "延迟必须为None或Uniform")
  const item = record(body, `${path}.Uniform`)
  exact(item, ["max_days"], `${path}.Uniform`)
  const maximum = integer(item.max_days, `${path}.Uniform.max_days`, 1)
  if (maximum > 31) throw new SaveSchemaError(`${path}.Uniform.max_days`, "均匀随机延迟最多31天")
  return { Uniform: { max_days: maximum } }
}

export function parseMonthlyReportSchedule(value: unknown, path: string): MonthlyReportSchedule {
  const [tag, body] = variant(value, path)
  const item = record(body, `${path}.${tag}`)
  if (tag === "Preset") {
    exact(item, ["preset", "delay"], `${path}.Preset`)
    return { Preset: { preset: oneOf(item.preset, `${path}.Preset.preset`, ["FirstDayEvening", "TenthDayEvening"] as const), delay: parseMonthlyReportDelay(item.delay, `${path}.Preset.delay`) } }
  }
  if (tag === "Custom") {
    exact(item, ["day", "second_of_day", "delay"], `${path}.Custom`)
    const day = integer(item.day, `${path}.Custom.day`, 1)
    const second = integer(item.second_of_day, `${path}.Custom.second_of_day`, 0)
    if (day > 28 || second >= 86400) throw new SaveSchemaError(`${path}.Custom`, "次月日期须为1–28日，时间须在0–86399秒，不自动截断")
    return { Custom: { day, second_of_day: second, delay: parseMonthlyReportDelay(item.delay, `${path}.Custom.delay`) } }
  }
  throw new SaveSchemaError(path, "月报排期须为Preset或Custom")
}

export function parseReportFrequency(value: unknown, path = "report_frequency"): ReportFrequency {
  if (value === "Quarterly") return value
  if (value === "Monthly") throw new SaveSchemaError(path, "请明确选择月报排期，不能只使用Monthly或静默补10日")
  const item = record(value, path)
  exact(item, ["Monthly"], path)
  const monthly = record(item.Monthly, `${path}.Monthly`)
  exact(monthly, ["schedule"], `${path}.Monthly`)
  return { Monthly: { schedule: parseMonthlyReportSchedule(monthly.schedule, `${path}.Monthly.schedule`) } }
}
