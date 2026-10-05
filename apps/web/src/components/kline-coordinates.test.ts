import assert from "node:assert/strict";
import { test } from "node:test";
import { coordinateTicks, formatCoordinateValue, klineTimeTicks, klineTradingDayLabel, klineVolumeTicks } from "./kline-coordinates.ts";
import { MobileKlineProjection } from "../mobile/market-model.ts";
import type { KlinePoint } from "./PriceChart.tsx";

test("坐标保留微小幅度与零点，不用重复的两位小数掩盖不同位置", { timeout: 10000 }, () => {
  assert.deepEqual(coordinateTicks(0, 0, () => 36), [{ value: 0, y: 36 }]);
  assert.deepEqual(coordinateTicks(-2, 4, value => 68 - (value + 2) / 6 * 64, 3).map(tick => tick.value), [4, 1, -2]);
  assert.equal(formatCoordinateValue(.001, .0005), "0.0010");
  assert.equal(formatCoordinateValue(12, .75), "12.00");
  assert.equal(formatCoordinateValue(-.00000001, 1), "0.00");
  const tiny = coordinateTicks(0, 1e-10, value => value, 3).map(tick => formatCoordinateValue(tick.value, 5e-11));
  assert.deepEqual(tiny, ["1.00e-10", "5.00e-11", "0"]);
  assert.equal(new Set(tiny).size, 3);
  const narrow = coordinateTicks(10, 10 + 1e-10, value => value).map(tick => formatCoordinateValue(tick.value, 2.5e-11));
  assert.equal(new Set(narrow).size, 5, "非零基数的窄幅值也必须显示不同刻度");
});

test("时间坐标沿真实槽位吸附，稀疏历史不拉满，开局前和周K不伪造公历", { timeout: 10000 }, () => {
  const candles: KlinePoint[] = Array.from({ length: 75 }, (_, index) => ({ time: (index * 86400) as KlinePoint["time"], open: 10, high: 12, low: 9, close: 11 }));
  const p = MobileKlineProjection.fromInputs(candles, { capacity: 72, offsetFromEnd: 0 }, { kind: "idle" });
  const ticks = klineTimeTicks(p);
  assert.equal(ticks[0].label, "第4日");
  assert.equal(ticks.at(-1)?.label, "第75日");
  assert.equal(ticks[0].x, p.slotFor(0).center);
  assert.equal(ticks.at(-1)?.x, p.slotFor(71).center);
  const sparse = MobileKlineProjection.fromInputs(candles.slice(0, 2), { capacity: 72, offsetFromEnd: 0 }, { kind: "idle" });
  assert.equal(klineTimeTicks(sparse).length, 1);
  assert.equal(klineTimeTicks(sparse)[0].x, sparse.slotFor(0).center);
  assert.equal(klineTradingDayLabel(-86400), "前1日");
  assert.equal(klineTradingDayLabel(-86400, false), "开局前1个交易日");
  assert.equal(klineTradingDayLabel(86400 * 5), "第6日");
  assert.equal(klineTradingDayLabel(86400, false), "第2个交易日");
});

test("奇数股量的中间刻度仍为整数股并投影到对应量高，零和一股不重复", { timeout: 10000 }, () => {
  assert.deepEqual(klineVolumeTicks(3), [{ value: 3, y: 9 }, { value: 2, y: 31 }, { value: 0, y: 75 }]);
  assert.deepEqual(klineVolumeTicks(1), [{ value: 1, y: 9 }, { value: 0, y: 75 }]);
  assert.deepEqual(klineVolumeTicks(0), [{ value: 0, y: 75 }]);
});

test("非法坐标范围、数量和股数显式失败，不生成NaN刻度", { timeout: 10000 }, () => {
  for (const args of [[2, 1, 3], [0, Infinity, 3], [NaN, 1, 3], [0, 1, 1], [0, 1, 2.5]]) {
    assert.throws(() => coordinateTicks(args[0], args[1], value => value, args[2]), RangeError);
  }
  for (const value of [-1, .5, Infinity, Number.MAX_SAFE_INTEGER + 1]) assert.throws(() => klineVolumeTicks(value), RangeError);
});
