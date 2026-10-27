import assert from "node:assert/strict";
import test from "node:test";
import { createSeedDraft, changeSeedDraft, editCompanySystemDraft, parseSeedDraft, parseSessionSeed, changeCompanyPreferencesDraft, regenerateSeedDraft, changeSettlementCycleDraft } from "./seed-draft.ts";

test("seed 完整 u64、规范十进制边界，在创建前拒绝非法 seed", () => {
  assert.equal(parseSessionSeed("18446744073709551615"), 18446744073709551615n);
  for (const text of ["-1", "18446744073709551616", "01", "+1", " 1", "", "1.0", "0x10"]) assert.throws(() => parseSessionSeed(text), /seed/);
});
test("预览 seed 原值创建；seed 编辑不覆盖 custom 非法 raw JSON", () => {
  const draft = { seed: "42", origin: "preset" as const, companySystem: '{"mode":"Simulation"}' };
  const custom = editCompanySystemDraft(draft, "unfinished {");
  assert.deepEqual(changeSeedDraft(custom, "000"), { seed: "000", origin: "custom", companySystem: "unfinished {" });
  assert.equal(changeSeedDraft(draft, "99").companySystem, draft.companySystem);
  assert.throws(() => parseSeedDraft(custom), /JSON/);
});
test("Simulation 创建草稿显式不支持，非法 seed 先于运行失败", () => {
  assert.throws(() => createSeedDraft({ mode: "Simulation" }, 42n), /Simulation/);
  assert.throws(() => parseSeedDraft({ seed: "-1", origin: "custom", companySystem: "{}" }), /seed/);
});


function simpleTemplate() {
  const noise = { monthly_bp: 0, quarterly_bp: 0, half_year_bp: 0, annual_bp: 0 };
  const trend = { kind: "Fixed" as const, annual_growth_bp: 0 };
  return { mode: "Simple" as const, config: {
    environment: { initial_change_bp: 0, persistence_bp: 0, noise }, prehistory_periods: 0, settlement_cycle: "Monthly" as const,
    companies: [{ company: 17, kind: "Industrial" as const, generation: { initial_revenue: "1.00", initial_fixed_expense: "0.00", revenue_trend: trend, fixed_expense_trend: trend, revenue_noise: noise, fixed_expense_noise: noise, demand_sensitivity_bp: 0, variable_expense: { rule: "RevenueRatio" as const, ratio_bp: 0, noise } }, finance: { opening_lines: [{ account: "simple_receivable", side: "Debit" as const, amount: "1.00" }, { account: "4001", side: "Credit" as const, amount: "1.00" }], tax_policy: { version: 1, vat: { output_rate_bp: 0, input_rate_bp: 0, deductible_share_bp: 0 }, income_tax: { rate_bp: 0, loss_carryforward_years: 5 } }, summary_rule: "ReceivableRevenuePayableExpenses" as const, book_display: { investment_of_revenue_bp: 0 } }, preferences: { cash_dividend: null, stock_distribution: null } }],
  } };
}
test("合法 seed 预览与创建一致，偏好更新只按 numeric CompanyId 且不改金额", () => {
  const draft = createSeedDraft(simpleTemplate(), 18446744073709551615n);
  assert.equal(parseSeedDraft(draft).seed, 18446744073709551615n);
  const preferences = { cash_dividend: { target_payout_bp: 3000, min_distributable_profit: "100000000", cycles_between_proposals: 1 }, stock_distribution: null };
  const next = changeCompanyPreferencesDraft(draft, 17, preferences);
  const parsed = parseSeedDraft(next).companySystem;
  assert.equal(parsed.mode, "Simple");
  if (parsed.mode !== "Simple") throw new Error("Fixture 必须为 Simple");
  assert.deepEqual(parsed.config.companies[0]?.generation, simpleTemplate().config.companies[0]?.generation);
  assert.deepEqual(parsed.config.companies[0]?.preferences, preferences);
  assert.equal(next.origin, "custom"); assert.equal(next.seed, draft.seed);
  assert.throws(() => changeCompanyPreferencesDraft(draft, 0, preferences), /唯一存在/);
  assert.throws(() => changeCompanyPreferencesDraft(draft, "17" as never, preferences), /numeric/);
  assert.throws(() => changeCompanyPreferencesDraft(draft, 17, { ...preferences, cash_dividend: { ...preferences.cash_dividend, target_payout_bp: 0 } }), /target_payout_bp/);
});


test("显式重新生成只抽一次 seed，周期切换使用明确期间范围、保留seed且拒绝custom", () => {
  const draft = createSeedDraft(simpleTemplate(), 42n);
  const bases = [{ company: 17, revenue: { min: "20.00", max: "20.00" }, fixedExpense: { min: "1.00", max: "1.00" }, variableExpense: null }];
  let calls = 0;
  const next = regenerateSeedDraft(draft, bases, () => { calls++; return 77n; });
  assert.equal(calls, 1); assert.equal(parseSeedDraft(next).seed, 77n);
  const cycled = changeSettlementCycleDraft(next, "Quarterly", bases);
  assert.equal(cycled.seed, next.seed);
  const parsed = parseSeedDraft(cycled).companySystem;
  if (parsed.mode !== "Simple") throw new Error("Fixture 必须为 Simple");
  assert.equal(parsed.config.settlement_cycle, "Quarterly");
  assert.equal(parsed.config.companies[0]?.generation.initial_revenue, "20.00");
  assert.throws(() => changeSettlementCycleDraft(editCompanySystemDraft(next, next.companySystem), "Annual", bases), /本人编辑|custom/);
  assert.throws(() => regenerateSeedDraft({ ...draft, companySystem: "unfinished {" }, bases, () => { calls++; return 88n; }), /JSON/);
  assert.equal(calls, 1);
  assert.throws(() => regenerateSeedDraft(draft, bases, () => { throw new Error("熵失败"); }), /熵失败/);
  assert.equal(draft.seed, "42");
});
