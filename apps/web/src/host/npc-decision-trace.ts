import type { CatalogResolver } from "./catalog-resolver.ts";
import { parsePlanId } from "../utils/plan-identity.ts";
import type { PlanId } from "../types/generated/PlanId.ts";
import type { AccountId } from "../types/generated/AccountId.ts";

export type NpcDecisionTraceRecord = {
  readonly account: AccountId;
  readonly tick: string;
  readonly source_report_ids: readonly string[];
  readonly expectation_method: string | null;
  readonly plan_ids: readonly PlanId[];
  readonly plan_changes: readonly string[];
  readonly budget_constraints: readonly string[];
  readonly order_ids: readonly number[];
  readonly codes: readonly string[];
};

export type NpcDecisionDiagnostics =
  | { readonly kind: "unsupported" }
  | { readonly kind: "supported"; readonly records: readonly NpcDecisionTraceRecord[] };

export function parseNpcDecisionDiagnostics(value: unknown, identity: CatalogResolver): NpcDecisionDiagnostics {
  if (!isRecord(value) || typeof value["kind"] !== "string") {
    throw new Error("NPC 决策诊断响应必须包含 kind");
  }
  switch (value["kind"]) {
    case "unsupported":
      if (Object.keys(value).length !== 1) throw new Error("不支持的诊断响应不得携带私有数据");
      return { kind: "unsupported" };
    case "supported":
      if (Object.keys(value).length !== 2 || !("records" in value)) throw new Error("诊断记录响应字段无效");
      return { kind: "supported", records: parseNpcDecisionTrace(value["records"], identity) };
    default:
      throw new Error("未知的 NPC 决策诊断结果");
  }
}

export function parseNpcDecisionTrace(value: unknown, identity: CatalogResolver): readonly NpcDecisionTraceRecord[] {
  if (!Array.isArray(value)) throw new Error("NPC 决策诊断响应必须是数组");
  return value.map((record, index) => parseRecord(record, index, identity));
}

function parseRecord(value: unknown, index: number, identity: CatalogResolver): NpcDecisionTraceRecord {
  if (!isRecord(value)) throw new Error(`NPC 决策诊断第 ${index} 条必须是对象`);
  const keys = [
    "account",
    "tick",
    "source_report_ids",
    "expectation_method",
    "plan_ids",
    "plan_changes",
    "budget_constraints",
    "order_ids",
    "codes",
  ] as const;
  if (Object.keys(value).length !== keys.length || keys.some((key) => !(key in value))) {
    throw new Error(`NPC 决策诊断第 ${index} 条字段不完整`);
  }
  if (!Array.isArray(value["plan_ids"])) throw new Error(`NPC 决策诊断第 ${index} 条 plan_ids 必须是数组`);
  if (!isU64Decimal(value["account"]) || !isU64Decimal(value["tick"])
    || !isStringArray(value["source_report_ids"]) || !isStringOrNull(value["expectation_method"])
    || !isStringArray(value["plan_changes"])
    || !isStringArray(value["budget_constraints"])
    || !isNonNegativeIntegerArray(value["order_ids"]) || !isNonNegativeIntegerArray(value["codes"])) {
    throw new Error(`NPC 决策诊断第 ${index} 条字段无效`);
  }
  const seen = new Set<string>();
  const plan_ids = value["plan_ids"].map((entry, planIndex) => {
    const path = `NPC 决策诊断第 ${index} 条 plan_ids[${planIndex}]`;
    const id = parsePlanId(entry, path);
    if (id.owner !== value["account"]) throw new Error(`${path}.owner 必须与诊断 account 一致`);
    if (seen.has(id.sequence)) throw new Error(`${path} 包含重复的 PlanId`);
    seen.add(id.sequence);
    return id;
  });
  return {
    account: value["account"],
    tick: value["tick"],
    source_report_ids: value["source_report_ids"],
    expectation_method: value["expectation_method"],
    plan_ids,
    plan_changes: value["plan_changes"],
    budget_constraints: value["budget_constraints"],
    order_ids: value["order_ids"],
    // wire 的 codes 为数值 StockId 数组（ADR 0041）；摄入时逐个译回代码串。
    codes: value["codes"].map((id, codeIndex) => identity.stockCode(id, `NPC 决策诊断第 ${index} 条 codes[${codeIndex}]`)),
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isNonNegativeInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function isU64Decimal(value: unknown): value is string {
  return typeof value === "string" && /^(0|[1-9][0-9]*)$/.test(value)
    && value.length <= 20 && BigInt(value) <= 18_446_744_073_709_551_615n;
}

function isNonNegativeIntegerArray(value: unknown): value is readonly number[] {
  return Array.isArray(value) && value.every(isNonNegativeInteger);
}

function isStringArray(value: unknown): value is readonly string[] {
  return Array.isArray(value) && value.every((entry) => typeof entry === "string");
}

function isStringOrNull(value: unknown): value is string | null {
  return typeof value === "string" || value === null;
}
