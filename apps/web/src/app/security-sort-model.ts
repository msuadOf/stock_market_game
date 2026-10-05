import { buildMarketRows, type MarketGridRow } from "../components/market-grid-rows.ts";
import type { MarketSnap } from "../types/engine.ts";
import { compareMoney } from "../utils/money.ts";

export const SECURITY_SORT_FIELDS = ["code", "name", "lastPrice", "changeAbs", "changePct", "lastClose", "best_bid", "best_ask"] as const;
export type SecuritySortField = typeof SECURITY_SORT_FIELDS[number];
export interface SecuritySort { field: SecuritySortField; direction: "asc" | "desc" }

export function validateSecuritySort(value: unknown): asserts value is SecuritySort[] {
  if (!Array.isArray(value)) throw new RangeError("证券排序规则必须为数组");
  const fields = new Set<string>();
  for (const rule of value) {
    if (rule === null || typeof rule !== "object" || !SECURITY_SORT_FIELDS.includes(rule.field)
      || !["asc", "desc"].includes(rule.direction) || fields.has(rule.field)
      || Object.keys(rule).some(key => key !== "field" && key !== "direction")) throw new RangeError("证券排序规则字段、方向或重复项无效");
    fields.add(rule.field);
  }
}

export function compareSecurityRows(left: MarketGridRow, right: MarketGridRow, field: SecuritySortField): number {
  if (field === "code" || field === "name") return left[field].localeCompare(right[field], "zh-CN");
  if (field === "changePct") {
    // 用真实分值交叉相乘，避免显示百分比舍入后相等或 Number 丢失最低分值。
    const leftClose = BigInt(left._rawLastClose);
    const rightClose = BigInt(right._rawLastClose);
    if (leftClose <= 0n || rightClose <= 0n) throw new RangeError("涨跌幅排序需要正昨收价");
    const difference = (BigInt(left.lastPrice) - leftClose) * rightClose - (BigInt(right.lastPrice) - rightClose) * leftClose;
    return difference < 0n ? -1 : difference > 0n ? 1 : 0;
  }
  if (field === "best_bid" || field === "best_ask") {
    const a = left._source[field];
    const b = right._source[field];
    return a === null ? (b === null ? 0 : -1) : b === null ? 1 : compareMoney(a, b);
  }
  if (field === "lastClose") return compareMoney(left._rawLastClose, right._rawLastClose);
  return compareMoney(left[field], right[field]);
}

export function sortSecurityCodes(codes: readonly string[], markets: Readonly<Record<string, MarketSnap>>, rules: readonly SecuritySort[]): string[] {
  validateSecuritySort(rules);
  for (const code of codes) if (!Object.hasOwn(markets, code)) throw new RangeError(`证券排序缺少行情：${code}`);
  if (rules.length === 0) return [...codes];
  return buildMarketRows(markets, codes).sort((left, right) => {
    for (const rule of rules) {
      const difference = compareSecurityRows(left, right, rule.field);
      if (difference !== 0) return rule.direction === "asc" ? difference : -difference;
    }
    return 0;
  }).map(row => row.code);
}

/** 手机仅有涨幅排序入口，不受隐藏桌面列驱动。 */
export function mobileSecuritySort(rules: readonly SecuritySort[]): SecuritySort[] {
  return rules.filter(rule => rule.field === "changePct");
}

export function cycleChangeSort(rules: readonly SecuritySort[]): SecuritySort[] {
  const current = mobileSecuritySort(rules)[0];
  if (current?.direction === "asc") return [];
  return [{ field: "changePct", direction: current?.direction === "desc" ? "asc" : "desc" }];
}
