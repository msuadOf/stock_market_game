import type { MonthlyReportSchedule } from "../../types/generated/MonthlyReportSchedule"
import type { MonthlyReportDelay } from "../../types/generated/MonthlyReportDelay"
import { SaveSchemaError, exact, integer, oneOf, record } from "./primitives.ts"

function bounded(value: unknown, path: string, min: number, max: number): number {
  const result = integer(value, path, min)
  if (result > max) throw new SaveSchemaError(path, `必须不大于 ${max}`)
  return result
}

function delay(value: unknown, path: string): MonthlyReportDelay {
  if (value === "None") return value
  const parsed = record(value, path)
  exact(parsed, ["Uniform"], path)
  const uniform = record(parsed.Uniform, `${path}.Uniform`)
  exact(uniform, ["max_days"], `${path}.Uniform`)
  return { Uniform: { max_days: bounded(uniform.max_days, `${path}.Uniform.max_days`, 1, 31) } }
}

export function parseMonthlyReportSchedule(value: unknown, path: string): MonthlyReportSchedule {
  const parsed = record(value, path)
  if ("Preset" in parsed) {
    exact(parsed, ["Preset"], path)
    const preset = record(parsed.Preset, `${path}.Preset`)
    exact(preset, ["preset", "delay"], `${path}.Preset`)
    return { Preset: { preset: oneOf(preset.preset, `${path}.Preset.preset`, ["FirstDayEvening", "TenthDayEvening"] as const), delay: delay(preset.delay, `${path}.Preset.delay`) } }
  }
  exact(parsed, ["Custom"], path)
  const custom = record(parsed.Custom, `${path}.Custom`)
  exact(custom, ["day", "second_of_day", "delay"], `${path}.Custom`)
  return { Custom: { day: bounded(custom.day, `${path}.Custom.day`, 1, 28), second_of_day: bounded(custom.second_of_day, `${path}.Custom.second_of_day`, 0, 86399), delay: delay(custom.delay, `${path}.Custom.delay`) } }
}

