import type { SessionSetup } from "../types/engine.ts";
import type { SimpleCompanyPreferences } from "../types/generated/SimpleCompanyPreferences.ts";
import { createPriceAnchoredCompanyConfig } from "./company-initial-preset.ts";
import { createNewSessionSeed } from "./session-seed.ts";
import { parseCompanySystemConfig, parseSimpleCompanyPreferences } from "../save/schema/company/system-config.ts";
import { parseSettlementCycle } from "../save/schema/company/period-generation.ts";

export type DraftSettlementCycle = Extract<SessionSetup["company_system"], { mode: "Simple" }>["config"]["settlement_cycle"];

export interface SeedDraft {
  readonly seed: string;
  readonly origin: "preset" | "custom";
  readonly companySystem: string;
}

export function parseSessionSeed(seed: string): bigint {
  if (!/^(0|[1-9]\d*)$/.test(seed) || BigInt(seed) > 18446744073709551615n) throw new RangeError("seed 必须为规范 u64 十进制字符串");
  return BigInt(seed);
}

export function createSeedDraft(setup: SessionSetup, seed: bigint): SeedDraft {
  parseSessionSeed(seed.toString());
  const config = setup.company_system.mode === "Simple" ? createPriceAnchoredCompanyConfig(setup, seed) : setup.company_system;
  return { seed: seed.toString(), origin: "preset", companySystem: JSON.stringify(parseCompanySystemConfig(config), null, 2) };
}

export function changeSeedDraft(setup: SessionSetup, draft: SeedDraft, seed: string): SeedDraft {
  if (draft.origin === "custom") return { ...draft, seed };
  let parsed: bigint;
  try { parsed = parseSessionSeed(seed); } catch (error) {
    if (!(error instanceof RangeError)) throw error;
    return { ...draft, seed };
  }
  return createSeedDraft({ ...setup, company_system: parseCompanySystemConfig(JSON.parse(draft.companySystem)) }, parsed);
}

export function regenerateSeedDraft(setup: SessionSetup, draft: SeedDraft, createSeed: () => bigint = createNewSessionSeed): SeedDraft {
  const config = parseCompanySystemConfig(JSON.parse(draft.companySystem));
  return createSeedDraft({ ...setup, company_system: config }, createSeed());
}

export function changeSettlementCycleDraft(setup: SessionSetup, draft: SeedDraft, cycle: DraftSettlementCycle): SeedDraft {
  if (draft.origin === "custom") throw new Error("当前配置为本人编辑（含仅经偏好表单修改的情况）：切换结算周期需重建同长度期初金额，请在 JSON 中同时指定结算周期和对应金额，或先明确重新生成预设后再切换；不会自动覆盖本人参数");
  const config = parseCompanySystemConfig(JSON.parse(draft.companySystem));
  if (config.mode !== "Simple") throw new Error("结算周期预览仅适用于 Simple；不会自动切换公司模式");
  const companySystem = { ...config, config: { ...config.config, settlement_cycle: parseSettlementCycle(cycle) } };
  return createSeedDraft({ ...setup, company_system: companySystem }, parseSessionSeed(draft.seed));
}

/**
 * 更新某公司的行为偏好（ADR-0037）：直接编辑本人配置 JSON 并标记 origin=custom，
 * 不经过 seed 预设重建——偏好是显式配置，不能被重新生成覆盖或丢弃。
 * 数值域由 parseSimpleCompanyPreferences 严格校验（与 engine
 * `SimpleCompanyPreferences::validate` 同域），非法值显式抛错。
 */
export function changeCompanyPreferencesDraft(draft: SeedDraft, company: string, preferences: SimpleCompanyPreferences): SeedDraft {
  const config = parseCompanySystemConfig(JSON.parse(draft.companySystem));
  const index = config.config.companies.findIndex(entry => entry.company === company);
  if (index === -1) throw new Error(`公司 ${company} 不在当前配置中，不能更新偏好`);
  const parsedPreferences = parseSimpleCompanyPreferences(preferences, `公司系统.config.companies[${index}].preferences`);
  const companies = config.config.companies.map((entry, entryIndex) => entryIndex === index ? { ...entry, preferences: parsedPreferences } : entry);
  return { seed: draft.seed, origin: "custom", companySystem: JSON.stringify({ ...config, config: { ...config.config, companies } }, null, 2) };
}
