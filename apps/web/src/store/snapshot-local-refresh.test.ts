import assert from "node:assert/strict";
import test from "node:test";
import { applyProtocolFrame, setSnapshot, snapshotReducer } from "./store.ts";

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

test("协议时间序列帧原子更新权威行情和游戏时钟", () => {
  const before = snapshotReducer(undefined, setSnapshot(SNAPSHOT));
  const beforeSnapshot = before.snapshot!;
  const after = snapshotReducer(before, applyProtocolFrame({
    tick: 1,
    seq: 1,
    markets: {
      ...SNAPSHOT.markets,
      "600101": { ...SNAPSHOT.markets["600101"], last_price: 1_001, bids: [[1_000, 100]], asks: [[1_002, 100]] },
    },
    activeDailyCandles: {},
  }));

  assert.notEqual(after.snapshot, beforeSnapshot);
  assert.notEqual(after.snapshot!.markets["600101"], beforeSnapshot.markets["600101"]);
  assert.equal(after.snapshot!.markets["000001"], beforeSnapshot.markets["000001"]);
  assert.equal(after.snapshot!.accounts, beforeSnapshot.accounts);
});

test("协议帧拒绝倒退序列，保持当前权威快照", () => {
  const before = snapshotReducer(undefined, setSnapshot(SNAPSHOT));
  const after = snapshotReducer(before, applyProtocolFrame({
    tick: 1,
    seq: -1,
    markets: {},
    activeDailyCandles: {},
  }));

  assert.equal(after.snapshot!.phase, "Continuous");
  assert.equal(after.snapshot!.markets["600101"].last_price, 1_000);
});
