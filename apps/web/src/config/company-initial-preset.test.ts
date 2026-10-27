import assert from "node:assert/strict";
import test from "node:test";
import { createCompanyInitialPreset } from "./company-initial-preset.ts";

function simpleTemplate() {
  const noise = { monthly_bp: 0, quarterly_bp: 0, half_year_bp: 0, annual_bp: 0 };
  const trend = { kind: "Fixed" as const, annual_growth_bp: 0 };
  return { mode: "Simple" as const, config: {
    environment: { initial_change_bp: 0, persistence_bp: 0, noise }, prehistory_periods: 0, settlement_cycle: "Monthly" as const,
    companies: [{ company: 17, kind: "Industrial" as const, generation: { initial_revenue: "1.00", initial_fixed_expense: "0.00", revenue_trend: trend, fixed_expense_trend: trend, revenue_noise: noise, fixed_expense_noise: noise, demand_sensitivity_bp: 0, variable_expense: { rule: "RevenueRatio" as const, ratio_bp: 0, noise } }, finance: { opening_lines: [{ account: "simple_receivable", side: "Debit" as const, amount: "1.00" }, { account: "4001", side: "Credit" as const, amount: "1.00" }], tax_policy: { version: 1, vat: { output_rate_bp: 0, input_rate_bp: 0, deductible_share_bp: 0 }, income_tax: { rate_bp: 0, loss_carryforward_years: 5 } }, summary_rule: "ReceivableRevenuePayableExpenses" as const, book_display: { investment_of_revenue_bp: 0 } }, preferences: { cash_dividend: null, stock_distribution: null } }],
  } };
}
const fixed = (value: string) => ({ min: value, max: value });
const basis = () => [{ company: 17, revenue: fixed("123.45"), fixedExpense: fixed("67.89"), variableExpense: null }];

test("显式期间金额直接采样，不按周期缩放，不改变kind/开账/偏好且不修改入参", () => {
  for (const kind of ["Industrial", "Bank", "Insurance", "RealEstate"] as const) {
    for (const cycle of ["Monthly", "Quarterly", "HalfYear", "Annual"] as const) {
      const template = simpleTemplate();
      const input = { ...template, config: { ...template.config, settlement_cycle: cycle, companies: template.config.companies.map(company => ({ ...company, kind })) } };
      const before = structuredClone(input);
      const output = createCompanyInitialPreset(input, 42n, basis());
      assert.equal(output.config.companies[0]?.generation.initial_revenue, "123.45");
      assert.equal(output.config.companies[0]?.generation.initial_fixed_expense, "67.89");
      assert.deepEqual(output.config.companies[0]?.finance, input.config.companies[0]?.finance);
      assert.deepEqual(output.config.companies[0]?.preferences, input.config.companies[0]?.preferences);
      assert.equal(output.config.companies[0]?.kind, kind);
      assert.deepEqual(input, before);
    }
  }
});
test("精确混合vector，完整u64范围和i128上界，非零min大额无JS精度损失", () => {
  const template = simpleTemplate();
  template.config.companies[0]!.company = 0;
  const output = createCompanyInitialPreset(template, 0n, [{ company: 0, revenue: { min: "0.00", max: "184467440737095516.15" }, fixedExpense: fixed("1701411834604692317316873037158841057.27"), variableExpense: null }]);
  assert.equal(output.config.companies[0]?.generation.initial_revenue, "4436879973638968.10");
  assert.equal(output.config.companies[0]?.generation.initial_fixed_expense, "1701411834604692317316873037158841057.27");
  assert.equal(createCompanyInitialPreset(simpleTemplate(), 42n, [{ ...basis()[0]!, revenue: fixed("9007199254740993.01") }]).config.companies[0]?.generation.initial_revenue, "9007199254740993.01");
});
test("拒绝非法区间、缺失/重复身份、不匹配expense规则与seed，不补默认", () => {
  for (const range of [{ min: "0.00", max: "184467440737095516.16" }, { min: "2.00", max: "1.00" }, { min: "01.00", max: "2.00" }, { min: "-1.00", max: "2.00" }, { min: "0.00", max: "1701411834604692317316873037158841057.28" }]) assert.throws(() => createCompanyInitialPreset(simpleTemplate(), 42n, [{ ...basis()[0]!, revenue: range }]), /revenue/);
  for (const seed of [-1n, 18446744073709551616n]) assert.throws(() => createCompanyInitialPreset(simpleTemplate(), seed, basis()), /seed/);
  for (const bases of [[], [...basis(), ...basis()], [{ ...basis()[0]!, company: 0 }], [{ ...basis()[0]!, variableExpense: fixed("1.00") }]]) assert.throws(() => createCompanyInitialPreset(simpleTemplate(), 42n, bases), /company|公司|variableExpense/);
  assert.throws(() => createCompanyInitialPreset({ mode: "Simulation" }, 42n, []), /Simulation/);
});

test("Growth独立字段vector、bases乱序与其他公司不影响采样", () => {
  const template = simpleTemplate();
  const company = template.config.companies[0]!;
  const growth = { rule: "Growth" as const, initial_amount: "0.00", trend: company.generation.revenue_trend, noise: company.generation.revenue_noise };
  const input = { ...template, config: { ...template.config, companies: [{ ...company, company: 4294967295, generation: { ...company.generation, variable_expense: growth } }, company] } };
  const full = { min: "0.00", max: "184467440737095516.15" };
  const bases = [{ company: 4294967295, revenue: full, fixedExpense: full, variableExpense: full }, ...basis()];
  const result = createCompanyInitialPreset(input, 18446744073709551615n, bases);
  const variable = result.config.companies[0]?.generation.variable_expense;
  assert.equal(variable?.rule, "Growth");
  if (variable?.rule !== "Growth") throw new Error("Fixture 必须为 Growth");
  assert.equal(variable.initial_amount, "63421138969744902.00");
  assert.deepEqual(result, createCompanyInitialPreset(input, 18446744073709551615n, [...bases].reverse()));
  const fixedVector = createCompanyInitialPreset(template, 42n, [{ ...basis()[0]!, fixedExpense: full }]);
  assert.equal(fixedVector.config.companies[0]?.generation.initial_fixed_expense, "77757689236522707.64");
  const duplicate = { ...template, config: { ...template.config, companies: [company, company] } };
  assert.throws(() => createCompanyInitialPreset(duplicate, 42n, basis()), /重复 company/);
  assert.throws(() => createCompanyInitialPreset(input, 42n, [{ ...bases[0]!, variableExpense: null }, ...basis()]), /variableExpense/);
});
