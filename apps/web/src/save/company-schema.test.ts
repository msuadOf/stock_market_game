import assert from "node:assert/strict"
import test from "node:test"
import { representativeCompanyOperationsFixture } from "./company-slice-test-fixture.ts"
import { parseClosingRegistry, parseCompanyOperations, parseDisclosureDispatch, parseOperationsWiring, parsePublicLibrary } from "./schema/company/index.ts"

const instant = { date: "2030-04-20", second_of_day: 64800 }
const balance = { asset_lines: [["CashFunds", "100.00"]], total_assets: "100.00", liability_lines: [], total_liabilities: "0.00", equity_lines: [["PaidInCapital", "100.00"]], total_equity: "100.00", equity_to_parent: "100.00", liabilities_and_equity: "100.00", closing_cash: "100.00", prior_year_end: { Unavailable: { reason: "NoPriorYearHistory" } } }
const columns = { operating: [], operating_subtotal: "0.00", investing: [], investing_subtotal: "0.00", financing: [], financing_subtotal: "0.00", discontinued: [], discontinued_subtotal: "0.00", income_tax: "0.00", net_income: "0.00" }
const reportSet = { scope: { Standalone: "C-1" }, period: "2030-03", kind: "Quarter", window: ["2030-01", "2030-03"], version: { sequence: 1, supersedes: null, kind: "Original" }, balance_sheet: balance, income: { quarter: columns, cumulative: columns, prior_year: { Unavailable: { reason: "NoPriorYearHistory" } }, minority_net_income: null, net_income_to_parent: null }, cash_flow: { operating: "0.00", investing: "0.00", financing: "0.00", net_change: "0.00", opening_cash: "0.00", closing_cash: "0.00", indirect: [] }, equity: { opening_parent: "0.00", net_income: "0.00", other_comprehensive: "0.00", capital_contributions: "0.00", distributions: "0.00", closing_parent: "0.00", opening_minority: null, minority_net_income: null, closing_minority: null }, notes: { items: [], consolidation_split_items: [] } }
const publicLibrary = { next_seq: 2, reports: [{ id: 0, company: "C-1", source: "SimulationAccounting", policy: { chart_version: 2 }, approved_at: { date: "2030-04-20", second_of_day: 28800 }, published_at: instant, origin: { ScheduledDisclosure: { fiscal_year: 2030, kind: "Q1", offset_days: 0 } }, supersedes: null, reports: reportSet }], announcements: [{ id: 1, company: "C-1", occurred_on: "2030-04-20", published_at: instant, event: { kind: "ContractWon", amplitude_bp: 100, starts_on: "2030-04-20", expires_on: "2030-04-25" } }] }
const operations = representativeCompanyOperationsFixture() as any

test("额外月报MonthlyDisclosure来源完整保存排期及实抽延迟，不冒充法定ScheduledDisclosure", { timeout: 10000 }, () => {
  const origin = { MonthlyDisclosure: { schedule: { Custom: { day: 12, second_of_day: 54001, delay: { Uniform: { max_days: 2 } } } }, delay_days: 1, seeded: false } }
  const report = { ...publicLibrary.reports[0], origin, reports: { ...reportSet, kind: "Monthly", window: ["2030-03", "2030-03"] }, published_at: { date: "2030-04-13", second_of_day: 54001 }, approved_at: { date: "2030-04-13", second_of_day: 54001 } }
  const library = { ...publicLibrary, reports: [report] }
  assert.deepEqual(parsePublicLibrary(library), library)
  for (const changed of [{ ...origin.MonthlyDisclosure, delay_days: 3 }, { ...origin.MonthlyDisclosure, seeded: "false" }, { ...origin.MonthlyDisclosure, fiscal_year: 2030 }]) assert.throws(() => parsePublicLibrary({ ...library, reports: [{ ...report, origin: { MonthlyDisclosure: changed } }] }), /origin/)
  assert.throws(() => parsePublicLibrary({ ...library, reports: [{ ...report, reports: reportSet }] }), /Monthly/)
  assert.throws(() => parsePublicLibrary({ ...library, reports: [{ ...report, published_at: { ...report.published_at, second_of_day: 54000 } }] }), /published_at/)
  assert.throws(() => parsePublicLibrary({ ...library, reports: [{ ...report, published_at: { ...report.published_at, date: "2030-04-12" } }] }), /published_at/)
  for (const [targetPeriod, day, delayDays, date] of [["2027-12", 28, 31, "2028-02-28"], ["2028-01", 28, 1, "2028-02-29"], ["2029-01", 28, 1, "2029-03-01"]] as const) {
    const monthlyOrigin = { MonthlyDisclosure: { schedule: { Custom: { day, second_of_day: 0, delay: { Uniform: { max_days: 31 } } } }, delay_days: delayDays, seeded: true } }
    const current = { ...report, origin: monthlyOrigin, reports: { ...report.reports, period: targetPeriod, window: [targetPeriod, targetPeriod] }, approved_at: { date, second_of_day: 0 }, published_at: { date, second_of_day: 0 } }
    assert.deepEqual(parsePublicLibrary({ ...library, reports: [current] }).reports[0].origin, monthlyOrigin)
  }
  const noDelay = { MonthlyDisclosure: { schedule: { Preset: { preset: "FirstDayEvening", delay: "None" } }, delay_days: 1, seeded: false } }
  assert.throws(() => parsePublicLibrary({ ...library, reports: [{ ...report, origin: noDelay }] }), /实际延迟/)
  const legacy = { ScheduledDisclosure: { fiscal_year: 2030, kind: { Monthly: { month: 3 } }, offset_days: 0 } }
  assert.throws(() => parsePublicLibrary({ ...library, reports: [{ ...report, origin: legacy }] }), /显式MonthlyDisclosure/)
});
const closingRegistry = { versions: [[{ Standalone: "C-1" }, "2030-03", "Quarter", [reportSet]]], restatements: [[{ Standalone: "C-1" }, [["18446744073709551615", "2030-03"]]]] }

test("company schema parsers preserve populated company slices", () => {
  assert.deepEqual(parseCompanyOperations(operations), operations)
  assert.deepEqual(parsePublicLibrary(publicLibrary), publicLibrary)
  assert.deepEqual(parseClosingRegistry(closingRegistry), closingRegistry)
  assert.deepEqual(parseOperationsWiring({ mirrored: [42] }), { mirrored: [42] })
  assert.deepEqual(parseDisclosureDispatch({ published_through: instant, announced_through: "2030-04-20" }), { published_through: instant, announced_through: "2030-04-20" })
})

test("company schema parsers reject malformed exact subdomains with paths", () => {
  assert.throws(() => parseCompanyOperations({ ...operations, market_rng: { state: 42 } }), /company_operations\.market_rng\.state/)
  assert.throws(() => parsePublicLibrary({ ...publicLibrary, reports: [{ ...publicLibrary.reports[0], reports: { ...reportSet, balance_sheet: { ...balance, total_assets: "1.234" } } }] }), /public_library\.reports\[0\]\.reports\.balance_sheet\.total_assets/)
  assert.throws(() => parseClosingRegistry({ versions: [[{ BadScope: "C-1" }, "2030-03", "Quarter", []]], restatements: [] }), /closing_registry\.versions\[0\]\[0\]/)
  assert.throws(() => parseOperationsWiring({ mirrored: ["42"] }), /ops_wiring\.mirrored\[0\]/)
  assert.throws(() => parseDisclosureDispatch({ published_through: null }), /disclosures\.announced_through/)
})
