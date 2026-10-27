import type { CompanySystemConfig } from "../types/generated/CompanySystemConfig.ts";
import { parseCompanySystemConfig } from "../save/schema/setup-company-options.ts";

export interface SeedDraft {
  readonly seed: string;
  readonly origin: "preset" | "custom";
  readonly companySystem: string;
}
export function parseSessionSeed(seed: string): bigint { return BigInt(seed); }
export function createSeedDraft(system: CompanySystemConfig, seed: bigint): SeedDraft {
  return { seed: seed.toString(), origin: "preset", companySystem: JSON.stringify(system, null, 2) };
}
export function changeSeedDraft(draft: SeedDraft, seed: string): SeedDraft { return { ...draft, seed }; }
export function editCompanySystemDraft(draft: SeedDraft, raw: string): SeedDraft { return { ...draft, origin: "custom", companySystem: raw }; }
export function parseSeedDraft(draft: SeedDraft): { seed: bigint; companySystem: CompanySystemConfig } {
  return { seed: parseSessionSeed(draft.seed), companySystem: parseCompanySystemConfig(JSON.parse(draft.companySystem), "公司系统") };
}
