import type { SaveSlot } from "../types/engine";

const CURRENT_SAVE_SCHEMA_VERSION = 2;
const LEGACY_SAVE_SCHEMA_VERSION = 1;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function inferLegacyExchange(code: unknown): "Shanghai" | "Shenzhen" {
  if (typeof code !== "string" || !/^\d{6}$/.test(code)) {
    throw new Error(`旧存档股票 ${String(code)} 无法推断交易所`);
  }
  const prefix = code.slice(0, 3);
  if (["600", "601", "603", "605"].includes(prefix)) return "Shanghai";
  if (["000", "001", "002", "003", "300", "301"].includes(prefix)) return "Shenzhen";
  throw new Error(`旧存档股票 ${code} 无法推断交易所：当前仅支持沪深主板与创业板`);
}

function inferLegacyCategory(
  code: unknown,
  limitPct: unknown,
): "MainBoard" | "StMainBoard" | "ChiNext" {
  if (typeof code !== "string") throw new Error(`旧存档股票 ${String(code)} 无法推断证券类别`);
  if (code.startsWith("300") || code.startsWith("301")) return "ChiNext";
  if (limitPct === 0.05) return "StMainBoard";
  return "MainBoard";
}

function normalizeStockExchanges(
  stocks: unknown[],
  allowLegacyInference: boolean,
): Record<string, unknown>[] {
  return stocks.map((stock, index) => {
    if (!isRecord(stock)) throw new Error(`存档 setup.stocks[${index}] 必须是对象`);
    let normalized = stock;
    if (stock.exchange === undefined) {
      if (!allowLegacyInference) {
        throw new Error(`当前版本存档 setup.stocks[${index}] 缺少交易所`);
      }
      normalized = { ...normalized, exchange: inferLegacyExchange(stock.code) };
    } else if (stock.exchange !== "Shanghai" && stock.exchange !== "Shenzhen") {
      throw new Error(`存档 setup.stocks[${index}] 的交易所无效`);
    }
    if (allowLegacyInference && stock.category === undefined) {
      normalized = {
        ...normalized,
        category: inferLegacyCategory(stock.code, stock.limit_pct),
      };
    }
    return normalized;
  });
}

/**
 * 校验不可信存档的跨界外壳。更深的市场、账户和 OHLC 不变量由 Rust
 * `GameSession::restore` 作最终权威校验。
 */
export function parseSaveSlot(value: unknown): SaveSlot {
  if (!isRecord(value)) throw new Error("存档根节点必须是对象");
  const sourceVersion = value.schema_version === undefined
    ? LEGACY_SAVE_SCHEMA_VERSION
    : value.schema_version;
  if (sourceVersion !== LEGACY_SAVE_SCHEMA_VERSION
    && sourceVersion !== CURRENT_SAVE_SCHEMA_VERSION) {
    throw new Error(`不支持的存档 schema_version：${String(sourceVersion)}`);
  }
  let normalized: Record<string, unknown> = { ...value, schema_version: sourceVersion };
  if (typeof value.seed === "number" && Number.isSafeInteger(value.seed) && value.seed >= 0) {
    // 兼容旧版在 JS 安全整数范围内写出的 numeric seed；读入后立即升级为无损字符串。
    normalized = { ...normalized, seed: String(value.seed) };
  }
  if (normalized.resting_orders === undefined) {
    normalized = { ...normalized, resting_orders: {} };
  }
  if (normalized.price_history === undefined) {
    normalized = { ...normalized, price_history: {} };
  }
  if (normalized.rng_state === undefined) {
    normalized = { ...normalized, rng_state: null };
  }
  if (typeof normalized.seed !== "string" || !/^\d+$/.test(normalized.seed)) {
    throw new Error("存档 seed 必须是无损十进制字符串");
  }
  if (!isRecord(normalized.setup) || !Array.isArray(normalized.setup.stocks)) {
    throw new Error("存档缺少合法的 setup");
  }
  const stocks = normalizeStockExchanges(
    normalized.setup.stocks,
    sourceVersion === LEGACY_SAVE_SCHEMA_VERSION,
  );
  const setup = { ...normalized.setup, stocks };
  normalized = {
    ...normalized,
    // Rust engine owns semantic migration (limits and T+1 locks). Keeping v1 here
    // prevents the host boundary from erasing the information needed for that migration.
    schema_version: sourceVersion,
    setup,
  };
  if (!isRecord(normalized.snapshot)
    || !Number.isSafeInteger(normalized.snapshot.seq)
    || !Number.isSafeInteger(normalized.snapshot.tick)
    || !isRecord(normalized.snapshot.markets)
    || !isRecord(normalized.snapshot.accounts)) {
    throw new Error("存档缺少合法的 snapshot");
  }
  if (!isRecord(normalized.auction_orders)
    || !isRecord(normalized.resting_orders)
    || !isRecord(normalized.price_history)
    || (normalized.rng_state !== null
      && (typeof normalized.rng_state !== "string" || !/^\d+$/.test(normalized.rng_state)))
    || !Number.isSafeInteger(normalized.next_order_id)) {
    throw new Error("存档委托队列或 next_order_id 无效");
  }
  return normalized as unknown as SaveSlot;
}

export function parseSaveJson(text: string): SaveSlot {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch (error) {
    throw new Error(`存档不是合法 JSON：${error instanceof Error ? error.message : String(error)}`);
  }
  return parseSaveSlot(value);
}
