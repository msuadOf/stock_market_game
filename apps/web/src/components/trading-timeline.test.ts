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
  assert.equal(timeline.currentMinuteOfDay(0), null);
  assert.equal(timeline.currentMinuteOfDay(1), null);
  assert.equal(timeline.currentMinuteOfDay(9), null);
  assert.equal(timeline.currentMinuteOfDay(10), 570);
  // engine 将 18 个连续 tick 映射到 240 个游戏分钟；tick 11 是第 13 个桶，
  // 其墙钟分钟为 09:43（583），不是桶索引 13。
  assert.equal(timeline.currentMinuteOfDay(11), 583);
  // 默认局（含真实开盘窗口）起局 tick 1 同样为 null；无开盘窗口的简化局
  // 从 tick 1 起即有形成中分钟槽。
  const standard = new TradingTimeline();
  assert.equal(standard.currentMinuteOfDay(1), null);
  const noOpening = new TradingTimeline({ ticks_per_day: 4, auction_ticks: 0, closing_auction_ticks: 0 });
  assert.equal(noOpening.currentMinuteOfDay(1), 570);
});

test("形成中观察分钟与 engine MinuteBarFact.minute_of_day 同为墙钟口径", { timeout: 10000 }, () => {
  // 回归：panel 的 currentMinute 与 engine 分钟 bar 的 minute_of_day 相比，
  // 若误用 0..239 桶索引会在 09:30 显示 00:00，并让 activeMinute 永远匹配
  // 不到 570..690/780..900 的真实桶。
  const timeline = new TradingTimeline();
  assert.equal(timeline.currentMinuteOfDay(900), null);
  assert.equal(timeline.currentMinuteOfDay(901), 570);
  assert.equal(timeline.currentMinuteOfDay(961), 571);
  // 默认连续窗 14220 tick 映射到 240 个游戏分钟；第 120 桶从 tick 8011
  // 开始，tick 8101 已进入第 121 桶（781）。
  assert.equal(timeline.currentMinuteOfDay(8011), 780);
  assert.equal(timeline.currentMinuteOfDay(8101), 781);
  assert.equal(timeline.currentMinuteOfDay(15120), 899);
  assert.equal(timeline.currentMinuteOfDay(15121), null);

  const shortDay = new TradingTimeline({ ticks_per_day: 15300, auction_ticks: 900, closing_auction_ticks: 180 });
  assert.equal(shortDay.currentMinuteOfDay(0), null);
  assert.equal(shortDay.currentMinuteOfDay(900), null);
  assert.equal(shortDay.currentMinuteOfDay(901), 570);
  assert.equal(shortDay.currentMinuteOfDay(16201), 570);
  assert.equal(shortDay.currentMinuteOfDay(30601), null);
  assert.throws(() => shortDay.minuteSlot(15301), /开盘窗口/);
});

test("无收盘窗的日末 tick 仍属连续窗，跨日才没有当前分钟", { timeout: 10000 }, () => {
  // 无收盘竞价时没有“收盘窗口挡在日界前”的简化：末 tick 是连续窗最后
  // 一拍，tradeTime 为 15:00；分钟K键仍是该拍覆盖区间的首分钟 14:59。
  const noClosing = new TradingTimeline({ ticks_per_day: 15300, auction_ticks: 900, closing_auction_ticks: 0 });
  assert.equal(noClosing.tradeTime(15300), "15:00:00");
  assert.equal(noClosing.currentMinuteOfDay(15300), 899);
  // 下一日第一拍回到开盘窗口，连续竞价尚未开始。
  assert.equal(noClosing.currentMinuteOfDay(15301), null);
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
