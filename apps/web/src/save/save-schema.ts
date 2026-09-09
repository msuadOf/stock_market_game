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
    if (typeof stock.total_shares !== "string" || !/^\d+$/.test(stock.total_shares)) {
      throw new Error(`存档 setup.stocks[${index}] 的总股本必须是无损十进制字符串`);
    }
  });
}

function validateNpcAttention(value: unknown): void {
  if (!isRecord(value)) throw new Error("存档缺少合法的 NPC 注意力状态");
  for (const [accountId, state] of Object.entries(value)) {
    if (!/^\d+$/.test(accountId) || !isRecord(state)) {
      throw new Error(`存档 NPC 注意力账户 ${accountId} 无效`);
    }
    if (typeof state.base_probability !== "number"
      || !Number.isFinite(state.base_probability)
      || state.base_probability <= 0
      || state.base_probability > 1
      || typeof state.next_attention_candidate_tick !== "string"
      || !/^\d+$/.test(state.next_attention_candidate_tick)
      || typeof state.rng_state !== "string"
      || !/^\d+$/.test(state.rng_state)) {
      throw new Error(`存档 NPC ${accountId} 的注意力状态无效`);
    }
  }
}

function validateRetailExperience(value: unknown): void {
  if (!isRecord(value)) throw new Error("存档缺少合法的散户经历状态");
  for (const [accountId, experience] of Object.entries(value)) {
    if (!/^\d+$/.test(accountId) || !isRecord(experience) || !isRecord(experience.stocks)) {
      throw new Error(`存档散户经历账户 ${accountId} 无效`);
    }
    if ((experience.reference_equity !== null && !Number.isSafeInteger(experience.reference_equity))
      || (experience.peak_equity !== null && !Number.isSafeInteger(experience.peak_equity))
      || !Number.isSafeInteger(experience.consecutive_failed_buys)
      || Number(experience.consecutive_failed_buys) < 0) {
      throw new Error(`存档散户经历账户 ${accountId} 的账户状态无效`);
    }
    for (const [code, stock] of Object.entries(experience.stocks)) {
      if (!isRecord(stock)
        || typeof stock.last_trade_market_minute !== "string"
        || !/^\d+$/.test(stock.last_trade_market_minute)
        || typeof stock.last_observed_market_minute !== "string"
        || !/^\d+$/.test(stock.last_observed_market_minute)
        || (stock.cooldown_until_market_minute !== null
          && (typeof stock.cooldown_until_market_minute !== "string"
            || !/^\d+$/.test(stock.cooldown_until_market_minute)))) {
        throw new Error(`存档散户经历账户 ${accountId} 的股票 ${code} 无效`);
      }
    }
  }
}

function validateDailyTradeStatistics(candle: unknown, where: string): void {
  if (!isRecord(candle)) throw new Error(`存档 ${where} 必须是对象`);
  const stats = candle.trade_stats;
  if (stats === undefined || stats === null) return;
  if (!isRecord(stats)
    || typeof stats.turnover_cents !== "string"
    || !/^\d+$/.test(stats.turnover_cents)
    || !Number.isSafeInteger(stats.trade_count)
    || Number(stats.trade_count) < 0) {
    throw new Error(`存档 ${where} 的成交统计必须使用无损十进制成交额和非负安全整数笔数`);
  }
}

function validateSnapshotCandles(snapshot: Record<string, unknown>): void {
  const completed = snapshot.daily_candles as Record<string, unknown>;
  for (const [code, candles] of Object.entries(completed)) {
    if (!Array.isArray(candles)) throw new Error(`存档股票 ${code} 的日 K 历史必须是数组`);
    candles.forEach((candle, index) => validateDailyTradeStatistics(candle, `daily_candles.${code}[${index}]`));
  }
  const active = snapshot.active_daily_candles as Record<string, unknown>;
  for (const [code, candle] of Object.entries(active)) {
    validateDailyTradeStatistics(candle, `active_daily_candles.${code}`);
  }
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
  validateNpcAttention(value.npc_attention);
  validateRetailExperience(value.retail_experience);
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
    || !isRecord(value.market_minute_closes)
    || !Array.isArray(value.pending_player)
    || typeof value.rng_state !== "string"
    || !/^\d+$/.test(value.rng_state)
    || !Number.isSafeInteger(value.next_order_id)) {
    throw new Error("存档委托队列、交易分钟历史或 next_order_id 无效");
  }
  validateSnapshotCandles(value.snapshot);
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
