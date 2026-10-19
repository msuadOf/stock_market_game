import assert from "node:assert/strict";
import test from "node:test";
import { activeMinuteKlineBucketMinute, aggregateMinuteBars, aggregateMinuteKlines, minuteKlineBucketMinute } from "./minute-kline.ts";

const bar = (minute_of_day: number, phase: "OpenAuction" | "Continuous" | "ClosingAuction", open: string, high: string, low: string, close: string, volume_shares: string, turnover_cents: string, trade_count: string) => ({ minute_of_day, phase, open, high, low, close, volume_shares, turnover_cents, trade_count });
const maximumMoney = "9223372036854775807";
const maximumU64 = "18446744073709551615";
const maximumLegalTurnover = (BigInt(maximumMoney) * BigInt(maximumU64)).toString();

test("分钟K按日期、午间时段及周期聚合真实OHLCV和u128成交额", { timeout: 10000 }, () => {
  const entries = [{ date: "2030-01-02", availability: "Traded" as const, daily_candle: null, bars: [
    bar(565, "OpenAuction", "100", "110", "100", "105", "10", "1050", "2"),
    bar(570, "Continuous", "105", "110", "105", "108", "100", "10650", "3"),
    bar(571, "Continuous", "108", "112", "107", "110", "200", "21900", "4"),
    bar(630, "Continuous", maximumMoney, maximumMoney, maximumMoney, maximumMoney, maximumU64, maximumLegalTurnover, "5"),
    bar(780, "Continuous", "109", "111", "109", "111", "100", "11000", "2"),
    bar(900, "ClosingAuction", "111", "112", "111", "112", "10", "1115", "1"),
  ] }, { date: "2030-01-03", availability: "Traded" as const, daily_candle: null, bars: [bar(570, "Continuous", "112", "113", "112", "113", "100", "11250", "2")] }];

  const result = aggregateMinuteKlines(entries, 60);
  assert.deepEqual(result.map(({ date, minute_of_day, phase }) => ({ date, minute_of_day, phase })), [
    { date: "2030-01-02", minute_of_day: 565, phase: "OpenAuction" },
    { date: "2030-01-02", minute_of_day: 570, phase: "Continuous" },
    { date: "2030-01-02", minute_of_day: 630, phase: "Continuous" },
    { date: "2030-01-02", minute_of_day: 780, phase: "Continuous" },
    { date: "2030-01-02", minute_of_day: 900, phase: "ClosingAuction" },
    { date: "2030-01-03", minute_of_day: 570, phase: "Continuous" },
  ]);
  assert.deepEqual(result[1], { date: "2030-01-02", minute_of_day: 570, phase: "Continuous", open: "105", high: "112", low: "105", close: "110", volume_shares: "300", turnover_cents: "32550", trade_count: "7" });
  assert.equal(result[2].volume_shares, "18446744073709551615");
  assert.equal(result[2].turnover_cents, maximumLegalTurnover);
  assert.equal(result[3].minute_of_day, 780, "午休后重新锚定下午桶");
});

test("分钟K不为零成交日或无成交分钟补柱，拒绝不支持周期及非成交条目", { timeout: 10000 }, () => {
  assert.deepEqual(aggregateMinuteKlines([{ date: "2030-01-02", availability: "NoTrades", daily_candle: null, bars: [] }], 5), []);
  assert.throws(() => aggregateMinuteKlines([], 2), /周期/);
  assert.throws(() => aggregateMinuteKlines([{ date: "2030-01-02", availability: "Closed", daily_candle: null, bars: [{ minute_of_day: 570, phase: "Continuous", open: "1", high: "1", low: "1", close: "1", volume_shares: "1", turnover_cents: "1", trade_count: "1" }] }], 1), /已日结真实成交/);
});

test("所有已支持周期均独立锚定上午与下午交易时段", { timeout: 10000 }, () => {
  const entry = { date: "2030-01-02", availability: "Traded" as const, daily_candle: null, bars: [
    bar(570, "Continuous", "100", "100", "100", "100", "1", "100", "1"),
    bar(689, "Continuous", "100", "100", "100", "100", "1", "100", "1"),
    bar(780, "Continuous", "100", "100", "100", "100", "1", "100", "1"),
    bar(899, "Continuous", "100", "100", "100", "100", "1", "100", "1"),
  ] };
  for (const period of [120, 60, 30, 15, 5, 1]) {
    const result = aggregateMinuteKlines([entry], period);
    const expected = [570, 570 + Math.floor(119 / period) * period, 780, 780 + Math.floor(119 / period) * period].filter((minute, index, values) => index === 0 || minute !== values[index - 1]);
    assert.deepEqual(result.map((item) => item.minute_of_day), expected, `周期${period}分钟必须独立锚定午前和午后`);
  }
});

test("活动分钟聚合直接使用Core实时成交bar，不从价格点或日K合成", { timeout: 10000 }, () => {
  const current = [bar(570, "Continuous", "100", "102", "100", "102", "100", "10100", "2"), bar(571, "Continuous", "102", "104", "101", "103", "200", "20500", "3")];
  assert.deepEqual(aggregateMinuteBars("2030-01-02", current, 5), [{ date: "2030-01-02", minute_of_day: 570, phase: "Continuous", open: "100", high: "104", low: "100", close: "103", volume_shares: "300", turnover_cents: "30600", trade_count: "5" }]);
});

test("Core允许的连续竞价边界分钟归入该半日最后一桶", { timeout: 10000 }, () => {
  for (const period of [120, 60, 30, 15, 5, 1]) {
    assert.equal(minuteKlineBucketMinute(690, "Continuous", period), 570 + Math.floor(119 / period) * period);
    assert.equal(minuteKlineBucketMinute(900, "Continuous", period), 780 + Math.floor(119 / period) * period);
  }
  assert.deepEqual(aggregateMinuteBars("2030-01-02", [bar(570, "Continuous", "100", "100", "100", "100", "1", "100", "1"), bar(690, "Continuous", "101", "101", "101", "101", "1", "101", "1")], 120).map((item) => item.minute_of_day), [570]);
  assert.deepEqual(aggregateMinuteBars("2030-01-02", [bar(899, "Continuous", "100", "100", "100", "100", "1", "100", "1"), bar(900, "Continuous", "101", "101", "101", "101", "1", "101", "1")], 120).map((item) => item.minute_of_day), [780]);
});

test("活动观察处于午休或15:00边界时没有形成中连续竞价桶", { timeout: 10000 }, () => {
  assert.equal(activeMinuteKlineBucketMinute(720, 5), null);
  assert.equal(activeMinuteKlineBucketMinute(690, 5), null);
  assert.equal(activeMinuteKlineBucketMinute(900, 5), null);
});

test("聚合验证自然日和规范u64/u128上界，不输出wire非法数据", { timeout: 10000 }, () => {
  assert.throws(() => aggregateMinuteBars("2030-02-30", [], 1), /自然日/);
  const small = (minute: number, qty: string, count: string) => bar(minute, "Continuous", "1", "1", "1", "1", qty, qty, count);
  assert.throws(() => aggregateMinuteBars("2030-01-02", [small(570, maximumU64, "1"), small(571, "1", "1")], 5), /u64/);
  assert.throws(() => aggregateMinuteBars("2030-01-02", [small(570, maximumU64, maximumU64), small(571, "1", "1")], 5), /u64/);
  assert.throws(() => aggregateMinuteBars("2030-01-02", [small(570, "1", "1"), { ...small(571, "1", "1"), turnover_cents: "340282366920938463463374607431768211456" }], 5), /u128/);
});

test("纯聚合入口拒绝坏阶段、非法OHLC和超出价格范围的成交额", { timeout: 10000 }, () => {
  const valid = bar(570, "Continuous", "100", "110", "90", "105", "10", "1050", "2");
  assert.throws(() => aggregateMinuteBars("2030-01-02", [{ ...valid, phase: "Other" as "Continuous" }], 1), /阶段/);
  assert.throws(() => aggregateMinuteBars("2030-01-02", [{ ...valid, open: "0100" }], 1), /规范/);
  assert.throws(() => aggregateMinuteBars("2030-01-02", [{ ...valid, low: "0" }], 1), /OHLC/);
  assert.throws(() => aggregateMinuteBars("2030-01-02", [{ ...valid, turnover_cents: "899" }], 1), /成交额/);
  assert.throws(() => minuteKlineBucketMinute(570.5, "Continuous", 1), /非负安全整数/);
  assert.throws(() => minuteKlineBucketMinute(570, "other" as "Continuous", 1), /阶段/);
});

test("历史自然日不允许重复或倒序", { timeout: 10000 }, () => {
  const first = { date: "2030-01-02", availability: "NoTrades" as const, daily_candle: null, bars: [] };
  const second = { ...first, date: "2030-01-03" };
  assert.throws(() => aggregateMinuteKlines([first, first], 1), /严格递增/);
  assert.throws(() => aggregateMinuteKlines([second, first], 1), /严格递增/);
});
