import type { CompanySystemConfig } from "../types/generated/CompanySystemConfig.ts";
import { parseCompanySystemConfig } from "../save/schema/setup-company-options.ts";

export interface AmountRange { readonly min: string; readonly max: string }
export interface CompanyPeriodBasis {
  readonly company: number;
  readonly revenue: AmountRange;
  readonly fixedExpense: AmountRange;
  readonly variableExpense: AmountRange | null;
}
export function createCompanyInitialPreset(template: CompanySystemConfig, _seed: bigint, _bases: readonly CompanyPeriodBasis[]): Extract<CompanySystemConfig, { mode: "Simple" }> {
  const result = parseCompanySystemConfig(template, "公司系统");
  if (result.mode !== "Simple") throw new Error("Simulation 尚未实现");
  return result;
}
