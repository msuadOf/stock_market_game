import assert from "node:assert/strict"
import test from "node:test"
import type { CompanySystemConfig } from "../../types/generated/CompanySystemConfig"
import { parseSetupCompanyOptions, SETUP_COMPANY_OPTION_KEYS } from "./setup-company-options.ts"

const noise = { monthly_bp: 0, quarterly_bp: 2, half_year_bp: 3, annual_bp: 4 }
function simple(): CompanySystemConfig {
  return structuredClone<CompanySystemConfig>({ mode: "Simple", config: {
    environment: { initial_change_bp: -200, persistence_bp: 9000, noise },
    prehistory_periods: 12, settlement_cycle: "Quarterly",
    companies: [{ company: 0, kind: "Industrial", generation: {
      initial_revenue: "10000.00", initial_fixed_expense: "2000.00",
      revenue_trend: { kind: "Fixed", annual_growth_bp: 1200 },
      fixed_expense_trend: { kind: "Persistent", annual_growth_min_bp: -200, annual_growth_max_bp: 300, duration_min_months: 2, duration_max_months: 12 },
      revenue_noise: noise, fixed_expense_noise: noise, demand_sensitivity_bp: 5000,
      variable_expense: { rule: "RevenueRatio", ratio_bp: 3000, noise },
    }, finance: { opening_lines: [{ account: "Cash", side: "Debit", amount: "100.00" }], tax_policy: { version: 1, vat: { output_rate_bp: 1300, input_rate_bp: 1300, deductible_share_bp: 10000 }, income_tax: { rate_bp: 2500, loss_carryforward_years: 5 } }, summary_rule: "ReceivableRevenuePayableExpenses", book_display: { investment_of_revenue_bp: 3000 } }, preferences: {
      cash_dividend: { target_payout_bp: 5000, min_distributable_profit: "9007199254740993", cycles_between_proposals: 1 },
      stock_distribution: { min_distributable_profit: "100", shares_per_existing_share_micros: 100000, max_cumulative_expansion_micros: 1000000, cycles_between_proposals: 2 },
    } }],
  } })
}
function options(): Record<string, unknown> {
  return { company_system: simple(), dividend_tax_mode: "FlatWithholding", flat_withholding_bp: 1000, rights_offering_enabled: true, issuer_repurchase_enabled: false, par_value_per_share: "100", auto_corporate_foundation: true, report_frequency: "Quarterly" }
}
const parse = (source: Record<string, unknown>) => parseSetupCompanyOptions(source, "setup")

test("公司选项逐字段保留且省略不生成默认值", () => {
  assert.equal(SETUP_COMPANY_OPTION_KEYS.length, 8)
  assert.deepEqual(parse({}), {})
  assert.deepEqual(parse(options()), options())
  for (const mode of ["AShareIndividual", "Exempt"]) {
    const source = options(); source.dividend_tax_mode = mode; delete source.flat_withholding_bp
    assert.deepEqual(parse(source), source)
  }
})

test("公司系统所有声明变体、四种周期与行业可解析", () => {
  assert.deepEqual(parse({ company_system: { mode: "Simulation" }, dividend_tax_mode: "Exempt", par_value_per_share: "100" }), { company_system: { mode: "Simulation" }, dividend_tax_mode: "Exempt", par_value_per_share: "100" })
  for (const settlement_cycle of ["Monthly", "Quarterly", "HalfYear", "Annual"] as const) {
    for (const kind of ["Industrial", "Bank", "Insurance", "RealEstate"] as const) {
      const system = simple()
      if (system.mode !== "Simple") throw new Error("Fixture 必须是 Simple")
      system.config.settlement_cycle = settlement_cycle
      const company = system.config.companies[0]
      if (company === undefined) throw new Error("Fixture 缺少公司")
      company.kind = kind
      company.generation.variable_expense = { rule: "Growth", initial_amount: "25.00", trend: { kind: "Fixed", annual_growth_bp: 100 }, noise }
      company.preferences = { cash_dividend: null, stock_distribution: null }
      const source = { ...options(), company_system: system }
      assert.deepEqual(parse(source), source)
    }
  }
})

test("月报两种排期、两个 preset 与两种 delay 全保留", () => {
  for (const delay of ["None", { Uniform: { max_days: 31 } }]) {
    for (const schedule of [{ Preset: { preset: "FirstDayEvening", delay } }, { Preset: { preset: "TenthDayEvening", delay } }, { Custom: { day: 28, second_of_day: 86399, delay } }]) {
      const source = { report_frequency: { Monthly: { schedule } } }
      assert.deepEqual(parse(source), source)
    }
  }
})

test("公司选项拒绝 null、错误类型、错误税务配对和残缺公司字段族", () => {
  for (const key of SETUP_COMPANY_OPTION_KEYS) assert.throws(() => parse({ ...options(), [key]: null }), new RegExp(`setup\\.${key}`))
  for (const [key, value] of [["dividend_tax_mode", "IndividualPublicMarket"], ["flat_withholding_bp", -1], ["flat_withholding_bp", 10001], ["flat_withholding_bp", 1.5], ["par_value_per_share", 100], ["par_value_per_share", "0"], ["rights_offering_enabled", 1], ["issuer_repurchase_enabled", "true"]]) assert.throws(() => parse({ ...options(), [key]: value }), new RegExp(`setup\\.${key}`))
  for (const key of ["dividend_tax_mode", "flat_withholding_bp", "par_value_per_share", "company_system"]) {
    const source = options(); delete source[key]
    assert.throws(() => parse(source), /setup\./)
  }
  for (const mode of ["Exempt", "AShareIndividual"]) assert.throws(() => parse({ ...options(), dividend_tax_mode: mode }), /flat_withholding_bp/)
  assert.deepEqual(parse({ rights_offering_enabled: false, issuer_repurchase_enabled: false, auto_corporate_foundation: false }), { rights_offering_enabled: false, issuer_repurchase_enabled: false, auto_corporate_foundation: false })
})

test("所有嵌套对象拒绝未知字段及缺少必填字段", () => {
  function visit(value: unknown, path: string[], source: Record<string, unknown>): void {
    if (typeof value !== "object" || value === null) return
    if (Array.isArray(value)) { value.forEach((entry, index) => visit(entry, [...path, String(index)], source)); return }
    const entry = structuredClone(source)
    let cursor: Record<string, unknown> = entry
    for (const part of path) {
      const next: unknown = cursor[part]
      if (typeof next !== "object" || next === null) throw new Error("Fixture 路径无效")
      cursor = next as Record<string, unknown>
    }
    cursor.unexpected = true
    assert.throws(() => parse(entry), /不是允许字段/)
    for (const key of Object.keys(value)) {
      const missing = structuredClone(entry)
      let nested: Record<string, unknown> = missing
      for (const part of path) nested = nested[part] as Record<string, unknown>
      delete nested.unexpected; delete nested[key]
      assert.throws(() => parse(missing), /setup\./)
      visit((value as Record<string, unknown>)[key], [...path, key], source)
    }
  }
  const source = options()
  visit(source.company_system, ["company_system"], source)
})

test("月报拒绝非法变体、范围和额外字段", () => {
  for (const schedule of [{ Preset: { preset: "Other", delay: "None" } }, { Custom: { day: 0, second_of_day: 0, delay: "None" } }, { Custom: { day: 29, second_of_day: 0, delay: "None" } }, { Custom: { day: 1, second_of_day: 86400, delay: "None" } }, { Preset: { preset: "FirstDayEvening", delay: { Uniform: { max_days: 0 } } } }, { Preset: { preset: "FirstDayEvening", delay: { Uniform: { max_days: 32, extra: 1 } } } }]) assert.throws(() => parse({ report_frequency: { Monthly: { schedule } } }), /setup\.report_frequency/)
  assert.throws(() => parse({ report_frequency: { Monthly: { schedule: { Preset: { preset: "FirstDayEvening", delay: "None" } }, extra: 1 } } }), /extra/)
})

function setPath(source: Record<string, unknown>, path: string, value: unknown): void {
  const parts = path.split(".")
  const last = parts.pop()
  if (last === undefined) throw new Error("Fixture 路径不能为空")
  let cursor = source
  for (const part of parts) {
    const next: unknown = cursor[part]
    if (typeof next !== "object" || next === null) throw new Error("Fixture 路径无效")
    cursor = next as Record<string, unknown>
  }
  cursor[last] = value
}

test("Simple 的整数范围、身份与 Money 边界显式拒绝", () => {
  const base = "company_system.config"
  for (const [field, value] of [
    ["environment.initial_change_bp", -2147483649], ["environment.initial_change_bp", 2147483648],
    ["environment.persistence_bp", -1], ["environment.persistence_bp", 10001],
    ["environment.noise.monthly_bp", -1], ["environment.noise.annual_bp", 2147483648],
    ["prehistory_periods", 65536], ["prehistory_periods", -1], ["settlement_cycle", "Daily"],
    ["companies.0.company", "0"], ["companies.0.company", 4294967296], ["companies.0.company", -1],
    ["companies.0.kind", "Other"], ["companies.0.generation.demand_sensitivity_bp", 2147483648],
    ["companies.0.generation.initial_revenue", 100],
    ["companies.0.generation.revenue_trend.annual_growth_bp", -10001],
    ["companies.0.generation.fixed_expense_trend.duration_min_months", 0],
    ["companies.0.generation.fixed_expense_trend.duration_max_months", 65536],
    ["companies.0.generation.fixed_expense_trend.annual_growth_max_bp", -201],
    ["companies.0.generation.variable_expense.rule", "Other"],
    ["companies.0.finance.book_display.investment_of_revenue_bp", 10001],
    ["companies.0.preferences.cash_dividend.target_payout_bp", 0],
    ["companies.0.preferences.cash_dividend.target_payout_bp", 10001],
    ["companies.0.preferences.cash_dividend.min_distributable_profit", "0"],
    ["companies.0.preferences.cash_dividend.min_distributable_profit", "9223372036854775808"],
    ["companies.0.preferences.cash_dividend.cycles_between_proposals", 65536],
    ["companies.0.preferences.stock_distribution.shares_per_existing_share_micros", 10000001],
    ["companies.0.preferences.stock_distribution.max_cumulative_expansion_micros", 1000000001],
  ] as const) {
    const source = options()
    setPath(source, `${base}.${field}`, value)
    assert.throws(() => parse(source), (error: unknown) => error instanceof Error && error.message.includes(`setup.${base}.${field}`.replace("companies.0.", "companies[0].")))
  }
  assert.throws(() => parse({ ...options(), company_system: { mode: "Simulation", config: {} } }), /config/)
  assert.throws(() => parse({ ...options(), company_system: { mode: "Unknown" } }), /mode/)
})

test("Simple finance 使用 Rust 整数字段宽度且不吞税务或 generation 非法输入", () => {
  for (const [field, value] of [
    ["finance.tax_policy.version", 4294967296],
    ["finance.tax_policy.vat.output_rate_bp", 2147483648],
    ["finance.tax_policy.income_tax.loss_carryforward_years", 65536],
    ["generation.initial_revenue", "1701411834604692317316873037158841057.28"],
    ["generation.variable_expense.ratio_bp", -1],
  ] as const) {
    const source = options()
    setPath(source, `company_system.config.companies.0.${field}`, value)
    assert.throws(() => parse(source), (error: unknown) => error instanceof Error && error.message.includes(`setup.company_system.config.companies[0].${field}`))
  }
})

test("opening_lines 保留 Rust serde 可解码的正负 i128 边界金额", () => {
  for (const value of ["1701411834604692317316873037158841057.27", "-1701411834604692317316873037158841057.27", "0.00"]) {
    const source = options()
    setPath(source, "company_system.config.companies.0.finance.opening_lines.0.amount", value)
    const result = parse(source)
    assert.equal(result.company_system?.mode, "Simple")
    if (result.company_system?.mode !== "Simple") throw new Error("Fixture 必须是 Simple")
    assert.equal(result.company_system.config.companies[0]?.finance.opening_lines[0]?.amount, value)
  }
})

test("opening_lines 拒绝正负溢出并保留精确 amount 错误路径", () => {
  for (const value of ["1701411834604692317316873037158841057.28", "-1701411834604692317316873037158841057.28", "-1701411834604692317316873037158841057.29"]) {
    const source = options()
    setPath(source, "company_system.config.companies.0.finance.opening_lines.0.amount", value)
    assert.throws(() => parse(source), (error: unknown) => error instanceof Error && error.message.includes("setup.company_system.config.companies[0].finance.opening_lines[0].amount") && error.message.includes("i128"))
  }
})

test("opening_lines 拒绝非当前存档两位小数金额格式", () => {
  for (const value of [" 1.00", "+1.00", "1", "1.0"]) {
    const source = options()
    setPath(source, "company_system.config.companies.0.finance.opening_lines.0.amount", value)
    assert.throws(() => parse(source), (error: unknown) => error instanceof Error && error.message.includes("setup.company_system.config.companies[0].finance.opening_lines[0].amount"))
  }
})
