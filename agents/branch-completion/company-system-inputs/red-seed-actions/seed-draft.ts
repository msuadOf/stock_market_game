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
export function changeCompanyPreferencesDraft(draft: SeedDraft, company: number, preferences: import("../types/generated/SimpleCompanyPreferences.ts").SimpleCompanyPreferences): SeedDraft {
  const system = parseSimple(JSON.parse(draft.companySystem));
  const companies = system.config.companies;
  if (!Number.isInteger(company) || company < 0 || company > 4294967295) throw new RangeError("company 必须为 numeric u32 CompanyId");
  if (companies.filter(entry => entry.company === company).length !== 1) throw new Error(`公司 ${company} 必须在当前配置中唯一存在`);
  const next = parseSimple({ ...system, config: { ...system.config, companies: companies.map(entry => entry.company === company ? { ...entry, preferences } : entry) } });
  return { ...draft, origin: "custom", companySystem: JSON.stringify(next, null, 2) };
}

export function regenerateSeedDraft(draft: SeedDraft, _bases: readonly import("./company-initial-preset.ts").CompanyPeriodBasis[], _createSeed: () => bigint): SeedDraft { return draft; }
export function changeSettlementCycleDraft(draft: SeedDraft, _cycle: import("../types/generated/SettlementCycle.ts").SettlementCycle, _bases: readonly import("./company-initial-preset.ts").CompanyPeriodBasis[]): SeedDraft { return draft; }
