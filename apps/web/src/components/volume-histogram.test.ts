import assert from "node:assert/strict";
import { test } from "node:test";
import { volumeHistogramData } from "./volume-histogram.ts";

test("桌面量能柱将权威股数换算为手，保留零股量和无成交槽", () => {
  assert.deepEqual(volumeHistogramData([
    { time: 0, value: 10, volume: 250, buy: true },
    { time: 1, value: 10, volume: 1, buy: false },
    { time: 2, value: 10 },
  ]), [
    { time: 0, value: 2.5, color: "rgba(216,30,6,0.5)" },
    { time: 1, value: 0.01, color: "rgba(0,153,68,0.5)" },
    { time: 2, value: 0, color: "rgba(0,153,68,0.5)" },
  ]);
  assert.deepEqual(volumeHistogramData([]), []);
});
