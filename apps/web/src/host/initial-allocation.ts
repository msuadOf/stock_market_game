import { exact, field, record, values, safeU32 } from "./protocol/guards.ts";

import type { InitialAllocation } from "../types/generated/InitialAllocation.ts";
import type { InitialAllocationCategory } from "../types/generated/InitialAllocationCategory.ts";
export type { InitialAllocation } from "../types/generated/InitialAllocation.ts";

export function parseInitialAllocation(value: unknown, expectedCodes: readonly string[]): InitialAllocation {
  const source = record(value, "初始分配响应");
  exact(source, ["stocks"], "初始分配响应");
  const seen = new Set<string>();
  const stocks = values(field(source, "stocks", "初始分配响应"), "初始分配响应.stocks").map((raw, index) => {
    const path = `初始分配响应.stocks[${index}]`;
    const stock = record(raw, path);
    exact(stock, ["code", "float_shares", "unallocated_shares", "categories"], path);
    const code = field(stock, "code", path);
    if (typeof code !== "string" || !expectedCodes.includes(code) || seen.has(code)) throw new Error(`${path}.code 不属于本次会话证券或重复`);
    seen.add(code);
    const float_shares = safeU32(stock.float_shares, `${path}.float_shares`);
    const unallocated_shares = safeU32(stock.unallocated_shares, `${path}.unallocated_shares`);
    const categories = values(field(stock, "categories", path), `${path}.categories`).map((item, categoryIndex) => {
      const categoryPath = `${path}.categories[${categoryIndex}]`;
      const category = record(item, categoryPath);
      exact(category, ["kind", "shares", "account_count", "zero_holders"], categoryPath);
      const kind = field(category, "kind", categoryPath);
      if (kind !== ["Retail", "Inst", "Hot"][categoryIndex]) throw new Error(`${categoryPath}.kind 必须按 Retail/Inst/Hot 完整且唯一排列`);
      const shares = safeU32(category.shares, `${categoryPath}.shares`);
      const account_count = safeU32(category.account_count, `${categoryPath}.account_count`);
      const zero_holders = safeU32(category.zero_holders, `${categoryPath}.zero_holders`);
      if (zero_holders > account_count || (account_count === zero_holders && shares !== 0) || (account_count > zero_holders && shares < account_count - zero_holders)) throw new Error(`${categoryPath} 股数与零持股人数不一致`);
      return { kind: kind as InitialAllocationCategory["kind"], shares, account_count, zero_holders };
    });
    if (categories.length !== 3) throw new Error(`${path}.categories 必须完整包含三类 NPC`);
    if (categories.reduce((sum, category) => sum + category.shares, unallocated_shares) !== float_shares) throw new Error(`${path} 实际分配与流通盘不守恒`);
    if (unallocated_shares > 0 && categories.some((category) => category.account_count > 0)) throw new Error(`${path} 存在 NPC 但流通盘未完整分配`);
    return { code, float_shares, unallocated_shares, categories };
  });
  if (seen.size !== expectedCodes.length) throw new Error("初始分配响应遗漏本局证券");
  return { stocks };
}
