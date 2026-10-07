import { SaveSchemaError, array, boolean, civilDate, exact, integer, money, oneOf, record, string } from "../primitives.ts"
import { parseMonthlyReportSchedule } from "../report-frequency.ts"
import type { MonthlyReportSchedule } from "../../../types/generated/MonthlyReportSchedule"
import { parseShockKind, type ActiveShock } from "./policies/shock.ts"
import { amount, instant, period, type CivilInstantValue, type DecimalAmount } from "./value.ts"
import { parseIssuerRepurchasePlanValue, parseRightsOfferingEventPlanValue } from "../corporate-actions.ts"
import type { CashDividendPlan } from "../corporate-actions.ts"

const reportKinds = ["Monthly", "Quarter", "HalfYear", "Annual"] as const
const scheduledKinds = ["Annual", "Q1", "HalfYear", "Q3"] as const
const assetLines = ["CashFunds", "Receivables", "SimpleDividendSettlementAdjustment", "InsuranceReceivables", "Inventory", "DevelopmentInventory", "FixedAssets", "LoansAndAdvances", "DeferredTaxAssets", "CurrentTaxAssets"] as const
const liabilityLines = ["ShortTermBorrowings", "AccountsPayable", "ContractLiabilities", "TaxesPayable", "InterestPayable", "CustomerDeposits", "LongTermBorrowings", "InsuranceContractLiabilities", "DeferredTaxLiabilities"] as const
const equityLines = ["PaidInCapital", "StatutoryReserve", "RetainedEarnings", "MinorityEquity"] as const
const balanceLines = [...assetLines, ...liabilityLines, ...equityLines] as const
const incomeLines = ["OperatingRevenue", "OperatingCost", "SellingExpense", "AdministrativeExpense", "ResearchExpense", "ImpairmentLoss", "InterestIncome", "InterestExpense", "NetInterestIncome", "FeeAndCommissionIncome", "InsuranceRevenue", "InsuranceServiceExpense", "InsuranceServiceResult", "InsuranceFinanceExpense", "FinanceExpense", "IncomeTaxExpense"] as const

export type Scope = { readonly Standalone: string } | { readonly Consolidated: string }
export type Comparative<T> = { readonly Available: T } | { readonly Unavailable: { readonly reason: "NoPriorYearHistory" } }
export type LineAmount = readonly [(typeof balanceLines)[number] | (typeof incomeLines)[number], DecimalAmount]
export type IncomeColumns = { readonly operating: readonly LineAmount[]; readonly operating_subtotal: DecimalAmount; readonly investing: readonly LineAmount[]; readonly investing_subtotal: DecimalAmount; readonly financing: readonly LineAmount[]; readonly financing_subtotal: DecimalAmount; readonly discontinued: readonly LineAmount[]; readonly discontinued_subtotal: DecimalAmount; readonly income_tax: DecimalAmount; readonly net_income: DecimalAmount }
export type ReportRoeUnavailable = "NonPositiveAverageEquity" | "MissingNonRecurringIncomeFacts" | "IncompleteEquityEventHistory" | "SameControlCombination" | "ComparativePeriodSpecialTreatment" | "UnclassifiedEquityEvent" | "ConsolidatedAttributionFactsUnavailable"
export type ReportRoeValue = { readonly Available: { readonly numerator: string; readonly denominator: string } } | { readonly Unavailable: { readonly reason: ReportRoeUnavailable } }
export type ReportRoe = { readonly basis: "AttributableToOrdinaryShareholders" | { readonly Unsupported: { readonly reason: ReportRoeUnavailable } }; readonly ordinary_roe: ReportRoeValue; readonly adjusted_roe: ReportRoeValue; readonly weighted_average_parent_equity_cents: ReportRoeValue }
export type BalanceSheet = { readonly asset_lines: readonly LineAmount[]; readonly total_assets: DecimalAmount; readonly liability_lines: readonly LineAmount[]; readonly total_liabilities: DecimalAmount; readonly equity_lines: readonly LineAmount[]; readonly total_equity: DecimalAmount; readonly equity_to_parent: DecimalAmount; readonly liabilities_and_equity: DecimalAmount; readonly closing_cash: DecimalAmount; readonly prior_year_end: Comparative<readonly LineAmount[]> }
export type CashFlow = { readonly operating: DecimalAmount; readonly investing: DecimalAmount; readonly financing: DecimalAmount; readonly net_change: DecimalAmount; readonly opening_cash: DecimalAmount; readonly closing_cash: DecimalAmount; readonly indirect: readonly { readonly label: string; readonly amount: DecimalAmount }[] }
export type Equity = { readonly opening_parent: DecimalAmount; readonly net_income: DecimalAmount; readonly other_comprehensive: DecimalAmount; readonly capital_contributions: DecimalAmount; readonly distributions: DecimalAmount; readonly closing_parent: DecimalAmount; readonly opening_minority: DecimalAmount | null; readonly minority_net_income: DecimalAmount | null; readonly closing_minority: DecimalAmount | null }
export type NoteTarget = { readonly BalanceSheet: (typeof balanceLines)[number] } | { readonly Income: (typeof incomeLines)[number] }
export type Notes = { readonly items: readonly NoteItem[]; readonly consolidation_split_items: readonly NoteItem[] }
export type NoteItem = { readonly code: string; readonly name: string; readonly target: NoteTarget; readonly opening: DecimalAmount; readonly movement: DecimalAmount; readonly ytd_movement: DecimalAmount; readonly closing: DecimalAmount }
export type ReportVersion = { readonly sequence: number; readonly supersedes: number | null; readonly kind: "Original" | { readonly Correction: { readonly reason: string } } }
export type ReportSet = { readonly scope: Scope; readonly period: string; readonly kind: (typeof reportKinds)[number]; readonly window: readonly [string, string]; readonly version: ReportVersion; readonly balance_sheet: BalanceSheet; readonly income: { readonly report_period: IncomeColumns; readonly report_period_net_income_to_parent: DecimalAmount | null; readonly quarter: IncomeColumns; readonly cumulative: IncomeColumns; readonly prior_year: Comparative<IncomeColumns>; readonly minority_net_income: DecimalAmount | null; readonly net_income_to_parent: DecimalAmount | null }; readonly roe: ReportRoe; readonly cash_flow: CashFlow; readonly equity: Equity; readonly notes: Notes }
type ScheduledKind = (typeof scheduledKinds)[number] | { readonly Monthly: { readonly month: number } }
export type PublicationOrigin = { readonly SeededPrehistory: { readonly fiscal_year: number; readonly kind: ScheduledKind; readonly offset_days: number } } | { readonly ScheduledDisclosure: { readonly fiscal_year: number; readonly kind: ScheduledKind; readonly offset_days: number } } | { readonly MonthlyDisclosure: { readonly schedule: MonthlyReportSchedule; readonly delay_days: number; readonly seeded: boolean } } | "Correction"
export type PublishedReport = { readonly source: "SimpleGenerated" | "SimulationAccounting"; readonly id: number; readonly company: string; readonly policy: { readonly chart_version: number }; readonly approved_at: CivilInstantValue; readonly published_at: CivilInstantValue; readonly origin: PublicationOrigin; readonly supersedes: number | null; readonly reports: ReportSet }
export type AnnouncementContent = { readonly Shock: ActiveShock } | { readonly CashDividend: { readonly plan: CashDividendPlan; readonly total_gross: string } } | { readonly RightsOffering: { readonly plan: import("../corporate-actions.ts").RightsOfferingEventPlan } } | { readonly IssuerRepurchase: { readonly plan: import("../corporate-actions.ts").IssuerRepurchasePlan } }
export type Announcement = { readonly id: number; readonly company: string; readonly occurred_on: string; readonly published_at: CivilInstantValue; readonly content: AnnouncementContent }
export type PublicLibrary = { readonly next_seq: number; readonly reports: readonly PublishedReport[]; readonly announcements: readonly Announcement[] }

function variant(value: unknown, path: string): readonly [string, unknown] { const entries = Object.entries(record(value, path)); if (entries.length !== 1 || entries[0] === undefined) throw new SaveSchemaError(path, "必须是单一标签联合"); return entries[0] }
function scope(value: unknown, path: string): Scope { const [tag, body] = variant(value, path); if (tag === "Standalone") return { Standalone: string(body, `${path}.Standalone`) }; if (tag === "Consolidated") return { Consolidated: string(body, `${path}.Consolidated`) }; throw new SaveSchemaError(path, "包含无效范围变体") }
function line(value: unknown, path: string, allowed: readonly string[]): LineAmount { const item = array(value, path); if (item.length !== 2) throw new SaveSchemaError(path, "必须是二元组"); return [oneOf(item[0], `${path}[0]`, allowed) as LineAmount[0], amount(item[1], `${path}[1]`)] }
function comparative<T>(value: unknown, path: string, parser: (item: unknown, itemPath: string) => T): Comparative<T> { const [tag, body] = variant(value, path); if (tag === "Available") return { Available: parser(body, `${path}.Available`) }; if (tag === "Unavailable") { const item = record(body, `${path}.Unavailable`); exact(item, ["reason"], `${path}.Unavailable`); return { Unavailable: { reason: oneOf(item.reason, `${path}.Unavailable.reason`, ["NoPriorYearHistory"] as const) } } }; throw new SaveSchemaError(path, "包含无效比较项") }
function parseColumns(value: unknown, path: string): IncomeColumns { const item = record(value, path); exact(item, ["operating", "operating_subtotal", "investing", "investing_subtotal", "financing", "financing_subtotal", "discontinued", "discontinued_subtotal", "income_tax", "net_income"], path); const lines = (name: "operating" | "investing" | "financing" | "discontinued") => array(item[name], `${path}.${name}`).map((entry, index) => line(entry, `${path}.${name}[${index}]`, incomeLines)); return { operating: lines("operating"), operating_subtotal: amount(item.operating_subtotal, `${path}.operating_subtotal`), investing: lines("investing"), investing_subtotal: amount(item.investing_subtotal, `${path}.investing_subtotal`), financing: lines("financing"), financing_subtotal: amount(item.financing_subtotal, `${path}.financing_subtotal`), discontinued: lines("discontinued"), discontinued_subtotal: amount(item.discontinued_subtotal, `${path}.discontinued_subtotal`), income_tax: amount(item.income_tax, `${path}.income_tax`), net_income: amount(item.net_income, `${path}.net_income`) } }
const roeUnavailable = ["NonPositiveAverageEquity", "MissingNonRecurringIncomeFacts", "IncompleteEquityEventHistory", "SameControlCombination", "ComparativePeriodSpecialTreatment", "UnclassifiedEquityEvent", "ConsolidatedAttributionFactsUnavailable"] as const
function parseReportRoeValue(value: unknown, path: string): ReportRoeValue {
  const [tag, body] = variant(value, path)
  if (tag === "Available") {
    const ratio = record(body, `${path}.Available`)
    exact(ratio, ["numerator", "denominator"], `${path}.Available`)
    const numerator = string(ratio.numerator, `${path}.Available.numerator`)
    const denominator = string(ratio.denominator, `${path}.Available.denominator`)
    if (!/^(0|-?[1-9]\d*)$/.test(numerator)) throw new SaveSchemaError(`${path}.Available.numerator`, "必须是规范有符号十进制整数")
    if (!/^[1-9]\d*$/.test(denominator)) throw new SaveSchemaError(`${path}.Available.denominator`, "必须是规范正十进制整数")
    return { Available: { numerator, denominator } }
  }
  if (tag === "Unavailable") {
    const unavailable = record(body, `${path}.Unavailable`)
    exact(unavailable, ["reason"], `${path}.Unavailable`)
    return { Unavailable: { reason: oneOf(unavailable.reason, `${path}.Unavailable.reason`, roeUnavailable) } }
  }
  throw new SaveSchemaError(path, "包含无效 ROE 可用值变体")
}
function parseReportRoe(value: unknown, path: string): ReportRoe {
  const item = record(value, path)
  exact(item, ["basis", "ordinary_roe", "adjusted_roe", "weighted_average_parent_equity_cents"], path)
  let basis: ReportRoe["basis"]
  if (item.basis === "AttributableToOrdinaryShareholders") basis = item.basis
  else {
    const [tag, body] = variant(item.basis, `${path}.basis`)
    if (tag !== "Unsupported") throw new SaveSchemaError(`${path}.basis`, "包含无效 ROE 口径")
    const unsupported = record(body, `${path}.basis.Unsupported`)
    exact(unsupported, ["reason"], `${path}.basis.Unsupported`)
    const reason = oneOf(unsupported.reason, `${path}.basis.Unsupported.reason`, roeUnavailable)
    if (reason === "NonPositiveAverageEquity" || reason === "MissingNonRecurringIncomeFacts") throw new SaveSchemaError(`${path}.basis.Unsupported.reason`, "该原因只可用于具体 ROE 指标，不可用于 unsupported basis")
    basis = { Unsupported: { reason } }
  }
  return { basis, ordinary_roe: parseReportRoeValue(item.ordinary_roe, `${path}.ordinary_roe`), adjusted_roe: parseReportRoeValue(item.adjusted_roe, `${path}.adjusted_roe`), weighted_average_parent_equity_cents: parseReportRoeValue(item.weighted_average_parent_equity_cents, `${path}.weighted_average_parent_equity_cents`) }
}
function validateReportRoe(report: ReportSet, path: string): void {
  const [windowStart, windowEnd] = report.window
  if (windowStart > windowEnd || windowEnd > report.period) throw new SaveSchemaError(`${path}.window`, "报告窗口起止期间无效")
  const month = Number(report.period.slice(5, 7))
  const quarterStartMonth = Math.floor((month - 1) / 3) * 3 + 1
  const expectedWindow = report.kind === "Monthly"
    ? [report.period, report.period]
    : report.kind === "Quarter" && month % 3 === 0
      ? [`${report.period.slice(0, 4)}-${quarterStartMonth.toString().padStart(2, "0")}`, report.period]
      : report.kind === "HalfYear" && (month === 6 || month === 12)
        ? [`${report.period.slice(0, 4)}-${month === 6 ? "01" : "07"}`, report.period]
        : report.kind === "Annual" && month === 12
          ? [`${report.period.slice(0, 4)}-01`, report.period]
          : null
  if (expectedWindow === null || expectedWindow[0] !== windowStart || expectedWindow[1] !== windowEnd) throw new SaveSchemaError(`${path}.window`, "必须符合报告类型与期间的实际窗口")
  const income = report.income.report_period
  const expectedNetIncome = BigInt(income.operating_subtotal.replace(".", "")) + BigInt(income.investing_subtotal.replace(".", "")) + BigInt(income.financing_subtotal.replace(".", "")) + BigInt(income.discontinued_subtotal.replace(".", "")) - BigInt(income.income_tax.replace(".", ""))
  if (expectedNetIncome !== BigInt(income.net_income.replace(".", ""))) throw new SaveSchemaError(`${path}.income.report_period.net_income`, "必须与报告窗口利润表小计及所得税勾稽")
  if ((report.kind === "Quarter" && JSON.stringify(report.income.report_period) !== JSON.stringify(report.income.quarter)) || ((report.kind === "HalfYear" || report.kind === "Annual") && JSON.stringify(report.income.report_period) !== JSON.stringify(report.income.cumulative))) throw new SaveSchemaError(`${path}.income.report_period`, "必须与该报告类型的实际流量窗口一致")
  const consolidated = "Consolidated" in report.scope
  if (consolidated !== (report.income.report_period_net_income_to_parent !== null)) throw new SaveSchemaError(`${path}.income.report_period_net_income_to_parent`, "必填值必须与报告范围一致")
  const reportPeriodParentNetIncome = report.income.report_period_net_income_to_parent ?? income.net_income
  if (report.equity.net_income !== reportPeriodParentNetIncome) throw new SaveSchemaError(`${path}.equity.net_income`, "必须与同窗口归属净利润一致")
  const roe = report.roe
  if (typeof roe.basis !== "string") {
    const reason = roe.basis.Unsupported.reason
    for (const [key, value] of [["ordinary_roe", roe.ordinary_roe], ["adjusted_roe", roe.adjusted_roe], ["weighted_average_parent_equity_cents", roe.weighted_average_parent_equity_cents]] as const) {
      if (!("Unavailable" in value) || value.Unavailable.reason !== reason) throw new SaveSchemaError(`${path}.roe.${key}`, "unsupported basis必须使所有 ROE 指标使用相同不可用原因")
    }
    return
  }
  if (consolidated) throw new SaveSchemaError(`${path}.roe.basis`, "合并报告缺少归母权益事件明细，不支持 ROE")
  const average = roe.weighted_average_parent_equity_cents
  if (!("Available" in average)) throw new SaveSchemaError(`${path}.roe.weighted_average_parent_equity_cents`, "归母口径必须保留平均归母权益精确值")
  const averageNumerator = BigInt(average.Available.numerator)
  const averageDenominator = BigInt(average.Available.denominator)
  if (averageNumerator <= 0n) {
    if (!("Unavailable" in roe.ordinary_roe) || roe.ordinary_roe.Unavailable.reason !== "NonPositiveAverageEquity") throw new SaveSchemaError(`${path}.roe.ordinary_roe`, "平均权益非正时普通 ROE 必须明确不可用")
  } else {
    if (!("Available" in roe.ordinary_roe)) throw new SaveSchemaError(`${path}.roe.ordinary_roe`, "平均权益为正时普通 ROE 必须可用")
    const profitCents = BigInt(reportPeriodParentNetIncome.replace(".", ""))
    const ratio = roe.ordinary_roe.Available
    if (BigInt(ratio.numerator) * averageNumerator !== profitCents * averageDenominator * BigInt(ratio.denominator)) throw new SaveSchemaError(`${path}.roe.ordinary_roe`, "必须与同期间同归属净利润和平均权益一致")
  }
  if (!("Unavailable" in roe.adjusted_roe) || roe.adjusted_roe.Unavailable.reason !== "MissingNonRecurringIncomeFacts") throw new SaveSchemaError(`${path}.roe.adjusted_roe`, "缺少扣非利润事实时必须明确不可用")
}
function parseNotes(value: unknown, path: string): Notes { const item = record(value, path); exact(item, ["items", "consolidation_split_items"], path); const note = (entry: unknown, entryPath: string): NoteItem => { const value = record(entry, entryPath); exact(value, ["code", "name", "target", "opening", "movement", "ytd_movement", "closing"], entryPath); const [tag, body] = variant(value.target, `${entryPath}.target`); const target = tag === "BalanceSheet" ? { BalanceSheet: oneOf(body, `${entryPath}.target.BalanceSheet`, balanceLines) } : tag === "Income" ? { Income: oneOf(body, `${entryPath}.target.Income`, incomeLines) } : (() => { throw new SaveSchemaError(`${entryPath}.target`, "包含无效附注目标") })(); return { code: string(value.code, `${entryPath}.code`), name: string(value.name, `${entryPath}.name`), target, opening: amount(value.opening, `${entryPath}.opening`), movement: amount(value.movement, `${entryPath}.movement`), ytd_movement: amount(value.ytd_movement, `${entryPath}.ytd_movement`), closing: amount(value.closing, `${entryPath}.closing`) } }; return { items: array(item.items, `${path}.items`).map((entry, index) => note(entry, `${path}.items[${index}]`)), consolidation_split_items: array(item.consolidation_split_items, `${path}.consolidation_split_items`).map((entry, index) => note(entry, `${path}.consolidation_split_items[${index}]`)) } }

export function parseReportSet(value: unknown, path: string): ReportSet {
  const item = record(value, path); exact(item, ["scope", "period", "kind", "window", "version", "balance_sheet", "income", "roe", "cash_flow", "equity", "notes"], path); const window = array(item.window, `${path}.window`); if (window.length !== 2) throw new SaveSchemaError(`${path}.window`, "必须是二元组"); const balance = record(item.balance_sheet, `${path}.balance_sheet`); exact(balance, ["asset_lines", "total_assets", "liability_lines", "total_liabilities", "equity_lines", "total_equity", "equity_to_parent", "liabilities_and_equity", "closing_cash", "prior_year_end"], `${path}.balance_sheet`); const balanceLinesOf = (name: "asset_lines" | "liability_lines" | "equity_lines") => { const allowed = name === "asset_lines" ? assetLines : name === "liability_lines" ? liabilityLines : equityLines; return array(balance[name], `${path}.balance_sheet.${name}`).map((entry, index) => line(entry, `${path}.balance_sheet.${name}[${index}]`, allowed)); }; const income = record(item.income, `${path}.income`); exact(income, ["report_period", "report_period_net_income_to_parent", "quarter", "cumulative", "prior_year", "minority_net_income", "net_income_to_parent"], `${path}.income`); const roe = parseReportRoe(item.roe, `${path}.roe`); const flow = record(item.cash_flow, `${path}.cash_flow`); exact(flow, ["operating", "investing", "financing", "net_change", "opening_cash", "closing_cash", "indirect"], `${path}.cash_flow`); const equity = record(item.equity, `${path}.equity`); exact(equity, ["opening_parent", "net_income", "other_comprehensive", "capital_contributions", "distributions", "closing_parent", "opening_minority", "minority_net_income", "closing_minority"], `${path}.equity`); const parsed: ReportSet = { scope: scope(item.scope, `${path}.scope`), period: period(item.period, `${path}.period`), kind: oneOf(item.kind, `${path}.kind`, reportKinds), window: [period(window[0], `${path}.window[0]`), period(window[1], `${path}.window[1]`)], version: parseVersion(item.version, `${path}.version`), balance_sheet: { asset_lines: balanceLinesOf("asset_lines"), total_assets: amount(balance.total_assets, `${path}.balance_sheet.total_assets`), liability_lines: balanceLinesOf("liability_lines"), total_liabilities: amount(balance.total_liabilities, `${path}.balance_sheet.total_liabilities`), equity_lines: balanceLinesOf("equity_lines"), total_equity: amount(balance.total_equity, `${path}.balance_sheet.total_equity`), equity_to_parent: amount(balance.equity_to_parent, `${path}.balance_sheet.equity_to_parent`), liabilities_and_equity: amount(balance.liabilities_and_equity, `${path}.balance_sheet.liabilities_and_equity`), closing_cash: amount(balance.closing_cash, `${path}.balance_sheet.closing_cash`), prior_year_end: comparative(balance.prior_year_end, `${path}.balance_sheet.prior_year_end`, (entry, entryPath) => array(entry, entryPath).map((lineValue, index) => line(lineValue, `${entryPath}[${index}]`, balanceLines))) }, income: { report_period: parseColumns(income.report_period, `${path}.income.report_period`), report_period_net_income_to_parent: income.report_period_net_income_to_parent === null ? null : amount(income.report_period_net_income_to_parent, `${path}.income.report_period_net_income_to_parent`), quarter: parseColumns(income.quarter, `${path}.income.quarter`), cumulative: parseColumns(income.cumulative, `${path}.income.cumulative`), prior_year: comparative(income.prior_year, `${path}.income.prior_year`, parseColumns), minority_net_income: income.minority_net_income === null ? null : amount(income.minority_net_income, `${path}.income.minority_net_income`), net_income_to_parent: income.net_income_to_parent === null ? null : amount(income.net_income_to_parent, `${path}.income.net_income_to_parent`) }, roe, cash_flow: { operating: amount(flow.operating, `${path}.cash_flow.operating`), investing: amount(flow.investing, `${path}.cash_flow.investing`), financing: amount(flow.financing, `${path}.cash_flow.financing`), net_change: amount(flow.net_change, `${path}.cash_flow.net_change`), opening_cash: amount(flow.opening_cash, `${path}.cash_flow.opening_cash`), closing_cash: amount(flow.closing_cash, `${path}.cash_flow.closing_cash`), indirect: array(flow.indirect, `${path}.cash_flow.indirect`).map((entry, index) => { const indirect = record(entry, `${path}.cash_flow.indirect[${index}]`); exact(indirect, ["label", "amount"], `${path}.cash_flow.indirect[${index}]`); return { label: string(indirect.label, `${path}.cash_flow.indirect[${index}].label`), amount: amount(indirect.amount, `${path}.cash_flow.indirect[${index}].amount`) } }) }, equity: { opening_parent: amount(equity.opening_parent, `${path}.equity.opening_parent`), net_income: amount(equity.net_income, `${path}.equity.net_income`), other_comprehensive: amount(equity.other_comprehensive, `${path}.equity.other_comprehensive`), capital_contributions: amount(equity.capital_contributions, `${path}.equity.capital_contributions`), distributions: amount(equity.distributions, `${path}.equity.distributions`), closing_parent: amount(equity.closing_parent, `${path}.equity.closing_parent`), opening_minority: equity.opening_minority === null ? null : amount(equity.opening_minority, `${path}.equity.opening_minority`), minority_net_income: equity.minority_net_income === null ? null : amount(equity.minority_net_income, `${path}.equity.minority_net_income`), closing_minority: equity.closing_minority === null ? null : amount(equity.closing_minority, `${path}.equity.closing_minority`) }, notes: parseNotes(item.notes, `${path}.notes`) }; validateReportRoe(parsed, path); return parsed
}

function parseVersion(value: unknown, path: string): ReportVersion { const item = record(value, path); exact(item, ["sequence", "supersedes", "kind"], path); const kind = item.kind === "Original" ? item.kind : (() => { const [tag, body] = variant(item.kind, `${path}.kind`); if (tag !== "Correction") throw new SaveSchemaError(`${path}.kind`, "包含无效版本种类"); const correction = record(body, `${path}.kind.Correction`); exact(correction, ["reason"], `${path}.kind.Correction`); return { Correction: { reason: string(correction.reason, `${path}.kind.Correction.reason`) } } })(); return { sequence: integer(item.sequence, `${path}.sequence`, 1), supersedes: item.supersedes === null ? null : integer(item.supersedes, `${path}.supersedes`, 1), kind } }
function parseScheduledKind(value: unknown, path: string): ScheduledKind {
  if (typeof value === "string") return oneOf(value, path, scheduledKinds)
  const item = record(value, path)
  exact(item, ["Monthly"], path)
  const body = record(item.Monthly, `${path}.Monthly`)
  exact(body, ["month"], `${path}.Monthly`)
  const month = integer(body.month, `${path}.Monthly.month`, 1)
  if (month > 12) throw new SaveSchemaError(`${path}.Monthly.month`, "月份不能超过 12")
  return { Monthly: { month } }
}
export function parsePublicationOrigin(value: unknown, path: string): PublicationOrigin {
  if (value === "Correction") return value
  const [tag, body] = variant(value, path)
  const item = record(body, `${path}.${tag}`)
  if (tag === "MonthlyDisclosure") {
    exact(item, ["schedule", "delay_days", "seeded"], `${path}.MonthlyDisclosure`)
    const schedule = parseMonthlyReportSchedule(item.schedule, `${path}.MonthlyDisclosure.schedule`)
    const delay = integer(item.delay_days, `${path}.MonthlyDisclosure.delay_days`, 0)
    const configured = "Preset" in schedule ? schedule.Preset.delay : schedule.Custom.delay
    const maximum = configured === "None" ? 0 : configured.Uniform.max_days
    if (delay > maximum) throw new SaveSchemaError(`${path}.MonthlyDisclosure.delay_days`, "实际延迟超出排期配置")
    return { MonthlyDisclosure: { schedule, delay_days: delay, seeded: boolean(item.seeded, `${path}.MonthlyDisclosure.seeded`) } }
  }
  if (tag !== "SeededPrehistory" && tag !== "ScheduledDisclosure") throw new SaveSchemaError(path, "包含无效公布来源")
  exact(item, ["fiscal_year", "kind", "offset_days"], `${path}.${tag}`)
  const result = { fiscal_year: integer(item.fiscal_year, `${path}.${tag}.fiscal_year`), kind: parseScheduledKind(item.kind, `${path}.${tag}.kind`), offset_days: integer(item.offset_days, `${path}.${tag}.offset_days`, 0) }
  if (typeof result.kind === "object") throw new SaveSchemaError(path, "Monthly必须使用显式MonthlyDisclosure排期来源")
  return tag === "SeededPrehistory" ? { SeededPrehistory: result } : { ScheduledDisclosure: result }
}

function parseCashDividendAnnouncement(value: unknown, company: string, occurredOn: string, path: string): AnnouncementContent {
  const content = record(value, path)
  exact(content, ["kind", "value"], path)
  const tag = oneOf(content.kind, `${path}.kind`, ["Shock", "CashDividend"] as const)
  const body = content.value
  if (tag === "Shock") {
    const event = record(body, `${path}.value`)
    exact(event, ["kind", "amplitude_bp", "starts_on", "expires_on"], `${path}.value`)
    const startsOn = civilDate(event.starts_on, `${path}.value.starts_on`)
    const expiresOn = civilDate(event.expires_on, `${path}.value.expires_on`)
    if (expiresOn < startsOn) throw new SaveSchemaError(`${path}.value.expires_on`, "不得早于 starts_on")
    const kind = parseShockKind(event.kind, `${path}.value.kind`)
    const amplitude = integer(event.amplitude_bp, `${path}.value.amplitude_bp`)
    if (typeof kind === "object" && "PaymentFailure" in kind && (amplitude !== 0 || startsOn !== occurredOn || expiresOn !== occurredOn)) throw new SaveSchemaError(`${path}.value`, "PaymentFailure 必须为幅度 0 的当日事实")
    return { Shock: { kind, amplitude_bp: amplitude, starts_on: startsOn, expires_on: expiresOn } }
  }
  const declaration = record(body, `${path}.value`)
  exact(declaration, ["plan", "total_gross"], `${path}.value`)
  const planValue = record(declaration.plan, `${path}.value.plan`)
  const planPath = `${path}.value.plan`
  exact(planValue, ["plan_id", "issuer", "stock", "exchange", "formula", "approved_on", "announced_on", "registered_on", "ex_dividend_on", "payable_on", "gross_per_share", "distributable_amount"], planPath)
  const planId = string(planValue.plan_id, `${planPath}.plan_id`)
  const issuer = string(planValue.issuer, `${planPath}.issuer`)
  const stock = string(planValue.stock, `${planPath}.stock`)
  if (!planId.trim() || !issuer.trim() || !stock.trim()) throw new SaveSchemaError(planPath, "方案、发行人及证券身份不得为空")
  const exchange = oneOf(planValue.exchange, `${planPath}.exchange`, ["sse", "szse"] as const)
  const formula = oneOf(planValue.formula, `${planPath}.formula`, ["StandardCashOnly", "ExchangeApprovedAdjustment"] as const)
  const approved_on = civilDate(planValue.approved_on, `${planPath}.approved_on`)
  const announced_on = civilDate(planValue.announced_on, `${planPath}.announced_on`)
  const registered_on = civilDate(planValue.registered_on, `${planPath}.registered_on`)
  const ex_dividend_on = civilDate(planValue.ex_dividend_on, `${planPath}.ex_dividend_on`)
  const payable_on = civilDate(planValue.payable_on, `${planPath}.payable_on`)
  if (approved_on > announced_on || announced_on > registered_on || registered_on >= ex_dividend_on || ex_dividend_on > payable_on || announced_on !== occurredOn) throw new SaveSchemaError(planPath, "分红方案日期顺序或公告发生日期无效")
  const monthDelta = (Number(payable_on.slice(0, 4)) - Number(approved_on.slice(0, 4))) * 12 + Number(payable_on.slice(5, 7)) - Number(approved_on.slice(5, 7))
  if (monthDelta > 6 || (monthDelta === 6 && Number(payable_on.slice(8, 10)) > Number(approved_on.slice(8, 10)))) throw new SaveSchemaError(`${planPath}.payable_on`, "付款日期超出批准日起六个月期限")
  const gross_per_share = money(planValue.gross_per_share, `${planPath}.gross_per_share`)
  const distributable_amount = money(planValue.distributable_amount, `${planPath}.distributable_amount`)
  const total_gross = money(declaration.total_gross, `${path}.value.total_gross`)
  if (BigInt(gross_per_share) <= 0n || BigInt(distributable_amount) < 0n || BigInt(total_gross) <= 0n || BigInt(total_gross) > BigInt(distributable_amount)) throw new SaveSchemaError(`${path}.value`, "分红金额须为正数且不超过可分配金额")
  if (issuer !== company) throw new SaveSchemaError(`${planPath}.issuer`, "必须与公告公司一致")
  const plan: CashDividendPlan = { plan_id: planId, issuer, stock, exchange, formula, approved_on, announced_on, registered_on, ex_dividend_on, payable_on, gross_per_share, distributable_amount }
  return { CashDividend: { plan, total_gross } }
}

/// 配股／增发与回购方案公告：披露事实只含方案条款（engine `RightsOfferingAnnouncement`
/// / `IssuerRepurchaseAnnouncement` 的序列化形态），认购与结算结果由后续事实承载。
/// 方案细节字段按 corporate-actions 的权威 parser 校验；此处校验发行人与发生日归属。
function parseCompanyActionAnnouncement(
  value: unknown,
  company: string,
  occurredOn: string,
  path: string,
): AnnouncementContent {
  const content = record(value, path)
  exact(content, ["kind", "value"], path)
  const tag = oneOf(content.kind, `${path}.kind`, ["RightsOffering", "IssuerRepurchase"] as const)
  const body = record(content.value, `${path}.value`)
  if (tag === "RightsOffering") {
    exact(body, ["plan"], `${path}.value`)
    const plan = parseRightsOfferingEventPlanValue(body.plan, `${path}.value.plan`)
    if (plan.issuer !== company || plan.announced_on !== occurredOn) throw new SaveSchemaError(`${path}.value.plan`, "配股公告发行人或发生日不一致")
    return { RightsOffering: { plan } }
  }
  exact(body, ["plan"], `${path}.value`)
  const plan = parseIssuerRepurchasePlanValue(body.plan, `${path}.value.plan`)
  if (plan.issuer !== company || plan.announced_on !== occurredOn) throw new SaveSchemaError(`${path}.value.plan`, "回购公告发行人或发生日不一致")
  return { IssuerRepurchase: { plan } }
}

function parseAnnouncement(value: unknown, path: string): Announcement {
  const item = record(value, path)
  exact(item, ["id", "company", "occurred_on", "published_at", "content"], path)
  const company = string(item.company, `${path}.company`)
  const occurred_on = civilDate(item.occurred_on, `${path}.occurred_on`)
  const contentKind = typeof item.content === "object" && item.content !== null
    ? (item.content as Record<string, unknown>).kind
    : undefined
  const content = contentKind === "RightsOffering" || contentKind === "IssuerRepurchase"
    ? parseCompanyActionAnnouncement(item.content, company, occurred_on, `${path}.content`)
    : parseCashDividendAnnouncement(item.content, company, occurred_on, `${path}.content`)
  return { id: integer(item.id, `${path}.id`, 0), company, occurred_on, published_at: instant(item.published_at, `${path}.published_at`), content }
}

function parsePublishedReport(value: unknown, path: string): PublishedReport {
  const report = record(value, path)
  exact(report, ["source", "id", "company", "policy", "approved_at", "published_at", "origin", "supersedes", "reports"], path)
  const policy = record(report.policy, `${path}.policy`)
  exact(policy, ["chart_version"], `${path}.policy`)
  const origin = parsePublicationOrigin(report.origin, `${path}.origin`)
  const reports = parseReportSet(report.reports, `${path}.reports`)
  const published = instant(report.published_at, `${path}.published_at`)
  if (typeof origin === "object" && "MonthlyDisclosure" in origin) {
    if (reports.kind !== "Monthly") throw new SaveSchemaError(`${path}.origin`, "MonthlyDisclosure来源必须为Monthly报告")
    const monthly = origin.MonthlyDisclosure
    const day = "Custom" in monthly.schedule ? monthly.schedule.Custom.day : monthly.schedule.Preset.preset === "FirstDayEvening" ? 1 : 10
    const second = "Custom" in monthly.schedule ? monthly.schedule.Custom.second_of_day : 64800
    const [yearText, monthText] = reports.period.split("-")
    const date = new Date(Date.UTC(Number(yearText), Number(monthText), day + monthly.delay_days))
    const expected = date.toISOString().slice(0, 10)
    civilDate(expected, `${path}.origin.MonthlyDisclosure`)
    if (date.getUTCFullYear() < 1900 || date.getUTCFullYear() > 2199 || published.date !== expected || published.second_of_day !== second) throw new SaveSchemaError(`${path}.published_at`, "公布时点不符合原期间月报排期与实抽延迟")
  }
  return { source: oneOf(report.source, `${path}.source`, ["SimpleGenerated", "SimulationAccounting"] as const), id: integer(report.id, `${path}.id`, 0), company: string(report.company, `${path}.company`), policy: { chart_version: integer(policy.chart_version, `${path}.policy.chart_version`, 1) }, approved_at: instant(report.approved_at, `${path}.approved_at`), published_at: published, origin, supersedes: report.supersedes === null ? null : integer(report.supersedes, `${path}.supersedes`, 0), reports }
}

export function parsePublicLibrary(value: unknown, path = "public_library"): PublicLibrary { const item = record(value, path); exact(item, ["next_seq", "reports", "announcements"], path); const reports = array(item.reports, `${path}.reports`).map((entry, index) => parsePublishedReport(entry, `${path}.reports[${index}]`)); const announcements = array(item.announcements, `${path}.announcements`).map((entry, index) => parseAnnouncement(entry, `${path}.announcements[${index}]`)); return { next_seq: integer(item.next_seq, `${path}.next_seq`, 0), reports, announcements } }
