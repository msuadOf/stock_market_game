import type { SessionSetup } from "../../types/generated/SessionSetup"
import type { SimpleConfig } from "../../types/generated/SimpleConfig"
import type { SimpleCompanyConfig } from "../../types/generated/SimpleCompanyConfig"
import type { PeriodNoiseConfig } from "../../types/generated/PeriodNoiseConfig"
import type { AnnualTrendConfig } from "../../types/generated/AnnualTrendConfig"
import type { PeriodGenerationParameters } from "../../types/generated/PeriodGenerationParameters"
import type { PeriodVariableExpenseRule } from "../../types/generated/PeriodVariableExpenseRule"
import type { SimpleFinanceConfig } from "../../types/generated/SimpleFinanceConfig"
import type { SimpleCompanyPreferences } from "../../types/generated/SimpleCompanyPreferences"
import { parseMonthlyReportSchedule } from "./monthly-report-schedule.ts"
import type { ReportFrequency } from "../../types/generated/ReportFrequency"
import { parseJournalLine } from "./company/accounting/journal.ts"
import { parseTaxPolicy } from "./company/books/industrial.ts"
import { amount, i128 } from "./company/value.ts"
import { SaveSchemaError, array, boolean, exact, integer, money, oneOf, record } from "./primitives.ts"

export const SETUP_COMPANY_OPTION_KEYS = ["company_system", "dividend_tax_mode", "flat_withholding_bp", "rights_offering_enabled", "issuer_repurchase_enabled", "par_value_per_share", "auto_corporate_foundation", "report_frequency"] as const

type SetupCompanyOptions = Pick<SessionSetup, (typeof SETUP_COMPANY_OPTION_KEYS)[number]>
const I32_MAX = 2_147_483_647

function bounded(value: unknown, path: string, min: number, max: number): number {
  const result = integer(value, path, min)
  if (result > max) throw new SaveSchemaError(path, `必须不大于 ${max}`)
  return result
}

function generationAmount(value: unknown, path: string): string {
  const result = amount(value, path)
  i128(result.replace(".", ""), path)
  if (result.startsWith("-")) throw new SaveSchemaError(path, "期间营收与费用不得为负")
  return result
}

function positiveMoney(value: unknown, path: string): string {
  const result = money(value, path)
  if (BigInt(result) <= 0n) throw new SaveSchemaError(path, "必须为正的 Money 分字符串")
  return result
}

function noise(value: unknown, path: string): PeriodNoiseConfig {
  const parsed = record(value, path)
  exact(parsed, ["monthly_bp", "quarterly_bp", "half_year_bp", "annual_bp"], path)
  return { monthly_bp: bounded(parsed.monthly_bp, `${path}.monthly_bp`, 0, I32_MAX), quarterly_bp: bounded(parsed.quarterly_bp, `${path}.quarterly_bp`, 0, I32_MAX), half_year_bp: bounded(parsed.half_year_bp, `${path}.half_year_bp`, 0, I32_MAX), annual_bp: bounded(parsed.annual_bp, `${path}.annual_bp`, 0, I32_MAX) }
}

function trend(value: unknown, path: string): AnnualTrendConfig {
  const parsed = record(value, path)
  const kind = oneOf(parsed.kind, `${path}.kind`, ["Fixed", "Persistent"] as const)
  if (kind === "Fixed") {
    exact(parsed, ["kind", "annual_growth_bp"], path)
    return { kind, annual_growth_bp: bounded(parsed.annual_growth_bp, `${path}.annual_growth_bp`, -10000, I32_MAX) }
  }
  exact(parsed, ["kind", "annual_growth_min_bp", "annual_growth_max_bp", "duration_min_months", "duration_max_months"], path)
  const minimum = bounded(parsed.annual_growth_min_bp, `${path}.annual_growth_min_bp`, -10000, I32_MAX)
  const duration = bounded(parsed.duration_min_months, `${path}.duration_min_months`, 1, 65535)
  return { kind, annual_growth_min_bp: minimum, annual_growth_max_bp: bounded(parsed.annual_growth_max_bp, `${path}.annual_growth_max_bp`, minimum, I32_MAX), duration_min_months: duration, duration_max_months: bounded(parsed.duration_max_months, `${path}.duration_max_months`, duration, 65535) }
}

function variableExpense(value: unknown, path: string): PeriodVariableExpenseRule {
  const parsed = record(value, path)
  const rule = oneOf(parsed.rule, `${path}.rule`, ["RevenueRatio", "Growth"] as const)
  if (rule === "RevenueRatio") {
    exact(parsed, ["rule", "ratio_bp", "noise"], path)
    return { rule, ratio_bp: bounded(parsed.ratio_bp, `${path}.ratio_bp`, 0, I32_MAX), noise: noise(parsed.noise, `${path}.noise`) }
  }
  exact(parsed, ["rule", "initial_amount", "trend", "noise"], path)
  return { rule, initial_amount: generationAmount(parsed.initial_amount, `${path}.initial_amount`), trend: trend(parsed.trend, `${path}.trend`), noise: noise(parsed.noise, `${path}.noise`) }
}

function generation(value: unknown, path: string): PeriodGenerationParameters {
  const parsed = record(value, path)
  exact(parsed, ["initial_revenue", "initial_fixed_expense", "revenue_trend", "fixed_expense_trend", "revenue_noise", "fixed_expense_noise", "demand_sensitivity_bp", "variable_expense"], path)
  return { initial_revenue: generationAmount(parsed.initial_revenue, `${path}.initial_revenue`), initial_fixed_expense: generationAmount(parsed.initial_fixed_expense, `${path}.initial_fixed_expense`), revenue_trend: trend(parsed.revenue_trend, `${path}.revenue_trend`), fixed_expense_trend: trend(parsed.fixed_expense_trend, `${path}.fixed_expense_trend`), revenue_noise: noise(parsed.revenue_noise, `${path}.revenue_noise`), fixed_expense_noise: noise(parsed.fixed_expense_noise, `${path}.fixed_expense_noise`), demand_sensitivity_bp: bounded(parsed.demand_sensitivity_bp, `${path}.demand_sensitivity_bp`, -2147483648, I32_MAX), variable_expense: variableExpense(parsed.variable_expense, `${path}.variable_expense`) }
}

function finance(value: unknown, path: string): SimpleFinanceConfig {
  const parsed = record(value, path)
  exact(parsed, ["opening_lines", "tax_policy", "summary_rule", "book_display"], path)
  const display = record(parsed.book_display, `${path}.book_display`)
  exact(display, ["investment_of_revenue_bp"], `${path}.book_display`)
  const taxPath = `${path}.tax_policy`
  const tax = parseTaxPolicy(parsed.tax_policy, taxPath)
  const taxPolicy: SimpleFinanceConfig["tax_policy"] = {
    version: bounded(tax.version, `${taxPath}.version`, 1, 4294967295),
    vat: { output_rate_bp: bounded(tax.vat.output_rate_bp, `${taxPath}.vat.output_rate_bp`, 0, 10000), input_rate_bp: bounded(tax.vat.input_rate_bp, `${taxPath}.vat.input_rate_bp`, 0, 10000), deductible_share_bp: bounded(tax.vat.deductible_share_bp, `${taxPath}.vat.deductible_share_bp`, 0, 10000) },
    income_tax: { rate_bp: bounded(tax.income_tax.rate_bp, `${taxPath}.income_tax.rate_bp`, 0, 10000), loss_carryforward_years: bounded(tax.income_tax.loss_carryforward_years, `${taxPath}.income_tax.loss_carryforward_years`, 1, 65535) },
  }
  return { opening_lines: array(parsed.opening_lines, `${path}.opening_lines`).map((line, index) => {
    const linePath = `${path}.opening_lines[${index}]`
    const parsedLine = parseJournalLine(line, linePath)
    const parsedAmount = amount(parsedLine.amount, `${linePath}.amount`)
    // Rust from_yuan_str 先构造正绝对值，再取负；负下界同样不包含 i128::MIN。
    i128(parsedAmount.replace(".", "").replace(/^-/, ""), `${linePath}.amount`)
    return { ...parsedLine, amount: parsedAmount }
  }), tax_policy: taxPolicy, summary_rule: oneOf(parsed.summary_rule, `${path}.summary_rule`, ["ReceivableRevenuePayableExpenses"] as const), book_display: { investment_of_revenue_bp: bounded(display.investment_of_revenue_bp, `${path}.book_display.investment_of_revenue_bp`, 0, 10000) } }
}

function preferences(value: unknown, path: string): SimpleCompanyPreferences {
  const parsed = record(value, path)
  exact(parsed, ["cash_dividend", "stock_distribution"], path)
  const result: SimpleCompanyPreferences = { cash_dividend: null, stock_distribution: null }
  if (parsed.cash_dividend !== null) {
    const cashPath = `${path}.cash_dividend`
    const cash = record(parsed.cash_dividend, cashPath)
    exact(cash, ["target_payout_bp", "min_distributable_profit", "cycles_between_proposals"], cashPath)
    result.cash_dividend = { target_payout_bp: bounded(cash.target_payout_bp, `${cashPath}.target_payout_bp`, 1, 10000), min_distributable_profit: positiveMoney(cash.min_distributable_profit, `${cashPath}.min_distributable_profit`), cycles_between_proposals: bounded(cash.cycles_between_proposals, `${cashPath}.cycles_between_proposals`, 1, 65535) }
  }
  if (parsed.stock_distribution !== null) {
    const stockPath = `${path}.stock_distribution`
    const stock = record(parsed.stock_distribution, stockPath)
    exact(stock, ["min_distributable_profit", "shares_per_existing_share_micros", "max_cumulative_expansion_micros", "cycles_between_proposals"], stockPath)
    result.stock_distribution = { min_distributable_profit: positiveMoney(stock.min_distributable_profit, `${stockPath}.min_distributable_profit`), shares_per_existing_share_micros: bounded(stock.shares_per_existing_share_micros, `${stockPath}.shares_per_existing_share_micros`, 1, 10000000), max_cumulative_expansion_micros: bounded(stock.max_cumulative_expansion_micros, `${stockPath}.max_cumulative_expansion_micros`, 1, 1000000000), cycles_between_proposals: bounded(stock.cycles_between_proposals, `${stockPath}.cycles_between_proposals`, 1, 65535) }
  }
  return result
}

function company(value: unknown, path: string): SimpleCompanyConfig {
  const parsed = record(value, path)
  exact(parsed, ["company", "kind", "generation", "finance", "preferences"], path)
  return { company: bounded(parsed.company, `${path}.company`, 0, 4294967295), kind: oneOf(parsed.kind, `${path}.kind`, ["Industrial", "Bank", "Insurance", "RealEstate"] as const), generation: generation(parsed.generation, `${path}.generation`), finance: finance(parsed.finance, `${path}.finance`), preferences: preferences(parsed.preferences, `${path}.preferences`) }
}

function simple(value: unknown, path: string): SimpleConfig {
  const parsed = record(value, path)
  exact(parsed, ["environment", "companies", "prehistory_periods", "settlement_cycle"], path)
  const environment = record(parsed.environment, `${path}.environment`)
  exact(environment, ["initial_change_bp", "persistence_bp", "noise"], `${path}.environment`)
  return { environment: { initial_change_bp: bounded(environment.initial_change_bp, `${path}.environment.initial_change_bp`, -2147483648, I32_MAX), persistence_bp: bounded(environment.persistence_bp, `${path}.environment.persistence_bp`, 0, 10000), noise: noise(environment.noise, `${path}.environment.noise`) }, companies: array(parsed.companies, `${path}.companies`).map((item, index) => company(item, `${path}.companies[${index}]`)), prehistory_periods: bounded(parsed.prehistory_periods, `${path}.prehistory_periods`, 0, 65535), settlement_cycle: oneOf(parsed.settlement_cycle, `${path}.settlement_cycle`, ["Monthly", "Quarterly", "HalfYear", "Annual"] as const) }
}

function frequency(value: unknown, path: string): ReportFrequency {
  if (value === "Quarterly") return value
  const parsed = record(value, path)
  exact(parsed, ["Monthly"], path)
  const monthly = record(parsed.Monthly, `${path}.Monthly`)
  exact(monthly, ["schedule"], `${path}.Monthly`)
  return { Monthly: { schedule: parseMonthlyReportSchedule(monthly.schedule, `${path}.Monthly.schedule`) } }
}

/** 只投影当前存档中实际出现的配置；跨股票与账户的不变量仍由 Rust 恢复门禁校验。 */
export function parseSetupCompanyOptions(source: Record<string, unknown>, path: string): SetupCompanyOptions {
  const result: SetupCompanyOptions = {}
  if ("company_system" in source) {
    const systemPath = `${path}.company_system`
    const system = record(source.company_system, systemPath)
    const mode = oneOf(system.mode, `${systemPath}.mode`, ["Simple", "Simulation"] as const)
    exact(system, mode === "Simple" ? ["mode", "config"] : ["mode"], systemPath)
    result.company_system = mode === "Simple" ? { mode, config: simple(system.config, `${systemPath}.config`) } : { mode }
  }
  if ("dividend_tax_mode" in source) result.dividend_tax_mode = oneOf(source.dividend_tax_mode, `${path}.dividend_tax_mode`, ["FlatWithholding", "AShareIndividual", "Exempt"] as const)
  if ("flat_withholding_bp" in source) result.flat_withholding_bp = bounded(source.flat_withholding_bp, `${path}.flat_withholding_bp`, 0, 10000)
  if ("par_value_per_share" in source) result.par_value_per_share = positiveMoney(source.par_value_per_share, `${path}.par_value_per_share`)
  for (const key of ["rights_offering_enabled", "issuer_repurchase_enabled", "auto_corporate_foundation"] as const) {
    if (key in source) result[key] = boolean(source[key], `${path}.${key}`)
  }
  if ("report_frequency" in source) result.report_frequency = frequency(source.report_frequency, `${path}.report_frequency`)
  if (result.company_system === undefined) {
    for (const key of ["dividend_tax_mode", "flat_withholding_bp", "par_value_per_share"] as const) {
      if (key in result) throw new SaveSchemaError(`${path}.${key}`, "要求 company_system 配置")
    }
    for (const key of ["rights_offering_enabled", "issuer_repurchase_enabled", "auto_corporate_foundation"] as const) {
      if (result[key] === true) throw new SaveSchemaError(`${path}.${key}`, "要求 company_system 配置")
    }
  } else {
    if (result.dividend_tax_mode === undefined) throw new SaveSchemaError(`${path}.dividend_tax_mode`, "是公司局必填字段")
    if (result.par_value_per_share === undefined) throw new SaveSchemaError(`${path}.par_value_per_share`, "是公司局必填字段")
    if (result.dividend_tax_mode === "FlatWithholding") {
      if (result.flat_withholding_bp === undefined) throw new SaveSchemaError(`${path}.flat_withholding_bp`, "是 FlatWithholding 必填字段")
    } else if ("flat_withholding_bp" in result) throw new SaveSchemaError(`${path}.flat_withholding_bp`, "仅允许 FlatWithholding 携带")
  }
  return result
}
