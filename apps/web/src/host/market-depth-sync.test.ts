import assert from "node:assert/strict";
import test from "node:test";
import { applyPriceTickMarket } from "../store/market-depth-sync.ts";
import type { MarketSnap, PriceTickEvent } from "../types/engine.ts";

test("PriceTick refreshes the visible top-five order book instead of leaving the initial snapshot stale", () => {
  const market: MarketSnap = {
    last_price: "1000",
    last_close: "1000",
    cash_ex_reference_pending_trade: false,
    day_market_activity: false,
    last_cash_ex_reference: null,
    best_bid: "999",
    best_ask: "1001",
    bids: [["999", 100]],
    asks: [["1001", 200]],
  };
  const tick: PriceTickEvent = {
    seq: 2,
    tick: 2,
    code: "600101",
    last_price: "1002",
    daily_candle: { time: 1893456000, open: "1000", high: "1002", low: "1000", close: "1002", volume: 300 },
    bids: [["1001", 300], ["1000", 500]],
    asks: [["1003", 400], ["1004", 600]],
  };

  applyPriceTickMarket(market, tick);

  assert.deepEqual(market.bids, [["1001", 300], ["1000", 500]]);
  assert.deepEqual(market.asks, [["1003", 400], ["1004", 600]]);
  assert.equal(market.best_bid, "1001");
  assert.equal(market.best_ask, "1003");
});
