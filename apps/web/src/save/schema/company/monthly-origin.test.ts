import assert from "node:assert/strict"
import test from "node:test"
import { parsePublicLibrary } from "./reports.ts"
import { SaveSchemaError } from "../primitives.ts"

const originPath = "public_library.reports[0].origin"
// 手构 DTO 只验证 Web parser 边界，不是 Rust producer 的真实月报存档。
const unavailable = { Unavailable: { reason: "MissingNonRecurringIncomeFacts" } }
const columns = { operating: [], operating_subtotal: "0.00", investing: [], investing_subtotal: "0.00", financing: [], financing_subtotal: "0.00", discontinued: [], discontinued_subtotal: "0.00", income_tax: "0.00", net_income: "0.00" }
const report = {
  id: 0, company: 0, source: "SimpleGenerated", policy: { chart_version: 2 },
  approved_at: { date: "2030-02-01", second_of_day: 0 }, published_at: { date: "2030-02-01", second_of_day: 64800 }, supersedes: null,
  reports: {
    scope: { Standalone: 0 }, period: "2030-01", kind: "Monthly", window: ["2030-01", "2030-01"], version: { sequence: 1, supersedes: null, kind: "Original" },
    balance_sheet: { asset_lines: [], total_assets: "0.00", liability_lines: [], total_liabilities: "0.00", equity_lines: [], total_equity: "0.00", equity_to_parent: "0.00", liabilities_and_equity: "0.00", closing_cash: "0.00", prior_year_end: { Unavailable: { reason: "NoPriorYearHistory" } } },
    income: { report_period: columns, report_period_net_income_to_parent: null, quarter: columns, cumulative: columns, prior_year: { Unavailable: { reason: "NoPriorYearHistory" } }, minority_net_income: null, net_income_to_parent: null },
    roe: { basis: { Unsupported: { reason: "MissingNonRecurringIncomeFacts" } }, ordinary_roe: unavailable, adjusted_roe: unavailable, weighted_average_parent_equity_cents: unavailable, non_recurring: unavailable, equity_events: { attributable_to_parent: { Items: [] }, minority_interest: { Unavailable: { reason: "StandaloneHasNoMinorityInterest" } } } },
    cash_flow: { operating: "0.00", investing: "0.00", financing: "0.00", net_change: "0.00", opening_cash: "0.00", closing_cash: "0.00", indirect: [] },
    equity: { opening_parent: "0.00", net_income: "0.00", other_comprehensive: "0.00", capital_contributions: "0.00", distributions: "0.00", closing_parent: "0.00", opening_minority: null, minority_net_income: null, closing_minority: null }, notes: { items: [], consolidation_split_items: [] },
  },
}
const preset = { Preset: { preset: "FirstDayEvening", delay: "None" } }
const monthly = (schedule: unknown = preset, delay_days: unknown = 0, seeded: unknown = false) => ({ MonthlyDisclosure: { schedule, delay_days, seeded } })
const library = (origin: unknown) => ({ next_seq: 1, reports: [{ ...report, origin }], announcements: [] })
function rejects(origin: unknown, expectedPath: string): void {
  assert.throws(() => parsePublicLibrary(library(origin)), (error) => error instanceof SaveSchemaError && error.path === expectedPath)
}

for (const seeded of [true, false]) {
  test(`MonthlyDisclosure 保留 Preset/Custom、None/Uniform 与 seeded=${seeded}`, { timeout: 10000 }, () => {
    for (const delay of ["None", { Uniform: { max_days: 1 } }, { Uniform: { max_days: 31 } }] as const) {
      for (const schedule of [
        { Preset: { preset: "FirstDayEvening", delay } }, { Preset: { preset: "TenthDayEvening", delay } },
        { Custom: { day: 1, second_of_day: 0, delay } }, { Custom: { day: 28, second_of_day: 86399, delay } },
      ]) {
        const fixture = library(monthly(schedule, delay === "None" ? 0 : delay.Uniform.max_days, seeded))
        assert.deepEqual(parsePublicLibrary(fixture), fixture)
      }
    }
  })
}

test("MonthlyDisclosure 的 delay_days 仅做 u8 shape 校验，配置一致性留给 Rust", { timeout: 10000 }, () => {
  for (const delay_days of [0, 1, 31, 255]) {
    const fixture = library(monthly(preset, delay_days))
    assert.deepEqual(parsePublicLibrary(fixture), fixture)
  }
  for (const value of [-1, 256, 1.5, "1", null, NaN, Infinity]) rejects(monthly(preset, value), `${originPath}.MonthlyDisclosure.delay_days`)
  rejects({ MonthlyDisclosure: { schedule: preset, delay_days: undefined, seeded: false } }, `${originPath}.MonthlyDisclosure.delay_days`)
  rejects({ MonthlyDisclosure: { schedule: preset, delay_days: 0, seeded: undefined } }, `${originPath}.MonthlyDisclosure.seeded`)
  for (const seeded of [0, 1, "false", null]) rejects(monthly(preset, 0, seeded), `${originPath}.MonthlyDisclosure.seeded`)
})

test("MonthlyDisclosure 及排期、延迟对象拒绝缺失和未知键", { timeout: 10000 }, () => {
  const origin = monthly({ Custom: { day: 1, second_of_day: 0, delay: { Uniform: { max_days: 31 } } } })
  function visit(value: Record<string, unknown>, path: string, root: Record<string, unknown>): void {
    for (const key of Object.keys(value)) {
      const missing = structuredClone(root)
      let target = missing
      for (const part of path.split(".").slice(1)) target = target[part] as Record<string, unknown>
      delete target[key]
      rejects(missing, path === "root" ? originPath : `${originPath}${path.slice(4)}.${key}`)
      const nested = value[key]
      if (typeof nested === "object" && nested !== null) visit(nested as Record<string, unknown>, `${path}.${key}`, root)
    }
    const extra = structuredClone(root)
    let target = extra
    for (const part of path.split(".").slice(1)) target = target[part] as Record<string, unknown>
    target.extra = true
    // 顶层多 tag 由 variant 统一在 origin 路径拒绝。
    rejects(extra, path === "root" ? originPath : `${originPath}${path.slice(4)}.extra`)
  }
  visit(origin, "root", origin)
  for (const schedule of [preset, { Preset: { preset: "TenthDayEvening", delay: { Uniform: { max_days: 1 } } } }]) {
    const body = schedule.Preset
    for (const key of ["preset", "delay"]) { const invalid: Record<string, unknown> = { ...body }; delete invalid[key]; rejects(monthly({ Preset: invalid }), `${originPath}.MonthlyDisclosure.schedule.Preset.${key}`) }
    rejects(monthly({ Preset: { ...body, extra: true } }), `${originPath}.MonthlyDisclosure.schedule.Preset.extra`)
  }
})

test("MonthlyDisclosure 拒绝无效排期与延迟变体、数值边界和类型", { timeout: 10000 }, () => {
  const schedulePath = `${originPath}.MonthlyDisclosure.schedule`
  for (const schedule of [null, "Monthly", {}, { Monthly: { schedule: preset } }, { Preset: preset.Preset, Custom: {} }]) assert.throws(() => parsePublicLibrary(library(monthly(schedule))), SaveSchemaError)
  rejects(monthly({ Preset: { preset: "Unknown", delay: "None" } }), `${schedulePath}.Preset.preset`)
  for (const day of [0, 29, -1, 256, 1.5, "1", null]) rejects(monthly({ Custom: { day, second_of_day: 0, delay: "None" } }), `${schedulePath}.Custom.day`)
  for (const second_of_day of [-1, 86400, 0.5, "0", null]) rejects(monthly({ Custom: { day: 1, second_of_day, delay: "None" } }), `${schedulePath}.Custom.second_of_day`)
  for (const max_days of [0, 32, 256, -1, 1.5, "1", null]) rejects(monthly({ Preset: { preset: "FirstDayEvening", delay: { Uniform: { max_days } } } }), `${schedulePath}.Preset.delay.Uniform.max_days`)
  for (const delay of [null, "Unknown", {}, { None: null }, { Uniform: { max_days: 1 }, extra: true }]) assert.throws(() => parsePublicLibrary(library(monthly({ Preset: { preset: "FirstDayEvening", delay } }))), SaveSchemaError)
})

test("旧 PublicationOrigin 三分支保持字段与变体校验", { timeout: 10000 }, () => {
  for (const tag of ["SeededPrehistory", "ScheduledDisclosure"]) {
    const fields = { fiscal_year: 2030, kind: "Q1", offset_days: 0 }
    assert.deepEqual(parsePublicLibrary(library({ [tag]: fields })), library({ [tag]: fields }))
    rejects({ [tag]: { ...fields, extra: true } }, `${originPath}.${tag}.extra`)
    rejects({ [tag]: { ...fields, kind: "Monthly" } }, `${originPath}.${tag}.kind`)
    rejects({ [tag]: { ...fields, offset_days: -1 } }, `${originPath}.${tag}.offset_days`)
    for (const key of Object.keys(fields)) { const invalid: Record<string, unknown> = { ...fields }; delete invalid[key]; rejects({ [tag]: invalid }, `${originPath}.${tag}.${key}`) }
  }
  assert.deepEqual(parsePublicLibrary(library("Correction")), library("Correction"))
  for (const invalid of ["MonthlyDisclosure", { Correction: {} }, { Unknown: {} }, {}, { ...monthly(), ScheduledDisclosure: {} }]) rejects(invalid, originPath)
})
