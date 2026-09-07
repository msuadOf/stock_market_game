import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { CALL_AUCTION_TICKS, DEFAULT_SETUP, STOCK_LIST, TOTAL_TICKS_PER_DAY } from "./defaults.ts";

describe("game watchlist seed", () => {
  it("keeps the game's own securities independent from visual references", () => {
    assert.deepEqual(STOCK_LIST.map((stock) => stock.code), [
      "600101", "002156", "300260", "600610", "000812",
    ]);
    assert.equal(DEFAULT_SETUP.stocks.find((stock) => stock.code === "600101")?.initial_price, 1120);
  });

  it("starts every new game with a 15-minute call auction", () => {
    assert.equal(DEFAULT_SETUP.auction_ticks, CALL_AUCTION_TICKS);
    assert.equal(DEFAULT_SETUP.ticks_per_day, TOTAL_TICKS_PER_DAY);
    assert.equal(CALL_AUCTION_TICKS, 900);
    assert.equal(TOTAL_TICKS_PER_DAY, 15_300);
  });

  it("uses whole-lot NPC orders so trade quantities match the A-share lot contract", () => {
    const lotSize = DEFAULT_SETUP.config.lot_size;
    const sizes = [
      DEFAULT_SETUP.strategy_params.retail.order_size_mean,
      DEFAULT_SETUP.strategy_params.inst.order_size,
      DEFAULT_SETUP.strategy_params.hot.order_size,
    ];

    assert.ok(sizes.every((size) => size >= lotSize && size % lotSize === 0));
  });

  it("gives every NPC the documented ten-million-yuan liquidity reserve", () => {
    assert.equal(DEFAULT_SETUP.npcs.cash_per_npc, 1_000_000_000);
  });
});
