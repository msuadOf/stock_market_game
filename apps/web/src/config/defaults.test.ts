import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { DEFAULT_SETUP, STOCK_LIST } from "./defaults.ts";

describe("game watchlist seed", () => {
  it("keeps the game's own securities independent from visual references", () => {
    assert.deepEqual(STOCK_LIST.map((stock) => stock.code), [
      "600101", "002156", "300260", "600610", "000812",
    ]);
    assert.equal(DEFAULT_SETUP.stocks.find((stock) => stock.code === "600101")?.initial_price, 1120);
  });
});
