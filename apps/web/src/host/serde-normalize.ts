import type { PublicReportPage, PublicReportSummary } from "../types/engine.ts";
import { parsePublicReport, parsePublicReportPage } from "./public-report-normalize.ts";

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
    const record: Record<string, unknown> = {};
    for (const [key, entry] of value.entries()) {
      record[String(key)] = normalizeSerdeMaps(entry);
    }
    return record as T;
  }
  if (Array.isArray(value)) return value.map(normalizeSerdeMaps) as T;
  if (value !== null && typeof value === "object") {
    const record: Record<string, unknown> = {};
    for (const [key, entry] of Object.entries(value)) {
      record[key] = normalizeSerdeMaps(entry);
    }
    return record as T;
  }
  return value as T;
}

/**
 * 准备 JSON 形态的存档供 serde-wasm-bindgen 反序列化。
 *
 * JSON 对象键只能是字符串；Rust `BTreeMap<AccountId, _>` 经本地存档往返后会得到
 * `"0"` 这样的键，而 serde-wasm-bindgen 的 u64 map key 需要数值。仅在 WASM 入站
 * 边界恢复这一处 Map，远程/Tauri 的 JSON 协议仍保持标准对象形态。
 */
export function prepareSaveForWasm<T extends WasmSaveMaps>(slot: T): unknown {
  const accountMap = (entries: Record<string, unknown>, label: string): Map<number, unknown> => {
    const result = new Map<number, unknown>();
    for (const [rawAccountId, value] of Object.entries(entries)) {
      if (!/^\d+$/.test(rawAccountId)) {
        throw new Error(`${label}账户 ID 不是非负十进制整数：${rawAccountId}`);
      }
      const accountId = Number(rawAccountId);
      if (!Number.isSafeInteger(accountId)) {
        throw new Error(`${label}账户 ID 超出 JavaScript 安全整数范围：${rawAccountId}`);
      }
      if (result.has(accountId)) {
        throw new Error(`${label}账户 ID 转换后重复：${rawAccountId}`);
      }
      if (!/^(0|[1-9]\d*)$/.test(rawAccountId)) {
        throw new Error(`${label}账户 ID 不是规范非负十进制整数：${rawAccountId}`);
      }
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
  const plans = accountMap(slot.plans.plans, "交易计划");
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
