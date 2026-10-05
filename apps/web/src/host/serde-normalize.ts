import type { PublicReportAvailability, PublicReportAvailabilityQuery, PublicReportPage, PublicReportSummary } from "../types/engine.ts";
import { normalizePublicReportAvailabilityQuery as parsePublicReportAvailabilityQuery, parsePublicReport, parsePublicReportAvailability, parsePublicReportPage } from "./public-report-normalize.ts";
import { accountId as parseAccountId, safeIntegerKey } from "../save/schema/primitives.ts";

type WasmSaveMaps = {
  readonly runtime_state: {
    readonly strategy_states: Record<string, unknown>;
  };
  readonly snapshot: { readonly accounts: Record<string, unknown> };
  readonly npc_attention: Record<string, unknown>;
  readonly retail_experience: Record<string, unknown>;
  readonly parent_orders: Record<string, unknown>;
  readonly information_states: Record<string, unknown>;
  readonly belief_books: Record<string, unknown>;
  readonly watchlists: Record<string, unknown>;
  readonly price_memories: Record<string, unknown>;
  readonly plans: { readonly plans: Record<string, unknown> };
};

export function normalizePublicReportPage(value: unknown): PublicReportPage {
  return parsePublicReportPage(normalizeSerdeMaps(value));
}

export function normalizePublicReportById(value: unknown): PublicReportSummary {
  return parsePublicReport(normalizeSerdeMaps(value));
}

export function normalizePublicReportAvailability(value: unknown, query?: PublicReportAvailabilityQuery): PublicReportAvailability {
  return parsePublicReportAvailability(normalizeSerdeMaps(value), query);
}

export function normalizePublicReportAvailabilityQuery(value: unknown): PublicReportAvailabilityQuery {
  return parsePublicReportAvailabilityQuery(value);
}

/**
 * serde-wasm-bindgen 将 Rust Map 默认解码为 JS Map；UI、Redux 与 JSON 存档要求普通对象。
 * 所有 WASM 边界统一经过这个递归转换，避免某个宿主遗漏嵌套字段。
 */
export function normalizeSerdeMaps<T>(value: unknown): T {
  if (typeof value === "bigint") {
    const numericValue = Number(value);
    if (!Number.isSafeInteger(numericValue)) {
      throw new RangeError(`WASM 整数超出 JavaScript 安全整数范围：${value}`);
    }
    return numericValue as T;
  }
  if (value instanceof Map) {
    return Object.fromEntries([...value.entries()].map(([key, entry]) => [String(key), normalizeSerdeMaps(entry)])) as T;
  }
  if (Array.isArray(value)) return value.map(normalizeSerdeMaps) as T;
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([key, entry]) => [key, normalizeSerdeMaps(entry)])) as T;
  }
  return value as T;
}

/**
 * 准备 JSON 形态的存档供 serde-wasm-bindgen 反序列化。
 *
 * JSON 对象键只能是字符串；Rust `BTreeMap<AccountId, _>` 经本地存档往返后会得到
 * `"0"` 这样的键。AccountId 的当前 serde 契约要求字符串 Map key；PlanId 仍要求
 * 安全整数 Map key。远程/Tauri 的 JSON 协议保持标准对象形态。
 */
export function prepareSaveForWasm<T extends WasmSaveMaps>(slot: T): unknown {
  const accountMap = (entries: Record<string, unknown>, label: string): Map<string, unknown> => {
    const result = new Map<string, unknown>();
    for (const [rawAccountId, value] of Object.entries(entries)) {
      const accountId = parseAccountId(rawAccountId, `${label}账户 ID ${rawAccountId}`);
      result.set(accountId, value);
    }
    return result;
  };
  const accounts = accountMap(slot.snapshot.accounts, "");
  const strategyStates = accountMap(slot.runtime_state.strategy_states, "策略状态");
  const npcAttention = accountMap(slot.npc_attention, "NPC 注意力");
  const retailExperience = accountMap(slot.retail_experience, "散户经历");
  const parentOrders = accountMap(slot.parent_orders, "机构母单");
  const informationStates = accountMap(slot.information_states, "个人信息");
  const beliefBooks = accountMap(slot.belief_books, "个人信念");
  const watchlists = accountMap(slot.watchlists, "个人关注");
  const priceMemories = accountMap(slot.price_memories, "价格记忆");
  const plans = new Map(Object.entries(slot.plans.plans).map(([planId, value]) => {
    safeIntegerKey(planId, `交易计划 ID ${planId}`);
    return [Number(planId), value] as const;
  }));
  return {
    ...slot,
    runtime_state: {
      ...slot.runtime_state,
      strategy_states: strategyStates,
    },
    npc_attention: npcAttention,
    retail_experience: retailExperience,
    parent_orders: parentOrders,
    information_states: informationStates,
    belief_books: beliefBooks,
    watchlists,
    price_memories: priceMemories,
    plans: {
      ...slot.plans,
      plans,
    },
    snapshot: {
      ...slot.snapshot,
      accounts,
    },
  };
}
