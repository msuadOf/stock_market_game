import assert from "node:assert/strict";
import test from "node:test";
import { auctionContinuousJoin, auctionDisplayPoints } from "./intraday-auction-display.ts";

test("无指示价显示昨收参考值，原始null不变且不产生更新点", { timeout: 10000 }, () => {
  const points = [{ time: 0, value: null, volume: 0, buy: false }, { time: 1, value: 11, volume: 100, buy: true }, { time: 2, value: 11, volume: 100, buy: true }, { time: 3, value: 11, volume: 200, buy: true }, { time: 4, value: null, volume: 0, buy: false }];
  const result = auctionDisplayPoints(points, 10);
  assert.deepEqual(result.map(p => p.value), [10, 11, 11, 11, 10]);
  assert.deepEqual(result.map(p => p.updated), [false, true, false, true, false]);
  assert.equal(points[0].value, null);
  assert.deepEqual(auctionDisplayPoints([], 10), []);
});

test("09:30连接只引用同坐标已有价格，不改变源记录或把参考轴当成交", { timeout: 10000 }, () => {
  const raw = [{ time: 99, value: null, volume: 0, buy: false }];
  const auction = auctionDisplayPoints(raw, 10);
  const continuous = [{ time: 0, value: 12 }];
  assert.deepEqual(auctionContinuousJoin(auction, continuous), [{ x: 16, value: 10 }, { x: 16, value: 12 }]);
  assert.equal(raw[0].value, null);
  assert.equal(auction[0].updated, false);
  assert.deepEqual(continuous, [{ time: 0, value: 12 }]);
  assert.deepEqual(auctionContinuousJoin([], continuous), []);
  assert.deepEqual(auctionContinuousJoin(auction, []), []);
  assert.deepEqual(auctionContinuousJoin(auction, [{ time: 1, value: 12 }]), []);
  assert.deepEqual(auctionContinuousJoin([{ time: 98, value: 10 }], continuous), []);
  assert.deepEqual(auctionContinuousJoin(auction, [{ time: 0, value: 10 }]), []);
});
