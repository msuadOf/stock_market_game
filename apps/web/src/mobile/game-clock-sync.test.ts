import assert from "node:assert/strict";
import test from "node:test";
import { syncSnapshotTick } from "../store/snapshot-clock.ts";

test("价格事件把权威游戏 tick 同步到 Redux 时钟", () => {
  const snapshot = { tick: 0 };
  syncSnapshotTick(snapshot, [
    {
      PriceTick: {
        seq: 9,
        tick: 7_200,
        code: "000812",
        last_price: 286,
        daily_candle: { time: 0, open: 285, high: 286, low: 285, close: 286, volume: 1 },
      },
    },
  ]);

  assert.equal(snapshot.tick, 7_200);
});

test("旧批次事件不能让游戏时钟倒退", () => {
  const snapshot = { tick: 9_000 };
  syncSnapshotTick(snapshot, [{ PriceTick: { tick: 8_999 } }]);
  assert.equal(snapshot.tick, 9_000);
});

test("价格事件缺失合法 tick 时显式报错", () => {
  assert.throws(() => syncSnapshotTick({ tick: 0 }, [{ PriceTick: {} }]), /PriceTick\.tick/);
});
