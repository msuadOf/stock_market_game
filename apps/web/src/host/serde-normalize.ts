/**
 * serde-wasm-bindgen 将 Rust Map 默认解码为 JS Map；UI、Redux 与 JSON 存档要求普通对象。
 * 所有 WASM 边界统一经过这个递归转换，避免某个宿主遗漏嵌套字段。
 */
export function normalizeSerdeMaps<T>(value: unknown): T {
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
export function prepareSaveForWasm(slot: SaveSlot): unknown {
  const accounts = new Map<number, unknown>();
  for (const [rawAccountId, account] of Object.entries(slot.snapshot.accounts)) {
    if (!/^\d+$/.test(rawAccountId)) {
      throw new Error(`存档账户 ID 不是非负十进制整数：${rawAccountId}`);
    }
    const accountId = Number(rawAccountId);
    if (!Number.isSafeInteger(accountId)) {
      throw new Error(`存档账户 ID 超出 JavaScript 安全整数范围：${rawAccountId}`);
    }
    accounts.set(accountId, account);
  }
  return {
    ...slot,
    snapshot: {
      ...slot.snapshot,
      accounts,
    },
  };
}
import type { SaveSlot } from "../types/engine";
