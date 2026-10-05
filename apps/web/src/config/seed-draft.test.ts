import assert from "node:assert/strict";
import test from "node:test";
import { DEFAULT_SETUP } from "./defaults.ts";
import { changeSeedDraft, changeSettlementCycleDraft, createSeedDraft, parseSessionSeed, regenerateSeedDraft } from "./seed-draft.ts";
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
