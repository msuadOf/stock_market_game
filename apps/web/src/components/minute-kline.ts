import type { MarketHistoryPage, MinuteBar } from "../host/market-history.ts";
import { tradeHistoryDate } from "../host/personal-trade-history.ts";
import { parseDecimalId } from "../host/protocol/wire-values.ts";
import { parseMoney } from "../utils/money.ts";
import { parseTurnoverCents } from "../utils/turnover.ts";

type MarketHistoryEntry = MarketHistoryPage["entries"][number];

export const MINUTE_KLINE_PERIODS = [120, 60, 30, 15, 5, 1] as const;
export type MinuteKlinePeriod = typeof MINUTE_KLINE_PERIODS[number];
export type MinuteKline = Omit<MinuteBar, "minute_of_day" | "phase"> & {
  date: string;
  minute_of_day: number;
  phase: MinuteBar["phase"];
};

function assertPeriod(period: number): asserts period is MinuteKlinePeriod {
  if (!MINUTE_KLINE_PERIODS.includes(period as MinuteKlinePeriod)) throw new RangeError(`分钟K周期必须为${MINUTE_KLINE_PERIODS.join("/")}分钟`);
}

const U64_MAX = 18_446_744_073_709_551_615n;

function continuousSessionStart(minute: number): { start: number; end: number } {
  if (minute >= 570 && minute <= 690) return { start: 570, end: 690 };
  if (minute >= 780 && minute <= 900) return { start: 780, end: 900 };
  throw new RangeError(`连续竞价分钟超出交易时段：${minute}`);
}

export function minuteKlineBucketMinute(minute: number, phase: MinuteBar["phase"], period: number): number {
  assertPeriod(period);
  if (!Number.isSafeInteger(minute) || minute < 0) throw new RangeError("分钟标签必须是非负安全整数");
  if (phase !== "OpenAuction" && phase !== "ClosingAuction" && phase !== "Continuous") throw new RangeError("分钟成交阶段无效");
  if (phase !== "Continuous") return minute;
  const { start, end } = continuousSessionStart(minute);
  const bucketMinute = Math.min(minute, end - 1);
  return start + Math.floor((bucketMinute - start) / period) * period;
}

export function activeMinuteKlineBucketMinute(minute: number, period: number): number | null {
  assertPeriod(period);
  if (!((minute >= 570 && minute < 690) || (minute >= 780 && minute < 900))) return null;
  return minuteKlineBucketMinute(minute, "Continuous", period);
}

function positiveU64(value: string, path: string): bigint {
  const parsed = parseDecimalId(value, path);
  if (parsed !== value || parsed === "0") throw new RangeError(`${path}必须为正规范u64字符串`);
  const integer = BigInt(parsed);
  if (integer > U64_MAX) throw new RangeError(`${path}超出u64范围`);
  return integer;
}

function validatedBar(bar: MinuteBar): { volume: bigint; turnover: bigint; count: bigint } {
  const volume = positiveU64(bar.volume_shares, "volume_shares");
  const count = positiveU64(bar.trade_count, "trade_count");
  const turnover = BigInt(parseTurnoverCents(bar.turnover_cents, "turnover_cents"));
  if (turnover === 0n) throw new RangeError("真实成交分钟的成交额必须为正");
  if (count > volume) throw new RangeError("分钟真实成交笔数不能多于成交股数");
  const open = BigInt(parseMoney(bar.open, "minute.open"));
  const high = BigInt(parseMoney(bar.high, "minute.high"));
  const low = BigInt(parseMoney(bar.low, "minute.low"));
  const close = BigInt(parseMoney(bar.close, "minute.close"));
  if (low <= 0n || low > high || open < low || open > high || close < low || close > high) throw new RangeError("分钟K线OHLC价格范围不合法");
  if (count > volume || turnover < low * volume || turnover > high * volume) throw new RangeError("分钟成交额与真实价格、股数范围不一致");
  if (!Number.isSafeInteger(bar.minute_of_day) || bar.minute_of_day < 0) throw new RangeError("分钟标签必须是非负安全整数");
  if (bar.phase === "OpenAuction" && bar.minute_of_day !== 565) throw new RangeError("开盘集合竞价分钟标签无效");
  if (bar.phase === "ClosingAuction" && bar.minute_of_day !== 900) throw new RangeError("收盘集合竞价分钟标签无效");
  if (bar.phase === "Continuous") continuousSessionStart(bar.minute_of_day);
  if (bar.phase !== "OpenAuction" && bar.phase !== "ClosingAuction" && bar.phase !== "Continuous") throw new RangeError("分钟成交阶段无效");
  return { volume, turnover, count };
}

function checkedU64(value: bigint, path: string): string {
  if (value <= 0n || value > U64_MAX) throw new RangeError(`${path}累计值超出正u64范围`);
  return parseDecimalId(value.toString(), path);
}

export function aggregateMinuteBars(date: string, bars: readonly MinuteBar[], period: number): MinuteKline[] {
  assertPeriod(period);
  tradeHistoryDate(date, "minute_kline.date");
  const result: MinuteKline[] = [];
  let previousMinute = -1;
  for (const bar of bars) {
    if (bar.minute_of_day <= previousMinute) throw new Error("分钟成交事实必须按时刻递增");
    previousMinute = bar.minute_of_day;
    const amounts = validatedBar(bar);
    const bucketMinute = minuteKlineBucketMinute(bar.minute_of_day, bar.phase, period);
    const previous = result.at(-1);
    if (previous?.minute_of_day === bucketMinute && previous.phase === bar.phase) {
      previous.high = (BigInt(previous.high) >= BigInt(bar.high) ? previous.high : bar.high) as MinuteKline["high"];
      previous.low = (BigInt(previous.low) <= BigInt(bar.low) ? previous.low : bar.low) as MinuteKline["low"];
      previous.close = bar.close;
      const volume = checkedU64(BigInt(previous.volume_shares) + amounts.volume, "volume_shares");
      const turnover = parseTurnoverCents((BigInt(previous.turnover_cents) + amounts.turnover).toString(), "turnover_cents");
      const count = checkedU64(BigInt(previous.trade_count) + amounts.count, "trade_count");
      previous.volume_shares = volume;
      previous.turnover_cents = turnover;
      previous.trade_count = count;
    } else {
      result.push({ ...bar, date, minute_of_day: bucketMinute });
    }
  }
  return result;
}

export function aggregateMinuteKlines(entries: readonly MarketHistoryEntry[], period: number): MinuteKline[] {
  assertPeriod(period);
  const result: MinuteKline[] = [];
  let previousDate: string | null = null;
  for (const entry of entries) {
    tradeHistoryDate(entry.date, "minute_kline.entry.date");
    if (previousDate !== null && entry.date <= previousDate) throw new Error("分钟历史自然日期必须严格递增且不可重复");
    previousDate = entry.date;
    if (entry.availability !== "Traded") {
      if (entry.bars.length !== 0) throw new Error("仅已日结真实成交日期可以生成分钟K");
      continue;
    }
    if (entry.bars.length === 0) throw new Error("有成交日期必须包含真实分钟成交事实");
    result.push(...aggregateMinuteBars(entry.date, entry.bars, period));
  }
  return result;
}
