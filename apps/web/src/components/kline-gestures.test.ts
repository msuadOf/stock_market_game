import assert from "node:assert/strict";
import test from "node:test";
import { KlineGestures, klineTapIndex } from "./kline-gestures.ts";
test("滚轮累计缩放，Shift切换清除旧累计并左右移动", { timeout: 10000 }, () => {
 const g = new KlineGestures();
 assert.equal(g.wheel(20, false), null);
 assert.equal(g.wheel(20, false), "zoom-out");
 assert.equal(g.wheel(-40, false), "zoom-in");
 assert.equal(g.wheel(20, false), null);
 assert.equal(g.wheel(20, true), null);
 assert.equal(g.wheel(20, true), "pan-right");
 assert.equal(g.wheel(-40, true), "pan-left");
});
test("双指张开放大合拢缩小，微动不跳档", { timeout: 10000 }, () => {
 const g = new KlineGestures(); g.startPinch(100);
 assert.equal(g.pinch(110), null); assert.equal(g.pinch(120), "zoom-in");
 assert.equal(g.pinch(90), "zoom-out");
});
test("点按吸附真实K线槽位，空白与边界外不虚构蜡烛", { timeout: 10000 }, () => {
 assert.equal(klineTapIndex(15, 300, 30, 30), 1);
 assert.equal(klineTapIndex(299, 300, 30, 30), 29);
 assert.equal(klineTapIndex(150, 300, 30, 2), null);
 assert.equal(klineTapIndex(-1, 300, 30, 30), null);
});
