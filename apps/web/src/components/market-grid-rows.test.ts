import assert from "node:assert/strict";
import test from "node:test";
import type { MarketSnap } from "../types/engine.ts";
import { buildMarketRows, diffMarketRows } from "./market-grid-rows.ts";

const market = (lastPrice: number): MarketSnap => ({
  last_price: lastPrice,
  last_close: 1_000,
  best_bid: null,
  best_ask: null,
  bids: [],
  asks: [],
});

test("one stock quote change produces one AG Grid row update without rebuilding peers", () => {
  const markets = { AAA: market(1_000), BBB: market(2_000) };
  const previous = buildMarketRows(markets, ["AAA", "BBB"]);
  const next = buildMarketRows({ ...markets, AAA: market(1_001) }, ["AAA", "BBB"], previous);

  const transaction = diffMarketRows(previous, next);

  assert.deepEqual(transaction.add, []);
  assert.deepEqual(transaction.remove, []);
  assert.deepEqual(transaction.update.map((row) => row.code), ["AAA"]);
  assert.equal(next[1], previous[1], "unchanged rows must retain identity for local rendering");
});

test("watchlist membership changes map to add and remove transactions", () => {
  const markets = { AAA: market(1_000), BBB: market(2_000), CCC: market(3_000) };
  const previous = buildMarketRows(markets, ["AAA", "BBB"]);
  const next = buildMarketRows(markets, ["BBB", "CCC"], previous);

  const transaction = diffMarketRows(previous, next);

  assert.deepEqual(transaction.add.map((row) => row.code), ["CCC"]);
  assert.deepEqual(transaction.remove.map((row) => row.code), ["AAA"]);
  assert.deepEqual(transaction.update, []);
});
