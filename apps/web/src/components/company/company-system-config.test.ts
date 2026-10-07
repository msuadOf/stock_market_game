import assert from "node:assert/strict";
import test from "node:test";
import { parseCompanySystemConfig } from "../../save/schema/company/system-config.ts";
import { DEFAULT_SETUP, DEFAULT_SEED } from "../../config/defaults.ts";
import { createCompanyInitialPreset } from "../../config/company-initial-preset.ts";

const config = {
  mode: "Simple",
  config: {
    environment: { initial_change_bp: 0, persistence_bp: 5000, noise: { monthly_bp: 100, quarterly_bp: 150, half_year_bp: 200, annual_bp: 300 } },
    companies: [{ company: "C-600101", kind: "Industrial", generation: { initial_revenue: "1000.00", initial_fixed_expense: "300.00", revenue_trend: { kind: "Fixed", annual_growth_bp: 100 }, demand_sensitivity_bp: 5000, revenue_noise: { monthly_bp: 0, quarterly_bp: 0, half_year_bp: 0, annual_bp: 0 }, fixed_expense_trend: { kind: "Fixed", annual_growth_bp: 0 }, fixed_expense_noise: { monthly_bp: 0, quarterly_bp: 0, half_year_bp: 0, annual_bp: 0 }, variable_expense: { rule: "RevenueRatio", ratio_bp: 12000, noise: { monthly_bp: 0, quarterly_bp: 0, half_year_bp: 0, annual_bp: 0 } } }, finance: { opening_lines: [{ account: "1122", side: "Debit", amount: "1000.00" }, { account: "4001", side: "Credit", amount: "1000.00" }], tax_policy: { version: 1, vat: { output_rate_bp: 0, input_rate_bp: 0, deductible_share_bp: 10000 }, income_tax: { rate_bp: 2500, loss_carryforward_years: 5 } }, summary_rule: "ReceivableRevenuePayableExpenses" }, preferences: { cash_dividend: null, stock_distribution: null } }],
    settlement_cycle: "Monthly", prehistory_periods: 24,
  },
};

test("Simple 新局参数包含独立营收开支及平衡财务期初，不生成公司现金", { timeout: 10000 }, () => {
  assert.deepEqual(parseCompanySystemConfig(config), config);
  assert.equal(parseCompanySystemConfig(config).config.companies[0].generation.variable_expense.rule, "RevenueRatio");
});

test("未实现模式及不完整、额外字段不降级 Simple", { timeout: 10000 }, () => {
  for (const input of [{ mode: "Simulation" }, { mode: "Other", config: config.config }, { ...config, schema: 3 }, { ...config, config: { ...config.config, cash: "0.00" } }]) assert.throws(() => parseCompanySystemConfig(input));
  for (const key of Object.keys(config.config.companies[0])) {
    const company: Record<string, unknown> = { ...config.config.companies[0] };
    delete company[key];
    assert.throws(() => parseCompanySystemConfig({ ...config, config: { ...config.config, companies: [company] } }), new RegExp(key));
  }
});

test("参数不接受非法元金额、重复公司或超出整数范围", { timeout: 10000 }, () => {
  for (const revenue of [1000, "1", "-0.00", "01.00", "-1.00"]) assert.throws(() => parseCompanySystemConfig({ ...config, config: { ...config.config, companies: [{ ...config.config.companies[0], generation: { ...config.config.companies[0].generation, initial_revenue: revenue } }] } }));
  assert.throws(() => parseCompanySystemConfig({ ...config, config: { ...config.config, companies: [config.config.companies[0], config.config.companies[0]] } }), /重复/);
  assert.throws(() => parseCompanySystemConfig({ ...config, config: { ...config.config, prehistory_periods: 65536 } }));
  assert.throws(() => parseCompanySystemConfig({ ...config, config: { ...config.config, environment: { ...config.config.environment, noise: { ...config.config.environment.noise, monthly_bp: -1 } } } }));
});

test("虚拟预设匹配发行人且只有账面期初，显式配置不因后续报价变化被覆盖", { timeout: 10000 }, () => {
  const preset = parseCompanySystemConfig(DEFAULT_SETUP.company_system);
  assert.deepEqual(preset.config.companies.map(company => company.company), DEFAULT_SETUP.stocks.map(stock => `C-${stock.code}`));
  for (const company of preset.config.companies) {
    assert.deepEqual(company.finance.opening_lines.map(line => line.account), ["simple_receivable", "4001"]);
    assert.ok(BigInt(company.generation.initial_revenue.replace(".", "")) > 0n);
    assert.ok(BigInt(company.generation.initial_fixed_expense.replace(".", "")) > 0n);
    const stock = DEFAULT_SETUP.stocks.find(stock => `C-${stock.code}` === company.company)!;
    const initial = createCompanyInitialPreset(stock, DEFAULT_SEED, 1);
    assert.equal(company.generation.initial_revenue, initial.initial_revenue);
    assert.equal(company.generation.initial_fixed_expense, initial.initial_fixed_expense);
    assert.equal(company.finance.opening_lines[0]!.amount, initial.initial_equity);
  }
  const repriced = structuredClone(DEFAULT_SETUP); repriced.stocks[0]!.initial_price = "999999999";
  assert.deepEqual(parseCompanySystemConfig(repriced.company_system), preset);
});
