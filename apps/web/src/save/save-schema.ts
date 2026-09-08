import type { SaveSlot } from "../types/engine";

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function validateStocks(stocks: unknown[]): void {
  stocks.forEach((stock, index) => {
    if (!isRecord(stock)) throw new Error(`存档 setup.stocks[${index}] 必须是对象`);
    if (stock.exchange === undefined) {
      throw new Error(`存档 setup.stocks[${index}] 缺少交易所`);
    } else if (stock.exchange !== "Shanghai" && stock.exchange !== "Shenzhen") {
      throw new Error(`存档 setup.stocks[${index}] 的交易所无效`);
    }
    if (!(["MainBoard", "StMainBoard", "ChiNext"] as unknown[]).includes(stock.category)) {
      throw new Error(`存档 setup.stocks[${index}] 的证券类别无效`);
    }
  });
}

/**
 * 校验不可信存档的跨界外壳。更深的市场、账户和 OHLC 不变量由 Rust
 * `GameSession::restore` 作最终权威校验。
 */
export function parseSaveSlot(value: unknown): SaveSlot {
  if (!isRecord(value)) throw new Error("存档根节点必须是对象");
  if ("schema_version" in value) {
    throw new Error("存档包含已废弃的 schema_version，不支持旧格式");
  }
  if (typeof value.seed !== "string" || !/^\d+$/.test(value.seed)) {
    throw new Error("存档 seed 必须是无损十进制字符串");
  }
  if (!isRecord(value.setup) || !Array.isArray(value.setup.stocks)) {
    throw new Error("存档缺少合法的 setup");
  }
  validateStocks(value.setup.stocks);
  if (!isRecord(value.snapshot)
    || !Number.isSafeInteger(value.snapshot.seq)
    || !Number.isSafeInteger(value.snapshot.tick)
    || !isRecord(value.snapshot.markets)
    || !isRecord(value.snapshot.accounts)
    || !isRecord(value.snapshot.daily_candles)
    || !isRecord(value.snapshot.active_daily_candles)) {
    throw new Error("存档缺少合法的 snapshot");
  }
  if (!isRecord(value.auction_orders)
    || !isRecord(value.resting_orders)
    || !isRecord(value.price_history)
    || !Array.isArray(value.pending_player)
    || typeof value.rng_state !== "string"
    || !/^\d+$/.test(value.rng_state)
    || !Number.isSafeInteger(value.next_order_id)) {
    throw new Error("存档委托队列或 next_order_id 无效");
  }
  return value as unknown as SaveSlot;
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
