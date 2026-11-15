/** 历史日 K 查询：跨宿主使用同一 wire 校验和 code ↔ StockId 翻译。
 *  分钟数据仅来自单独的当前分钟接口；历史响应的 bars 兼容字段必须为空。
 */
import type { CivilDate } from "../types/generated/CivilDate.ts";
import type { DailyCandle } from "../types/generated/DailyCandle.ts";
import type { MarketHistoryRequest as WireMarketHistoryRequest } from "../types/generated/MarketHistoryRequest.ts";
import type { MarketHistoryAvailability } from "../types/generated/MarketHistoryAvailability.ts";
import { exact, field, record, values } from "./protocol/guards.ts";
import { parseDailyCandle } from "./protocol/wire-values.ts";
import type { CatalogResolver } from "./catalog-resolver.ts";
import { tradeHistoryDate } from "./personal-trade-history.ts";
import { assertSettledDailyCandle } from "../utils/archive-daily-candle.ts";
import { parseMoney } from "../utils/money.ts";
import { parseTurnoverCents } from "../utils/turnover.ts";

/** 上层使用证券六位代码，宿主边界翻译为 StockId。 */
export interface MarketHistoryRequest {
  readonly code: string;
  readonly date_from: CivilDate;
  readonly date_to: CivilDate;
  readonly after: CivilDate | null;
  readonly page_size: number;
}

/** 视图分钟 bar（wire `MinuteBarFact` 同形——字符串承载分/股数/笔数/成交额）。 */
export interface MinuteBar {
  readonly minute_of_day: number;
  readonly phase: "OpenAuction" | "Continuous" | "ClosingAuction";
  readonly open: string;
  readonly high: string;
  readonly low: string;
  readonly close: string;
  readonly volume_shares: string;
  readonly turnover_cents: string;
  readonly trade_count: string;
}

/** 视图一日条目。 */
export interface MarketHistoryEntry {
  readonly date: CivilDate;
  readonly availability: MarketHistoryAvailability;
  readonly bars: readonly MinuteBar[];
  readonly daily_candle: DailyCandle | null;
}

/** 视图分页结果。 */
export interface MarketHistoryPage {
  readonly code: string;
  readonly entries: readonly MarketHistoryEntry[];
  readonly next_cursor: CivilDate | null;
  readonly settled_through: CivilDate | null;
}

/** 请求层翻译：视图 code 串 → wire 数值 StockId（wasm 请求侧，ADR-0041）。 */
export function wireMarketHistoryRequest(request: MarketHistoryRequest, identity: CatalogResolver): WireMarketHistoryRequest {
  return {
    stock: identity.stockId(request.code),
    date_from: request.date_from,
    date_to: request.date_to,
    after: request.after,
    page_size: request.page_size,
  };
}

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

export function normalizeMarketHistoryPage(value: unknown, request: MarketHistoryRequest, identity: CatalogResolver): MarketHistoryPage {
  const expected = normalizeMarketHistoryRequest(request), source = record(value, "公开历史页");
  // wire 的 stock 为数值 StockId（ADR 0041）；摄入时译回代码串再与请求比对。
  exact(source, ["stock", "entries", "next_cursor", "settled_through"], "公开历史页");
  const wireStock = field(source, "stock", "公开历史页");
  if (typeof wireStock !== "number" || !Number.isInteger(wireStock)) throw new Error("公开历史页证券必须为数值StockId");
  const code = identity.stockCode(wireStock, "公开历史页.stock");
  if (code !== expected.code) throw new Error("公开历史页证券与请求不同");
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
    if (values(entry.bars, "bars").length !== 0) throw new Error("历史日K不得携带已结束日的分钟数据");
    const isSettledSession = availability === "Traded" || availability === "NoTrades";
    if ((isSettledSession || availability === "Closed") && (settled === null || date > settled)) throw new Error("尚未日结不能冒充归档日期");
    if (availability === "NotEnded" && settled !== null && date <= settled) throw new Error("已日结不能标为未结束");
    const candle = entry.daily_candle === null ? null : parseDailyCandle(entry.daily_candle, "daily_candle");
    if (candle !== null && candle.time !== Date.parse(`${date}T00:00:00Z`) / 1000) throw new Error("日K与历史条目日期不同");
    if (isSettledSession) {
      if (candle === null) throw new Error("开市归档日期缺少日K");
      assertSettledDailyCandle(candle, "daily_candle");
      if ((availability === "Traded") !== (candle.volume > 0)) throw new Error("历史状态与日K成交量不一致");
    } else if ((availability === "Closed" || availability === "NotEnded") && candle !== null) {
      throw new Error("休市或未结束日期不能包含完整日K");
    } else if (availability === "BeforeStart" && candle?.trade_stats != null) {
      throw new Error("虚拟前史不能包含本局真实成交统计");
    }
    return { date, availability: availability as MarketHistoryEntry["availability"], bars: [], daily_candle: candle };
  });
  if (entries.length > expected.page_size) throw new Error("历史页超过page_size");
  if (expectedTime <= throughTime && entries.length !== expected.page_size) throw new Error("历史页提前截断日期范围");
  const next = source.next_cursor === null ? null : tradeHistoryDate(source.next_cursor, "next_cursor");
  if (next !== null && (entries.length !== expected.page_size || next !== entries.at(-1)?.date || next >= expected.date_to)) throw new Error("历史next_cursor不合法");
  if ((next !== null) !== (expectedTime <= throughTime)) throw new Error("历史next_cursor不能遗漏或伪造后续日期");
  return { code: expected.code, entries, next_cursor: next, settled_through: settled };
}
