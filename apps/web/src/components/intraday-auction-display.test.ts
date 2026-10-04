import assert from "node:assert/strict";
import test from "node:test";
import { auctionDisplayPoints } from "./intraday-auction-display.ts";

test("无指示价显示昨收参考值，原始null不变且不产生更新点", { timeout: 10000 }, () => {
  const points = [{ time: 0, value: null, volume: 0, buy: false }, { time: 1, value: 11, volume: 100, buy: true }, { time: 2, value: 11, volume: 100, buy: true }, { time: 3, value: 11, volume: 200, buy: true }, { time: 4, value: null, volume: 0, buy: false }];
  const result = auctionDisplayPoints(points, 10);
  assert.deepEqual(result.map(p => p.value), [10, 11, 11, 11, 10]);
  assert.deepEqual(result.map(p => p.updated), [false, true, false, true, false]);
  assert.equal(points[0].value, null);
  assert.deepEqual(auctionDisplayPoints([], 10), []);
});
