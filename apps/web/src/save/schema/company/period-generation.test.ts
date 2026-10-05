import assert from "node:assert/strict";
import test from "node:test";
import { parsePeriodGenerationParameters, parsePeriodGenerationState, parsePeriodChangeExplanation } from "./period-generation.ts";

const noise = { monthly_bp: 10, quarterly_bp: 20, half_year_bp: 30, annual_bp: 40 };
const fixed = { kind: "Fixed", annual_growth_bp: 1200 };
const parameters = { initial_revenue: "1000.00", initial_fixed_expense: "300.00", revenue_trend: fixed, fixed_expense_trend: { kind: "Persistent", annual_growth_min_bp: -1000, annual_growth_max_bp: 300, duration_min_months: 2, duration_max_months: 24 }, revenue_noise: noise, fixed_expense_noise: noise, demand_sensitivity_bp: 10000, variable_expense: { rule: "RevenueRatio", ratio_bp: 12000, noise } };

test("年化趋势、自然月持续时长及各结算跨度扰动分别配置，不把年增长当每期增长", { timeout: 10000 }, () => {
  assert.deepEqual(parsePeriodGenerationParameters(parameters), parameters);
  assert.throws(() => parsePeriodGenerationParameters({ ...parameters, revenue_growth_bp: 1200 }), /revenue_growth_bp/);
  assert.throws(() => parsePeriodGenerationParameters({ ...parameters, fixed_expense_trend: { ...parameters.fixed_expense_trend, duration_min_months: 0 } }), /duration_min_months/);
  assert.throws(() => parsePeriodGenerationParameters({ ...parameters, revenue_noise: { ...noise, annual_bp: -1 } }), /annual_bp/);
  assert.throws(() => parsePeriodGenerationParameters({ ...parameters, variable_expense: { rule: "RevenueRatio", ratio_bp: 20, noise } }), /区间/);
});

test("持续趋势进度与必要 nullable 随机状态保存，无旧月度fallback", { timeout: 10000 }, () => {
  const value = { amounts: { revenue: "1000.00", fixed_expense: "300.00", variable_expense: "400.00" }, rng: { state: "18446744073709551615" }, revenue_trend: { annual_growth_bp: 1200, remaining_months: 2 }, fixed_expense_trend: { annual_growth_bp: 300, remaining_months: 24 }, variable_expense_trend: null };
  assert.deepEqual(parsePeriodGenerationState(value), value);
  const missing = { ...value }; Reflect.deleteProperty(missing, "variable_expense_trend");
  assert.throws(() => parsePeriodGenerationState(missing), /variable_expense_trend/);
  assert.throws(() => parsePeriodGenerationState({ ...value, rng: { state: 1 } }), /state/);
});

test("季度解释保留期间内各趋势段，段数时间必须覆盖本次完整跨度", { timeout: 10000 }, () => {
  const value = { previous: { revenue: "1000.00", fixed_expense: "300.00", variable_expense: "400.00" }, cycle: "Quarterly", environment_change_bp: 0, demand_contribution_bp: 0, revenue_segments: [{ annual_growth_bp: 1200, months: 2 }, { annual_growth_bp: -1000, months: 1 }], fixed_expense_segments: [{ annual_growth_bp: 300, months: 3 }], variable_expense_segments: [], revenue_noise_bp: -10, fixed_expense_noise_bp: 10, variable_expense_noise_bp: 10, restart_revenue: null, restart_source: null };
  assert.deepEqual(parsePeriodChangeExplanation(value), value);
  assert.throws(() => parsePeriodChangeExplanation({ ...value, revenue_segments: [{ annual_growth_bp: 1200, months: 2 }] }), /跨度/);
  assert.throws(() => parsePeriodChangeExplanation({ ...value, cycle: "Other" }), /cycle/);
  assert.throws(() => parsePeriodChangeExplanation({ ...value, restart_revenue: "1.00" }), /复业/);
});

test("期间配置保持全部必填、非负金额和两种变动开支规则互斥", { timeout: 10000 }, () => {
  for (const key of Object.keys(parameters)) {
    const missing: Record<string, unknown> = { ...parameters }; delete missing[key];
    assert.throws(() => parsePeriodGenerationParameters(missing), new RegExp(key));
  }
  assert.throws(() => parsePeriodGenerationParameters({ ...parameters, initial_fixed_expense: "-1.00" }), /initial_fixed_expense/);
  const growth = { ...parameters, variable_expense: { rule: "Growth", initial_amount: "0.00", trend: fixed, noise } };
  assert.deepEqual(parsePeriodGenerationParameters(growth), growth);
  assert.throws(() => parsePeriodGenerationParameters({ ...growth, variable_expense: { ...growth.variable_expense, ratio_bp: 4000 } }), /ratio_bp/);
});
