import assert from "node:assert/strict";
import test from "node:test";
import { applyPriceTickMarket } from "../store/market-depth-sync.ts";
import type { MarketSnap, PriceTickEvent } from "../types/engine.ts";

test("PriceTick refreshes the visible top-five order book instead of leaving the initial snapshot stale", () => {
  const market: MarketSnap = {
    last_price: 1_000,
    last_close: 1_000,
    best_bid: 999,
    best_ask: 1_001,
    fundamental_value: 1_000,
    bids: [[999, 100]],
    asks: [[1_001, 200]],
  };
  const tick: PriceTickEvent = {
    seq: 2,
    tick: 2,
    code: "600101",
    last_price: 1_002,
    daily_candle: { time: 0, open: 1_000, high: 1_002, low: 1_000, close: 1_002, volume: 300 },
    bids: [[1_001, 300], [1_000, 500]],
    asks: [[1_003, 400], [1_004, 600]],
  };

  applyPriceTickMarket(market, tick);

  assert.deepEqual(market.bids, [[1_001, 300], [1_000, 500]]);
  assert.deepEqual(market.asks, [[1_003, 400], [1_004, 600]]);
  assert.equal(market.best_bid, 1_001);
  assert.equal(market.best_ask, 1_003);
});
