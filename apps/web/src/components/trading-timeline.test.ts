import assert from "node:assert/strict";
import test from "node:test";
import { TradingTimeline } from "./trading-timeline.ts";

test("非三整除开盘沿用 engine civil 时钟，不另造09:25边界", { timeout: 10000 }, () => {
  const timeline = new TradingTimeline({ ticks_per_day: 31, auction_ticks: 10, closing_auction_ticks: 3 });
  assert.equal(timeline.auctionEntryTicks, 7);
  assert.equal(timeline.clock(7), "09:25:30");
  assert.equal(timeline.tradeTime(7), "09:25:30");
  assert.equal(timeline.clock(10), "09:30:00");
  assert.equal(timeline.clock(28), "14:57:00");
  assert.equal(timeline.tradeTime(31), "15:00:00");
  assert.equal(timeline.auctionSlot(7, true), 99);
});

test("无开盘或收盘的简化局不除零，分钟和跨日端点仍有效", { timeout: 10000 }, () => {
  const timeline = new TradingTimeline({ ticks_per_day: 4, auction_ticks: 0, closing_auction_ticks: 0 });
  assert.equal(timeline.clock(0), "09:30:00");
  assert.match(timeline.displayNote!, /无开盘集合竞价及 PreOpen/);
  assert.match(timeline.displayNote!, /无收盘集合竞价/);
  assert.equal(timeline.clock(2), "13:00:00");
  assert.equal(timeline.tradeTime(2), "11:30:00");
  assert.equal(timeline.tradeTime(4), "15:00:00");
  assert.equal(timeline.clock(4), "09:30:00");
  assert.deepEqual([1, 2, 3, 4].map(tick => timeline.minuteSlot(tick)), [0, 60, 120, 180]);
  assert.equal(timeline.eventDay(4), 0);
  assert.equal(timeline.eventDay(5), 1);
  assert.throws(() => timeline.auctionSlot(1), /集合竞价窗口/);
  const oneOpening = new TradingTimeline({ ticks_per_day: 4, auction_ticks: 1, closing_auction_ticks: 0 });
  assert.match(oneOpening.displayNote!, /无独立 PreOpen/);
  assert.equal(oneOpening.tradeTime(1), "09:30:00");
  assert.equal(oneOpening.auctionSlot(1, true), 99);
});

test("开盘集合竞价窗口内不提供形成中分钟槽——起局首渲染不得抛错", { timeout: 10000 }, () => {
  // 会话从 tick 1 起处于开盘集合竞价窗口（本地起局首快照即此形态）：
  // 连续竞价尚未开始，"形成中分钟桶"不存在，须返回 null 而非抛
  // RangeError（否则分钟面板首渲染即触发 UI 错误边界，见 2026-10-20
  // smoke 第 4 层断裂）。
  const timeline = new TradingTimeline({ ticks_per_day: 30, auction_ticks: 9, closing_auction_ticks: 3 });
  assert.equal(timeline.currentMinuteSlot(0), null);
  assert.equal(timeline.currentMinuteSlot(1), null);
  assert.equal(timeline.currentMinuteSlot(9), null);
  assert.equal(timeline.currentMinuteSlot(10), 0);
  assert.equal(timeline.currentMinuteSlot(11), 13);
  // 默认局（含真实开盘窗口）起局 tick 1 同样为 null；无开盘窗口的简化局
  // 从 tick 1 起即有形成中分钟槽。
  const standard = new TradingTimeline();
  assert.equal(standard.currentMinuteSlot(1), null);
  const noOpening = new TradingTimeline({ ticks_per_day: 4, auction_ticks: 0, closing_auction_ticks: 0 });
  assert.equal(noOpening.currentMinuteSlot(1), 0);
});

test("非法 timing 与事件不能被静默修补到零分钟", { timeout: 10000 }, () => {
  for (const timing of [
    { ticks_per_day: 0, auction_ticks: 0, closing_auction_ticks: 0 },
    { ticks_per_day: 30, auction_ticks: 28, closing_auction_ticks: 2 },
    { ticks_per_day: 30, auction_ticks: 9.5, closing_auction_ticks: 3 },
    { ticks_per_day: Number.MAX_SAFE_INTEGER + 1, auction_ticks: 9, closing_auction_ticks: 3 },
  ]) assert.throws(() => new TradingTimeline(timing), RangeError);
  const timeline = new TradingTimeline();
  assert.throws(() => timeline.eventDay(0), /tick/);
  assert.throws(() => timeline.minuteSlot(10), /开盘窗口/);
  assert.throws(() => timeline.auctionSlot(601), /集合竞价窗口/);
});
