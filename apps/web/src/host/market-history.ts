import type { MarketHistoryRequest } from "../types/generated/MarketHistoryRequest";
import type { MarketHistoryPage } from "../types/generated/MarketHistoryPage";
import type { MinuteBar } from "../types/generated/MinuteBar";
import { exact, field, record, values } from "./protocol/guards.ts";
import { parseDailyCandle } from "./protocol/wire-values.ts";
import { tradeHistoryDate } from "./personal-trade-history.ts";
import { parseMoney } from "../utils/money.ts";
import { parseTurnoverCents } from "../utils/turnover.ts";
export type { MarketHistoryRequest, MarketHistoryPage, MinuteBar };

function quantity(value: unknown, path: string): string {
  if (typeof value !== "string" || value.length > 20 || !/^[1-9]\d*$/.test(value) || BigInt(value) > 18446744073709551615n) throw new Error(`${path} 必须为正规范u64字符串`);
  return value;
}

export function normalizeMarketHistoryRequest(value: unknown): MarketHistoryRequest {
  const source = record(value, "公开历史请求");
  exact(source, ["code", "date_from", "date_to", "after", "page_size"], "公开历史请求");
  const code = field(source, "code", "公开历史请求");
  if (typeof code !== "string" || !/^\d{6}$/.test(code)) throw new Error("公开历史证券必须为六位代码");
  const from = tradeHistoryDate(source.date_from, "date_from");
  const to = tradeHistoryDate(source.date_to, "date_to");
  const after = source.after === null ? null : tradeHistoryDate(source.after, "after");
  if (from > to || (after !== null && (after < from || after > to))) throw new Error("公开历史日期范围或游标不合法");
  if (typeof source.page_size !== "number" || !Number.isInteger(source.page_size) || source.page_size <= 0 || source.page_size > 4294967295) throw new Error("公开历史page_size必须为正u32");
  return { code, date_from: from, date_to: to, after, page_size: source.page_size };
}

export function normalizeMinuteBar(value: unknown): MinuteBar {
  const source = record(value, "分钟bar");
  exact(source, ["minute_of_day", "phase", "open", "high", "low", "close", "volume_shares", "turnover_cents", "trade_count"], "分钟bar");
  const minute = source.minute_of_day;
  if (typeof minute !== "number" || !Number.isInteger(minute)) throw new Error("分钟标签必须为整数");
  const phase = source.phase;
  if ((phase === "OpenAuction" && minute !== 565) || (phase === "ClosingAuction" && minute !== 900) || (phase === "Continuous" && !((minute >= 570 && minute <= 690) || (minute >= 780 && minute <= 900))) || !["OpenAuction", "ClosingAuction", "Continuous"].includes(String(phase))) throw new Error("分钟成交阶段与自然时间不一致");
  const open = parseMoney(source.open), high = parseMoney(source.high), low = parseMoney(source.low), close = parseMoney(source.close);
  if (BigInt(low) <= 0n || BigInt(high) < BigInt(low) || BigInt(open) < BigInt(low) || BigInt(open) > BigInt(high) || BigInt(close) < BigInt(low) || BigInt(close) > BigInt(high)) throw new Error("分钟OHLC必须为真实正价格范围");
  const volume = quantity(source.volume_shares, "volume_shares"), count = quantity(source.trade_count, "trade_count"), turnover = parseTurnoverCents(source.turnover_cents, "turnover_cents");
  if (BigInt(count) > BigInt(volume) || BigInt(turnover) < BigInt(low) * BigInt(volume) || BigInt(turnover) > BigInt(high) * BigInt(volume)) throw new Error("分钟成交量、笔数与金额不一致");
  return { minute_of_day: minute, phase: phase as MinuteBar["phase"], open, high, low, close, volume_shares: volume, trade_count: count, turnover_cents: turnover };
}

export function normalizeMarketHistoryPage(value: unknown, request: MarketHistoryRequest): MarketHistoryPage {
  const expected = normalizeMarketHistoryRequest(request), source = record(value, "公开历史页");
  exact(source, ["code", "entries", "next_cursor", "settled_through"], "公开历史页");
  if (source.code !== expected.code) throw new Error("公开历史页证券与请求不同");
  const settled = source.settled_through === null ? null : tradeHistoryDate(source.settled_through, "settled_through");
  let previous = expected.after;
  let expectedTime = Date.parse(`${expected.after ?? expected.date_from}T00:00:00Z`) + (expected.after === null ? 0 : 86400000);
  const throughTime = Date.parse(`${expected.date_to}T00:00:00Z`);
  const entries = values(source.entries, "entries").map((raw) => {
    const entry = record(raw, "历史日期");
    exact(entry, ["date", "availability", "bars", "daily_candle"], "历史日期");
    const date = tradeHistoryDate(entry.date, "date");
    if (date < expected.date_from || date > expected.date_to || (previous !== null && date <= previous)) throw new Error("历史日期超范围或非严格升序");
    if (Date.parse(`${date}T00:00:00Z`) !== expectedTime) throw new Error("历史页遗漏自然日期，不能混淆休市、零成交和缺失");
    expectedTime += 86400000;
    previous = date;
    const availability = entry.availability;
    if (!["Traded", "NoTrades", "Closed", "BeforeStart", "NotEnded"].includes(String(availability))) throw new Error("未知历史可用性");
    const bars = values(entry.bars, "bars").map(normalizeMinuteBar);
    let last = -1;
    for (const bar of bars) { if (bar.minute_of_day <= last) throw new Error("分钟bar必须按分钟严格递增"); last = bar.minute_of_day; }
    if ((availability === "Traded") !== (bars.length > 0)) throw new Error("历史状态与真实成交不一致");
    if (["Traded", "NoTrades", "Closed"].includes(String(availability)) && (settled === null || date > settled)) throw new Error("尚未日结不能冒充归档日期");
    if (availability === "NotEnded" && settled !== null && date <= settled) throw new Error("已日结不能标为未结束");
    const candle = entry.daily_candle === null ? null : parseDailyCandle(entry.daily_candle, "daily_candle");
    if (candle !== null && candle.time !== Date.parse(`${date}T00:00:00Z`) / 1000) throw new Error("日K日期与分钟日期不同");
    if ((availability === "Closed" || availability === "NotEnded") && candle !== null) throw new Error("休市或未结束日期不能包含完整日K");
    return { date, availability: availability as MarketHistoryPage["entries"][number]["availability"], bars, daily_candle: candle };
  });
  if (entries.length > expected.page_size) throw new Error("历史页超过page_size");
  if (expectedTime <= throughTime && entries.length !== expected.page_size) throw new Error("历史页提前截断日期范围");
  const next = source.next_cursor === null ? null : tradeHistoryDate(source.next_cursor, "next_cursor");
  if (next !== null && (entries.length !== expected.page_size || next !== entries.at(-1)?.date || next >= expected.date_to)) throw new Error("历史next_cursor不合法");
  if ((next !== null) !== (expectedTime <= throughTime)) throw new Error("历史next_cursor不能遗漏或伪造后续日期");
  return { code: expected.code, entries, next_cursor: next, settled_through: settled };
}
