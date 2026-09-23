import type { MarketSnap, PriceTickEvent } from "../types/engine.ts";

/** 用权威价格 tick 原子刷新成交价与五档盘口，避免只更新价格而留下启动快照。 */
export function applyPriceTickMarket(market: MarketSnap, tick: PriceTickEvent): void {
  if (!Array.isArray(tick.bids) || !Array.isArray(tick.asks)) {
    throw new TypeError(`股票 ${tick.code} 的 PriceTick 缺少权威五档盘口`);
  }
  market.last_price = tick.last_price;
  market.bids = tick.bids;
  market.asks = tick.asks;
  market.best_bid = tick.bids[0]?.[0] ?? null;
  market.best_ask = tick.asks[0]?.[0] ?? null;
}
