import assert from "node:assert/strict";
import test from "node:test";
import { DEFAULT_SETUP } from "./defaults.ts";
import { changeCompanyPreferencesDraft, changeSeedDraft, changeSettlementCycleDraft, createSeedDraft, parseSessionSeed, regenerateSeedDraft } from "./seed-draft.ts";
import { createCompanyInitialPreset } from "./company-initial-preset.ts";
import { settlementMonths } from "../save/schema/company/period-generation.ts";

test("预览 seed 严格保留完整 u64，不接受非规范文本", { timeout: 10000 }, () => {
  assert.equal(parseSessionSeed("18446744073709551615"), 18446744073709551615n);
  assert.equal(parseSessionSeed("0"), 0n);
  for (const invalid of ["", "01", "-1", "1.0", " 1", "18446744073709551616"]) assert.throws(() => parseSessionSeed(invalid), /seed/);
});

test("预设切换四种结算周期保留展示 seed，并重建同跨度期初金额", { timeout: 10000 }, () => {
  const original = createSeedDraft(DEFAULT_SETUP, 9007199254740993n);
  for (const cycle of ["Monthly", "Quarterly", "HalfYear", "Annual"] as const) {
    const changed = changeSettlementCycleDraft(DEFAULT_SETUP, original, cycle);
    assert.equal(changed.seed, original.seed);
    assert.equal(changed.origin, "preset");
    const config = JSON.parse(changed.companySystem);
    assert.equal(config.config.settlement_cycle, cycle);
    for (const stock of DEFAULT_SETUP.stocks) {
      const expected = createCompanyInitialPreset(stock, 9007199254740993n, settlementMonths(cycle));
      const actual = config.config.companies.find((company: { company: string }) => company.company === `C-${stock.code}`);
      assert.equal(actual.generation.initial_revenue, expected.initial_revenue);
      assert.equal(actual.generation.initial_fixed_expense, expected.initial_fixed_expense);
      assert.deepEqual(actual.generation.revenue_trend, JSON.parse(original.companySystem).config.companies.find((company: { company: string }) => company.company === `C-${stock.code}`).generation.revenue_trend);
    }
  }
});

test("本人编辑配置不能被周期设置自动重用或覆盖", { timeout: 10000 }, () => {
  const custom = { ...createSeedDraft(DEFAULT_SETUP, 1n), origin: "custom" as const };
  const before = structuredClone(custom);
  assert.throws(() => changeSettlementCycleDraft(DEFAULT_SETUP, custom, "Annual"), /本人编辑.*同长度/);
  assert.deepEqual(custom, before);
});

test("预设与 seed 同步，但用户自定义不会被 seed 输入自动覆盖", { timeout: 10000 }, () => {
  const original = createSeedDraft(DEFAULT_SETUP, 1n);
  const changed = changeSeedDraft(DEFAULT_SETUP, original, "2");
  assert.equal(changed.seed, "2");
  assert.notEqual(changed.companySystem, original.companySystem);
  const custom = { ...original, origin: "custom" as const, companySystem: "用户正在编辑的 JSON" };
  assert.deepEqual(changeSeedDraft(DEFAULT_SETUP, custom, "2"), { ...custom, seed: "2" });
  assert.deepEqual(changeSeedDraft(DEFAULT_SETUP, original, ""), { ...original, seed: "" });
});

test("只有明确重新生成抽取新 seed，熵失败不产生半份草稿", { timeout: 10000 }, () => {
  const original = createSeedDraft(DEFAULT_SETUP, 1n);
  let reads = 0;
  const regenerated = regenerateSeedDraft(DEFAULT_SETUP, original, () => { reads++; return 9n; });
  assert.equal(reads, 1);
  assert.equal(regenerated.seed, "9");
  assert.equal(regenerated.origin, "preset");
  assert.deepEqual(regenerated, createSeedDraft(DEFAULT_SETUP, 9n));
  assert.throws(() => regenerateSeedDraft(DEFAULT_SETUP, original, () => { throw new Error("crypto 熵获取被拒绝"); }), /crypto 熵获取被拒绝/);
  assert.equal(original.seed, "1");
});

test("公司偏好编辑直接写入配置 JSON，不重建预设、不覆盖其他公司", { timeout: 10000 }, () => {
  const original = createSeedDraft(DEFAULT_SETUP, 7n);
  const before = JSON.parse(original.companySystem);
  const preferences = {
    cash_dividend: { target_payout_bp: 3000, min_distributable_profit: "50000000", cycles_between_proposals: 2 },
    stock_distribution: null,
  };
  const changed = changeCompanyPreferencesDraft(original, "C-600101", preferences);
  assert.equal(changed.seed, original.seed, "偏好编辑不得改动 seed");
  assert.equal(changed.origin, "custom", "偏好编辑标记本人配置，不被后续 seed/周期操作覆盖");
  const after = JSON.parse(changed.companySystem);
  assert.deepEqual(after.config.companies.find((company: { company: string }) => company.company === "C-600101").preferences, preferences);
  const other = after.config.companies.find((company: { company: string }) => company.company !== "C-600101");
  const otherBefore = before.config.companies.find((company: { company: string }) => company.company === other.company);
  assert.deepEqual(other.preferences, otherBefore.preferences, "其他公司的偏好保持不变");
  assert.equal(after.config.companies.find((company: { company: string }) => company.company === "C-600101").generation.initial_revenue, before.config.companies.find((company: { company: string }) => company.company === "C-600101").generation.initial_revenue, "偏好编辑不得重建期初金额");
  // 清空偏好：未配置=不自动产生方案。
  const cleared = changeCompanyPreferencesDraft(changed, "C-600101", { cash_dividend: null, stock_distribution: null });
  assert.deepEqual(JSON.parse(cleared.companySystem).config.companies.find((company: { company: string }) => company.company === "C-600101").preferences, { cash_dividend: null, stock_distribution: null });
});

test("偏好编辑后改 seed 与重新生成预设不丢偏好（origin 透传）", { timeout: 10000 }, () => {
  const original = createSeedDraft(DEFAULT_SETUP, 1n);
  const preferences = {
    cash_dividend: null,
    stock_distribution: { min_distributable_profit: "1000000", shares_per_existing_share_micros: 100000, max_cumulative_expansion_micros: 50000000, cycles_between_proposals: 4 },
  };
  const edited = changeCompanyPreferencesDraft(original, "C-600101", preferences);
  const readPreferences = (draft: SeedDraft) => JSON.parse(draft.companySystem).config.companies.find((company: { company: string }) => company.company === "C-600101").preferences;
  assert.deepEqual(readPreferences(changeSeedDraft(DEFAULT_SETUP, edited, "42")), preferences, "改 seed 不重建本人配置，偏好原样保留");
  const regenerated = regenerateSeedDraft(DEFAULT_SETUP, edited, () => 9n);
  assert.deepEqual(readPreferences(regenerated), preferences, "重新生成预设沿草稿配置克隆，偏好不丢");
  assert.equal(regenerated.origin, "preset");
});

test("公司偏好编辑对未知公司与非法数值显式拒绝且不改草稿", { timeout: 10000 }, () => {
  const original = createSeedDraft(DEFAULT_SETUP, 1n);
  assert.throws(() => changeCompanyPreferencesDraft(original, "C-unknown", { cash_dividend: null, stock_distribution: null }), /不在当前配置中/);
  assert.throws(() => changeCompanyPreferencesDraft(original, "C-600101", {
    cash_dividend: { target_payout_bp: 0, min_distributable_profit: "50000000", cycles_between_proposals: 1 },
    stock_distribution: null,
  }), /target_payout_bp/);
  assert.throws(() => changeCompanyPreferencesDraft(original, "C-600101", {
    cash_dividend: null,
    stock_distribution: { min_distributable_profit: "0", shares_per_existing_share_micros: 100000, max_cumulative_expansion_micros: 1000000, cycles_between_proposals: 1 },
  }), /min_distributable_profit/);
  assert.throws(() => changeCompanyPreferencesDraft({ ...original, companySystem: "not-json" }, "C-600101", { cash_dividend: null, stock_distribution: null }), /JSON|公司系统/);
  const before = structuredClone(original);
  try { changeCompanyPreferencesDraft(original, "C-unknown", { cash_dividend: null, stock_distribution: null }); } catch { /* 已验证显式抛错 */ }
  assert.deepEqual(original, before, "失败的编辑不留下半份草稿");
});
