import type { ClosedReason } from "../../types/generated/ClosedReason.ts";
import type { CompanyDisclosureKind } from "../../types/generated/CompanyDisclosureKind.ts";
import type { DailyCandle } from "../../types/generated/DailyCandle.ts";
import type { DayStatus } from "../../types/generated/DayStatus.ts";
import type { MarketSnap } from "../../types/generated/MarketSnap.ts";
import type { StockSpec } from "../../types/generated/StockSpec.ts";
import {
  enumValue,
  exact,
  field,
  finiteNumber,
  nullable,
  record,
  safeInteger,
  safeU32,
  signedSafeInteger,
  text,
  values,
} from "./guards.ts";
import { ProtocolError } from "./types.ts";

export const TRADING_PHASES = ["CallAuction", "PreOpen", "ClosingAuction", "Continuous"] as const;
export const HOLIDAY_KINDS = [
  "NewYearDay", "LabourDay", "NationalDay", "SpringFestival", "Qingming", "DragonBoat", "MidAutumn",
] as const;
export const STOCK_EXCHANGES = ["Shanghai", "Shenzhen"] as const;
export const SECURITY_CATEGORIES = ["MainBoard", "StMainBoard", "ChiNext"] as const;
const U64_MAX = 18_446_744_073_709_551_615n;

function malformed(path: string, message: string): never {
  throw new ProtocolError("PROTOCOL_MALFORMED", path, message);
}

export function parseIsoDate(value: unknown, path: string): string {
  const parsed = text(value, path);
  const matched = /^(\d{4})-(\d{2})-(\d{2})$/.exec(parsed);
  if (!matched) malformed(path, `${path} 必须是 ISO 日期`);
  const year = Number(matched[1]);
  const month = Number(matched[2]);
  const day = Number(matched[3]);
  const lastDay = new Date(Date.UTC(year, month, 0)).getUTCDate();
  if (year < 1900 || year > 2199 || month < 1 || month > 12 || day < 1 || day > lastDay) {
    malformed(path, `${path} 必须是有效 ISO 日期`);
  }
  return parsed;
}

export function parseMoney(value: unknown, path: string): number {
  return signedSafeInteger(value, path);
}

export function parseDepth(value: unknown, path: string): Array<[number, number]> {
  return values(value, path).map((entry, index) => {
    const pair = values(entry, `${path}[${index}]`);
    if (pair.length !== 2) malformed(`${path}[${index}]`, "盘口档位必须含价格与数量");
    return [parseMoney(pair[0], `${path}[${index}][0]`), safeInteger(pair[1], `${path}[${index}][1]`)];
  });
}

export function parseDailyCandle(value: unknown, path: string): DailyCandle {
  const source = record(value, path);
  const hasTradeStats = Object.hasOwn(source, "trade_stats");
  exact(source, hasTradeStats
    ? ["time", "open", "high", "low", "close", "volume", "trade_stats"]
    : ["time", "open", "high", "low", "close", "volume"], path);
  const candle = {
    time: signedSafeInteger(field(source, "time", path), `${path}.time`),
    open: parseMoney(field(source, "open", path), `${path}.open`),
    high: parseMoney(field(source, "high", path), `${path}.high`),
    low: parseMoney(field(source, "low", path), `${path}.low`),
    close: parseMoney(field(source, "close", path), `${path}.close`),
    volume: safeInteger(field(source, "volume", path), `${path}.volume`),
  };
  if (!hasTradeStats) return candle;
  const rawStats = field(source, "trade_stats", path);
  if (rawStats === null) return { ...candle, trade_stats: null };
  const stats = record(rawStats, `${path}.trade_stats`);
  exact(stats, ["turnover_cents", "trade_count"], `${path}.trade_stats`);
  const turnoverCents = text(field(stats, "turnover_cents", `${path}.trade_stats`), `${path}.trade_stats.turnover_cents`);
  if (!isU64Decimal(turnoverCents)) malformed(`${path}.trade_stats.turnover_cents`, "成交额必须是 u64 范围内的无损非负十进制字符串");
  return {
    ...candle,
    trade_stats: {
      turnover_cents: turnoverCents,
      trade_count: safeInteger(field(stats, "trade_count", `${path}.trade_stats`), `${path}.trade_stats.trade_count`),
    },
  };
}

export function parseMarket(value: unknown, path: string): MarketSnap {
  const source = record(value, path);
  exact(source, ["last_price", "last_close", "best_bid", "best_ask", "bids", "asks"], path);
  return {
    last_price: parseMoney(field(source, "last_price", path), `${path}.last_price`),
    last_close: parseMoney(field(source, "last_close", path), `${path}.last_close`),
    best_bid: nullable(field(source, "best_bid", path), parseMoney, `${path}.best_bid`),
    best_ask: nullable(field(source, "best_ask", path), parseMoney, `${path}.best_ask`),
    bids: parseDepth(field(source, "bids", path), `${path}.bids`),
    asks: parseDepth(field(source, "asks", path), `${path}.asks`),
  };
}

export function parseDayStatus(value: unknown, path: string): DayStatus {
  if (value === "Trading") return "Trading";
  const source = record(value, path);
  exact(source, ["Closed"], path);
  return { Closed: parseClosedReason(field(source, "Closed", path), `${path}.Closed`) };
}

function parseClosedReason(value: unknown, path: string): ClosedReason {
  if (value === "Weekend") return "Weekend";
  const source = record(value, path);
  const entries = Object.entries(source);
  if (entries.length !== 1) malformed(path, "休市原因必须是单一外部标签");
  const [kind, payload] = entries[0] ?? malformed(path, "休市原因缺失");
  switch (kind) {
    case "OfficialHoliday": {
      const holiday = record(payload, `${path}.OfficialHoliday`);
      exact(holiday, ["citation_id"], `${path}.OfficialHoliday`);
      return { OfficialHoliday: { citation_id: text(field(holiday, "citation_id", `${path}.OfficialHoliday`), `${path}.OfficialHoliday.citation_id`) } };
    }
    case "SimulatedHoliday":
      return { SimulatedHoliday: enumValue(payload, HOLIDAY_KINDS, `${path}.SimulatedHoliday`) };
    default:
      return malformed(path, `未知休市原因：${kind}`);
  }
}

export function parseInstant(value: unknown, path: string): { readonly date: string; readonly second_of_day: number } {
  const source = record(value, path);
  exact(source, ["date", "second_of_day"], path);
  const secondOfDay = safeInteger(field(source, "second_of_day", path), `${path}.second_of_day`);
  if (secondOfDay >= 86_400) malformed(`${path}.second_of_day`, "当日秒必须小于 86400");
  return { date: parseIsoDate(field(source, "date", path), `${path}.date`), second_of_day: secondOfDay };
}

export function parseDisclosureKind(value: unknown, path: string): CompanyDisclosureKind {
  if (value === "Announcement") return "Announcement";
  const source = record(value, path);
  exact(source, ["Report"], path);
  const report = record(field(source, "Report", path), `${path}.Report`);
  exact(report, ["report_revision"], `${path}.Report`);
  return { Report: { report_revision: safeU32(field(report, "report_revision", `${path}.Report`), `${path}.Report.report_revision`) } };
}

export function parseStock(value: unknown, path: string): StockSpec {
  const source = record(value, path);
  exact(source, ["code", "exchange", "initial_price", "category", "limit_pct", "tick", "total_shares", "float_shares"], path);
  const totalShares = text(field(source, "total_shares", path), `${path}.total_shares`);
  if (!isU64Decimal(totalShares)) malformed(`${path}.total_shares`, "总股本必须是 u64 范围内的无损非负十进制字符串");
  return {
    code: text(field(source, "code", path), `${path}.code`),
    exchange: enumValue(field(source, "exchange", path), STOCK_EXCHANGES, `${path}.exchange`),
    initial_price: parseMoney(field(source, "initial_price", path), `${path}.initial_price`),
    category: enumValue(field(source, "category", path), SECURITY_CATEGORIES, `${path}.category`),
    limit_pct: finiteNumber(field(source, "limit_pct", path), `${path}.limit_pct`),
    tick: parseMoney(field(source, "tick", path), `${path}.tick`),
    total_shares: totalShares,
    float_shares: safeU32(field(source, "float_shares", path), `${path}.float_shares`),
  };
}

export function parseDecimalId(value: unknown, path: string): string {
  const parsed = text(value, path);
  if (!isU64Decimal(parsed)) malformed(path, `${path} 必须是 u64 范围内的无损非负十进制字符串`);
  return parsed;
}

function isU64Decimal(value: string): boolean {
  return /^\d+$/.test(value) && BigInt(value) <= U64_MAX;
}
