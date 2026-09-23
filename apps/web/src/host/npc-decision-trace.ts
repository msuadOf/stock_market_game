export type NpcDecisionTraceRecord = {
  readonly account: number;
  readonly tick: number;
  readonly source_report_ids: readonly string[];
  readonly expectation_method: string | null;
  readonly plan_ids: readonly number[];
  readonly budget_constraints: readonly string[];
  readonly order_ids: readonly number[];
  readonly codes: readonly string[];
};

export type NpcDecisionDiagnostics =
  | { readonly kind: "unsupported" }
  | { readonly kind: "supported"; readonly records: readonly NpcDecisionTraceRecord[] };

export function parseNpcDecisionDiagnostics(value: unknown): NpcDecisionDiagnostics {
  if (!isRecord(value) || typeof value["kind"] !== "string") {
    throw new Error("NPC 决策诊断响应必须包含 kind");
  }
  switch (value["kind"]) {
    case "unsupported":
      if (Object.keys(value).length !== 1) throw new Error("不支持的诊断响应不得携带私有数据");
      return { kind: "unsupported" };
    case "supported":
      if (Object.keys(value).length !== 2 || !("records" in value)) throw new Error("诊断记录响应字段无效");
      return { kind: "supported", records: parseNpcDecisionTrace(value["records"]) };
    default:
      throw new Error("未知的 NPC 决策诊断结果");
  }
}

export function parseNpcDecisionTrace(value: unknown): readonly NpcDecisionTraceRecord[] {
  if (!Array.isArray(value)) throw new Error("NPC 决策诊断响应必须是数组");
  return value.map((record, index) => parseRecord(record, index));
}

function parseRecord(value: unknown, index: number): NpcDecisionTraceRecord {
  if (!isRecord(value)) throw new Error(`NPC 决策诊断第 ${index} 条必须是对象`);
  const keys = [
    "account",
    "tick",
    "source_report_ids",
    "expectation_method",
    "plan_ids",
    "budget_constraints",
    "order_ids",
    "codes",
  ] as const;
  if (Object.keys(value).length !== keys.length || keys.some((key) => !(key in value))) {
    throw new Error(`NPC 决策诊断第 ${index} 条字段不完整`);
  }
  if (!isNonNegativeInteger(value["account"]) || !isNonNegativeInteger(value["tick"])
    || !isStringArray(value["source_report_ids"]) || !isStringOrNull(value["expectation_method"])
    || !isNonNegativeIntegerArray(value["plan_ids"]) || !isStringArray(value["budget_constraints"])
    || !isNonNegativeIntegerArray(value["order_ids"]) || !isStringArray(value["codes"])) {
    throw new Error(`NPC 决策诊断第 ${index} 条字段无效`);
  }
  return {
    account: value["account"],
    tick: value["tick"],
    source_report_ids: value["source_report_ids"],
    expectation_method: value["expectation_method"],
    plan_ids: value["plan_ids"],
    budget_constraints: value["budget_constraints"],
    order_ids: value["order_ids"],
    codes: value["codes"],
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isNonNegativeInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
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
