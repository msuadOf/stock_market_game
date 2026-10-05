import type { SessionSetup } from "../types/engine.ts";
import { createPriceAnchoredCompanyConfig } from "./company-initial-preset.ts";
import { createNewSessionSeed } from "./session-seed.ts";
import { parseCompanySystemConfig } from "../save/schema/company/system-config.ts";
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
  if (draft.origin === "custom") throw new Error("本人编辑的配置请在 JSON 中同时指定结算周期和同长度期初基准，或明确重新生成预设后再切换；不会自动覆盖本人参数");
  const config = parseCompanySystemConfig(JSON.parse(draft.companySystem));
  if (config.mode !== "Simple") throw new Error("结算周期预览仅适用于 Simple；不会自动切换公司模式");
  const companySystem = { ...config, config: { ...config.config, settlement_cycle: parseSettlementCycle(cycle) } };
  return createSeedDraft({ ...setup, company_system: companySystem }, parseSessionSeed(draft.seed));
}
