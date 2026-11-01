import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
export const names = ["generated-company-state-registry.json", "manual-company-state-registry.json", "legacy-company-state.json"] as const;
export const historicalRaw = names.map(name => readFileSync(new URL(`../../fixtures/source-b/${name}`, import.meta.url), "utf8"));
// synthetic：仅为未改动的其他 codec 保留历史结构覆盖；补 report correction 空表、策略、bindings 与 pending_opening 都不代表当前 Rust producer 或恢复已验。
export function syntheticCorrectionOperationsProjection(text: string): string {
 const root=JSON.parse(text) as Record<string,any>;
 if(Object.hasOwn(root,"report_correction_operations"))return text;
 return JSON.stringify({...root,report_correction_operations:{}});
}
export function syntheticPolicyProjection(text: string): string {
 const corrected=syntheticCorrectionOperationsProjection(text);
 const root=JSON.parse(corrected) as Record<string,any>;
 if(root.setup===undefined||typeof root.setup!=="object"||root.setup===null||Object.hasOwn(root.setup,"runtime_sync_policy"))return corrected;
 return JSON.stringify({...root,setup:{...root.setup,runtime_sync_policy:"every_tick"}});
}
export function syntheticBindingsProjection(text: string): string {
 const root=JSON.parse(text) as Record<string,any>;const company=root.company_state.CompanySystem;
 if(company===undefined)return text;
 assert.equal(Object.hasOwn(company.corporate_actions,"repurchase_execution_bindings"),false,"历史原件应保持旧字段集合");
 return JSON.stringify({...root,company_state:{CompanySystem:{...company,corporate_actions:{...company.corporate_actions,repurchase_execution_bindings:[],registries:company.corporate_actions.registries.map((registry: Record<string, unknown>) => {
  assert.equal(Object.hasOwn(registry,"pending_opening"),false,"历史原件不含盘前字段");
  return {...registry,pending_opening:null};
 })}}}});
}
export const correctionProjectedRaw = historicalRaw.map(syntheticCorrectionOperationsProjection);
export const policyProjectedRaw = correctionProjectedRaw.map(syntheticPolicyProjection);
export const raw = policyProjectedRaw.map(syntheticBindingsProjection);
export function sample(index = 0): Record<string, unknown> { return JSON.parse(raw[index]); }
export function system(root: Record<string, unknown>): Record<string, any> { return (root.company_state as Record<string, any>).CompanySystem; }
export function binding(root: Record<string, unknown>): Record<string, any> { return system(root).corporate_actions.registries[0].receipts[0].request.scope.SettlementDayMarket.binding; }
