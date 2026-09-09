import assert from "node:assert/strict";
import test from "node:test";
import { applyEvents, setSnapshot, snapshotReducer } from "./store.ts";

const SNAPSHOT = {
  seq: 0,
  tick: 0,
  day: 0,
  phase: "Continuous" as const,
  markets: {
    "600101": { last_price: 1_000, last_close: 1_000, best_bid: null, best_ask: null, bids: [], asks: [], price_history: [] },
    "000001": { last_price: 2_000, last_close: 2_000, best_bid: null, best_ask: null, bids: [], asks: [], price_history: [] },
  },
  accounts: {},
  daily_candles: {},
  active_daily_candles: {},
};

test("行情事件只替换命中的股票分支，不刷新账户和其它股票状态", () => {
  const before = snapshotReducer(undefined, setSnapshot(SNAPSHOT));
  const beforeSnapshot = before.snapshot!;
  const after = snapshotReducer(before, applyEvents([{ PriceTick: {
    seq: 1,
    tick: 1,
    code: "600101",
    last_price: 1_001,
    daily_candle: { time: 0, open: 1_000, high: 1_001, low: 1_000, close: 1_001, volume: 100 },
    bids: [[1_000, 100]],
    asks: [[1_002, 100]],
  } }]));

  assert.notEqual(after.snapshot, beforeSnapshot);
  assert.notEqual(after.snapshot!.markets["600101"], beforeSnapshot.markets["600101"]);
  assert.equal(after.snapshot!.markets["000001"], beforeSnapshot.markets["000001"]);
  assert.equal(after.snapshot!.accounts, beforeSnapshot.accounts);
});
