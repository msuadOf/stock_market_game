import assert from "node:assert/strict";
import test from "node:test";
import { parseCompanySystemState, validateCompanySystemSession } from "./system.ts";
import { DEFAULT_SETUP } from "../../../config/defaults.ts";
import { books as baseBooks, position } from "./books/tax-owner-test-fixture.ts";
import { simpleAccountChart } from "./simple-chart.ts";

const books = { ...baseBooks, chart: simpleAccountChart("Industrial") };

function fixture() {
  const amounts = { revenue: "1000.00", fixed_expense: "300.00", variable_expense: "400.00" };
  const financeConfig = { opening_lines: [{ account: "1122", side: "Debit", amount: "1000.00" }, { account: "4001", side: "Credit", amount: "1000.00" }], tax_policy: { version: 1, vat: { output_rate_bp: 0, input_rate_bp: 0, deductible_share_bp: 10000 }, income_tax: { rate_bp: 2500, loss_carryforward_years: 5 } }, summary_rule: "ReceivableRevenuePayableExpenses" };
  return {
    issuers: { "C-600101": { id: "C-600101", name: "虚拟公司", industry: "listed-simple", kind: "Industrial", listed_stock: "600101", issued_shares: "1000", group_parent: null } },
    implementation: { mode: "Simple", state: {
      config: { environment: { initial_change_bp: 0, persistence_bp: 0, noise: { monthly_bp: 0, quarterly_bp: 0, half_year_bp: 0, annual_bp: 0 } }, companies: [{ company: "C-600101", kind: "Industrial", generation: { initial_revenue: "1000.00", initial_fixed_expense: "300.00", revenue_trend: { kind: "Fixed", annual_growth_bp: 0 }, demand_sensitivity_bp: 0, revenue_noise: { monthly_bp: 0, quarterly_bp: 0, half_year_bp: 0, annual_bp: 0 }, fixed_expense_trend: { kind: "Fixed", annual_growth_bp: 0 }, fixed_expense_noise: { monthly_bp: 0, quarterly_bp: 0, half_year_bp: 0, annual_bp: 0 }, variable_expense: { rule: "RevenueRatio", ratio_bp: 4000, noise: { monthly_bp: 0, quarterly_bp: 0, half_year_bp: 0, annual_bp: 0 } } }, finance: financeConfig }], prehistory_periods: 0, settlement_cycle: "Monthly" },
      history_start: "2030-01-01", advanced_through: "2030-01-31", environment_change_bp: 0, environment_rng: { state: "18446744073709551615" },
      companies: { "C-600101": { generation: { amounts, rng: { state: "19" }, revenue_trend: { annual_growth_bp: 0, remaining_months: 0 }, fixed_expense_trend: { annual_growth_bp: 0, remaining_months: 0 }, variable_expense_trend: null }, finance: { company: "C-600101", kind: "Industrial", config: financeConfig, books, closing: { versions: [], restatements: [] }, opening_date: "2029-12-31", as_of: "2030-01-31", last_month: "2030-01", next_event_id: "2", income_tax_position: position, recognized_periods: [["2030-01-01", "2030-01-31"]], dividends: {}, stock_distributions: {}, legal_facts: null }, pending_restart: null } },
      history: [{ company: "C-600101", period_start: "2030-01-01", period_end: "2030-01-31", amounts, explanation: { previous: amounts, cycle: "Monthly", environment_change_bp: 0, demand_contribution_bp: 0, revenue_segments: [{ annual_growth_bp: 0, months: 1 }], fixed_expense_segments: [{ annual_growth_bp: 0, months: 1 }], variable_expense_segments: [], revenue_noise_bp: 0, fixed_expense_noise_bp: 0, variable_expense_noise_bp: 0, restart_revenue: null, restart_source: null } }],
    } },
  };
}

test("Simple 保存月度金额与独立汇总财务，不保存旧指标或客户仿真", { timeout: 10000 }, () => { assert.deepEqual(parseCompanySystemState(fixture()), fixture()); });

test("Simple 拒绝缺失 nullable、旧字段和不匹配模式", { timeout: 10000 }, () => {
  for (const key of ["pending_restart", "generation", "finance"]) {
    const value = fixture(); Reflect.deleteProperty(value.implementation.state.companies["C-600101"], key);
    assert.throws(() => parseCompanySystemState(value), new RegExp(key));
  }
  for (const field of ["cash", "facts", "customers"]) {
    const value = fixture(); Reflect.set(value.implementation.state, field, {});
    assert.throws(() => parseCompanySystemState(value), new RegExp(field));
  }
  const other = fixture(); Reflect.set(other.implementation, "mode", "Simulation"); assert.throws(() => parseCompanySystemState(other), /模式/);
});

test("Simple 交叉核验发行人、月史连续性、最新状态和日期", { timeout: 10000 }, () => {
  const identity = fixture(); identity.issuers["C-600101"].id = "Other"; assert.throws(() => parseCompanySystemState(identity), /身份/);
  const future = fixture(); future.implementation.state.history[0].period_end = "2030-02-28"; assert.throws(() => parseCompanySystemState(future), /期间|未来/);
  const missing = fixture(); missing.implementation.state.history = []; assert.throws(() => parseCompanySystemState(missing), /历史/);
  const drift = fixture(); drift.implementation.state.companies["C-600101"].generation.amounts = { ...drift.implementation.state.companies["C-600101"].generation.amounts, revenue: "2000.00" }; assert.throws(() => parseCompanySystemState(drift), /最新/);
});

test("还原遵守 Rust i32 中间溢出及 i128 乘积约束，复业不能跳过增长校验", { timeout: 10000 }, () => {
  const intermediate = fixture();
  const parameters = intermediate.implementation.state.config.companies[0].generation;
  parameters.revenue_trend.annual_growth_bp = 2147483647; parameters.demand_sensitivity_bp = 10000;
  intermediate.implementation.state.companies["C-600101"].generation.revenue_trend.annual_growth_bp = 2147483647;
  Object.assign(intermediate.implementation.state.history[0].explanation, { environment_change_bp: 1, demand_contribution_bp: 1, revenue_segments: [{ annual_growth_bp: 2147483647, months: 1 }] });
  assert.throws(() => parseCompanySystemState(intermediate), /i32/);
  const restarted = fixture();
  Object.assign(restarted.implementation.state.config.companies[0].generation, { initial_revenue: "0.00", revenue_trend: { kind: "Fixed", annual_growth_bp: -10000 } });
  restarted.implementation.state.config.companies[0].generation.revenue_noise.monthly_bp = 1;
  restarted.implementation.state.companies["C-600101"].generation.revenue_trend.annual_growth_bp = -10000;
  restarted.implementation.state.history[0].explanation.previous = { revenue: "0.00", fixed_expense: "300.00", variable_expense: "0.00" };
  Reflect.set(restarted.implementation.state.history[0].explanation, "restart_revenue", "1.00");
  Reflect.set(restarted.implementation.state.history[0].explanation, "restart_source", "虚拟复业");
  restarted.implementation.state.history[0].explanation.revenue_segments = [{ annual_growth_bp: -10000, months: 1 }];
  restarted.implementation.state.history[0].explanation.revenue_noise_bp = -1;
  assert.throws(() => parseCompanySystemState(restarted), /负因子/);
  const product = fixture();
  product.implementation.state.config.companies[0].generation.initial_revenue = "1701411834604692317316873037158841057.27";
  assert.throws(() => parseCompanySystemState(product), /i128/);
});

test("发行人集合拒绝未知母公司、循环与股票重复映射", { timeout: 10000 }, () => {
  const absent = fixture(); Reflect.set(absent.issuers["C-600101"], "group_parent", "absent"); assert.throws(() => parseCompanySystemState(absent), /母公司/);
  const cycle = fixture(); Reflect.set(cycle.issuers["C-600101"], "group_parent", "C-600101"); assert.throws(() => parseCompanySystemState(cycle), /循环/);
  const duplicate = fixture(); Reflect.set(duplicate.issuers, "Other", { ...duplicate.issuers["C-600101"], id: "Other" }); assert.throws(() => parseCompanySystemState(duplicate), /股票.*重复/);
});

test("发行人类别必须与 Simple 财务类别一致，不能只替换成其他行业科目表", { timeout: 10000 }, () => {
  const value = fixture(); Reflect.set(value.issuers["C-600101"], "kind", "Bank"); Reflect.set(value.implementation.state.config.companies[0]!, "kind", "Bank");
  assert.throws(() => parseCompanySystemState(value), /财务身份、类别/);
  const configDrift = fixture(); Reflect.set(configDrift.implementation.state.config.companies[0]!, "kind", "Insurance");
  assert.throws(() => parseCompanySystemState(configDrift), /配置与发行人类别/);
});

test("四种明确配置的类别可恢复，不从股票代码推断或缺字段默认工业", { timeout: 10000 }, () => {
  for (const kind of ["Industrial", "Bank", "Insurance", "RealEstate"] as const) {
    const value = fixture(); value.issuers["C-600101"].name = "虚构上市公司600101";
    const config = value.implementation.state.config.companies[0]!;
    Reflect.set(config, "kind", kind); Reflect.set(value.issuers["C-600101"], "kind", kind);
    const finance = value.implementation.state.companies["C-600101"].finance;
    Reflect.set(finance, "kind", kind); Reflect.set(finance, "books", { ...finance.books, chart: simpleAccountChart(kind) });
    config.finance.opening_lines[0]!.account = "simple_receivable";
    const parsed = parseCompanySystemState(value);
    assert.deepEqual(parsed, value);
    assert.doesNotThrow(() => validateCompanySystemSession(parsed, { stocks: [{ ...DEFAULT_SETUP.stocks[0]!, total_shares: "1000" }], company_system: { mode: "Simple", config: parsed.implementation.state.config } }, "2030-02-01"));
  }
  const missing = fixture(); Reflect.deleteProperty(missing.implementation.state.config.companies[0]!, "kind");
  assert.throws(() => parseCompanySystemState(missing), /kind/);
  const invalid = fixture(); Reflect.set(invalid.implementation.state.config.companies[0]!, "kind", "Other");
  assert.throws(() => parseCompanySystemState(invalid), /kind/);
});

test("历史公司排序按 Rust UTF8 字节，而非 JavaScript UTF16 顺序", { timeout: 10000 }, () => {
  const value = fixture();
  const low = "\ue000", high = "\u{10000}";
  const issuer = value.issuers["C-600101"];
  const config = value.implementation.state.config.companies[0];
  const company = value.implementation.state.companies["C-600101"];
  const candidate = value.implementation.state.history[0];
  Reflect.set(value, "issuers", { [low]: { ...issuer, id: low }, [high]: { ...issuer, id: high, listed_stock: "600102" } });
  value.implementation.state.config.companies = [{ ...config, company: low }, { ...config, company: high }];
  Reflect.set(value.implementation.state, "companies", { [low]: { ...company, finance: { ...company.finance, company: low } }, [high]: { ...company, finance: { ...company.finance, company: high } } });
  value.implementation.state.history = [{ ...candidate, company: low }, { ...candidate, company: high }];
  assert.deepEqual(parseCompanySystemState(value), value);
  value.implementation.state.history.reverse();
  assert.throws(() => parseCompanySystemState(value), /顺序/);
});

test("完整槽连接当前时钟与唯一股票发行人，不接受私改股本或推进日", { timeout: 10000 }, () => {
  const value = fixture(); value.issuers["C-600101"].name = "虚构上市公司600101";
  const system = parseCompanySystemState(value);
  const setup = { stocks: [{ ...DEFAULT_SETUP.stocks[0]!, total_shares: "1000" }], company_system: { mode: "Simple" as const, config: system.implementation.state.config } };
  assert.doesNotThrow(() => validateCompanySystemSession(system, setup, "2030-02-01"));
  assert.throws(() => validateCompanySystemSession(system, setup, "2030-02-02"), /推进日期/);
  assert.throws(() => validateCompanySystemSession(system, { ...setup, stocks: [{ ...setup.stocks[0]!, total_shares: "1001" }] }, "2030-02-01"), /发行人/);
});

test("四种自然结算周期保留同长度基准及历史，不接受月金额冒称全年", { timeout: 10000 }, () => {
  for (const [cycle, months, end] of [["Monthly", 1, "2030-01-31"], ["Quarterly", 3, "2030-03-31"], ["HalfYear", 6, "2030-06-30"], ["Annual", 12, "2030-12-31"]] as const) {
    const value = fixture();
    Reflect.set(value.implementation.state.config, "settlement_cycle", cycle);
    value.implementation.state.advanced_through = end;
    const company = value.implementation.state.companies["C-600101"];
    company.finance.as_of = end; company.finance.last_month = end.slice(0, 7); company.finance.recognized_periods = [["2030-01-01", end]];
    const candidate = value.implementation.state.history[0]; candidate.period_end = end;
    Reflect.set(candidate.explanation, "cycle", cycle);
    candidate.explanation.revenue_segments[0]!.months = months; candidate.explanation.fixed_expense_segments[0]!.months = months;
    assert.deepEqual(parseCompanySystemState(value), value);
    candidate.period_end = "2030-01-31";
    if (months !== 1) assert.throws(() => parseCompanySystemState(value), /期间/);
  }
});

test("已保存趋势进度超出配置或比例开支携带金额趋势时拒绝", { timeout: 10000 }, () => {
  const progress = fixture(); progress.implementation.state.companies["C-600101"].generation.revenue_trend.remaining_months = 1;
  assert.throws(() => parseCompanySystemState(progress), /自然月进度/);
  const variable = fixture(); Reflect.set(variable.implementation.state.companies["C-600101"].generation, "variable_expense_trend", { annual_growth_bp: 0, remaining_months: 0 });
  assert.throws(() => parseCompanySystemState(variable), /比例开支/);
});

test("财务开账日应是前史开始前一天，不凭空缩短或扩大报告历史", { timeout: 10000 }, () => {
  const value = fixture();
  value.implementation.state.companies["C-600101"].finance.opening_date = "2029-11-30";
  value.implementation.state.companies["C-600101"].finance.recognized_periods.unshift(["2029-12-01", "2029-12-31"]);
  assert.throws(() => parseCompanySystemState(value), /开账日期/);
});

test("公司日期服从 CivilDate 验证窗，不接受不存在的仿真年代", { timeout: 10000 }, () => {
  for (const date of ["1899-12-31", "2200-01-01", "0000-01-01"]) {
    const value = fixture(); value.implementation.state.history_start = date;
    assert.throws(() => parseCompanySystemState(value), /CivilDate/);
  }
});

test("完整槽拒绝晚于 Session 日期的实际公司分红付款", { timeout: 10000 }, () => {
  const value = fixture();
  value.issuers["C-600101"].name = "虚构上市公司600101";
  value.implementation.state.companies["C-600101"].finance.dividends = {
    "plan-1": { payments: { "payment-1": { paid_on: "2030-02-02" } } },
  } as never;
  const system = value as unknown as ReturnType<typeof parseCompanySystemState>;
  const setup = { stocks: [{ ...DEFAULT_SETUP.stocks[0]!, total_shares: "1000" }], company_system: { mode: "Simple" as const, config: system.implementation.state.config } };
  assert.throws(() => validateCompanySystemSession(system, setup, "2030-02-01"), /实际付款日期晚于当前会话日期/);
});
