import type { PlanId } from "../types/generated/PlanId.ts";

/** PlanId 在边界保留完整 u64；显示和比较均不得转换为 number。 */
export function parsePlanU64(value: unknown, path: string): string {
  if (typeof value !== "string" || value.length > 20 || !/^(0|[1-9][0-9]*)$/.test(value)
    || BigInt(value) > 18_446_744_073_709_551_615n) {
    throw new Error(`${path} 必须是 u64 范围内的规范非负十进制字符串`);
  }
  return value;
}

export function parsePlanId(value: unknown, path: string): PlanId {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`${path} 必须是包含 owner 和 sequence 的对象`);
  }
  const source = value as Record<string, unknown>;
  if (Object.keys(source).length !== 2 || !Object.hasOwn(source, "owner") || !Object.hasOwn(source, "sequence")) {
    throw new Error(`${path} 必须且只能包含 owner 和 sequence`);
  }
  return {
    owner: parsePlanU64(source.owner, `${path}.owner`),
    sequence: parsePlanU64(source.sequence, `${path}.sequence`),
  };
}

export function formatPlanId(id: PlanId): string {
  return `owner=${id.owner}, sequence=${id.sequence}`;
}
