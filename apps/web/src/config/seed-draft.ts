import { createCompanyInitialPreset, type CompanyPeriodBasis } from "./company-initial-preset.ts";
import { createNewSessionSeed } from "./session-seed.ts";
import type { SettlementCycle } from "../types/generated/SettlementCycle.ts";
import type { SimpleCompanyPreferences } from "../types/generated/SimpleCompanyPreferences.ts";
import type { CompanySystemConfig } from "../types/generated/CompanySystemConfig.ts";
import { parseCompanySystemConfig } from "../save/schema/setup-company-options.ts";

export interface SeedDraft {
  readonly seed: string;
  readonly origin: "preset" | "custom";
  readonly companySystem: string;
}
export function parseSessionSeed(seed: string): bigint {
  if (!/^(0|[1-9]\d*)$/.test(seed) || BigInt(seed) > 18446744073709551615n) throw new RangeError("seed 必须为规范 u64 十进制字符串");
  return BigInt(seed);
}
function parseSimple(value: unknown) {
  const config = parseCompanySystemConfig(value, "公司系统");
  if (config.mode !== "Simple") throw new Error("Simulation 尚未实现，不能创建该模式");
  return config;
}
export function createSeedDraft(system: CompanySystemConfig, seed: bigint): SeedDraft {
  parseSessionSeed(seed.toString());
  return { seed: seed.toString(), origin: "preset", companySystem: JSON.stringify(parseSimple(system), null, 2) };
}
export function changeSeedDraft(draft: SeedDraft, seed: string): SeedDraft { return { ...draft, seed }; }
export function editCompanySystemDraft(draft: SeedDraft, raw: string): SeedDraft { return { ...draft, origin: "custom", companySystem: raw }; }
export function parseSeedDraft(draft: SeedDraft): { seed: bigint; companySystem: CompanySystemConfig } {
  return { seed: parseSessionSeed(draft.seed), companySystem: parseSimple(JSON.parse(draft.companySystem)) };
}

/** 偏好只更新指定 numeric CompanyId，不触发 seed 或金额重建。 */
export function changeCompanyPreferencesDraft(draft: SeedDraft, company: number, preferences: SimpleCompanyPreferences): SeedDraft {
  const system = parseSimple(JSON.parse(draft.companySystem));
  const companies = system.config.companies;
  if (!Number.isInteger(company) || company < 0 || company > 4294967295) throw new RangeError("company 必须为 numeric u32 CompanyId");
  if (companies.filter(entry => entry.company === company).length !== 1) throw new Error(`公司 ${company} 必须在当前配置中唯一存在`);
  const next = parseSimple({ ...system, config: { ...system.config, companies: companies.map(entry => entry.company === company ? { ...entry, preferences } : entry) } });
  return { ...draft, origin: "custom", companySystem: JSON.stringify(next, null, 2) };
}

/** 显式操作才抽取新 seed；输入预检失败不会消耗熵或改写原草稿。 */
export function regenerateSeedDraft(draft: SeedDraft, bases: readonly CompanyPeriodBasis[], createSeed: () => bigint = createNewSessionSeed): SeedDraft {
  const { companySystem, seed: previousSeed } = parseSeedDraft(draft);
  createCompanyInitialPreset(companySystem, previousSeed, bases);
  const seed = createSeed();
  return createSeedDraft(createCompanyInitialPreset(companySystem, seed, bases), seed);
}

/** preset 的新周期金额必须由调用方明确输入；custom 只能显式编辑或重新生成。 */
export function changeSettlementCycleDraft(draft: SeedDraft, cycle: SettlementCycle, bases: readonly CompanyPeriodBasis[]): SeedDraft {
  if (draft.origin !== "preset") throw new Error("本人编辑 custom 配置不能自动切换周期；请明确编辑 JSON 或重新生成预设");
  const { companySystem, seed } = parseSeedDraft(draft);
  const system = parseSimple(companySystem);
  const next = parseSimple({ ...system, config: { ...system.config, settlement_cycle: cycle } });
  return createSeedDraft(createCompanyInitialPreset(next, seed, bases), seed);
}
