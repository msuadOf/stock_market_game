import type { AccountSnap } from "../../types/generated/AccountSnap.ts";
import type { AuctionPoint } from "../../types/generated/AuctionPoint.ts";
import type { CivilRefresh } from "../../types/generated/CivilRefresh.ts";
import type { CivilUpdate } from "../../types/generated/CivilUpdate.ts";
import type { ContinuousPoint } from "../../types/generated/ContinuousPoint.ts";
import type { EngineUpdate } from "../../types/generated/EngineUpdate.ts";
import type { EntityTag } from "../../types/generated/EntityTag.ts";
import type { Event } from "../../types/generated/Event.ts";
import type { EventFact } from "../../types/generated/EventFact.ts";
import type { EventStableKey } from "../../types/generated/EventStableKey.ts";
import type { Snapshot } from "../../types/generated/Snapshot.ts";
import type { TickFrame } from "../../types/generated/TickFrame.ts";
import type { TickTimeseriesPayload } from "../../types/generated/TickTimeseriesPayload.ts";
import { exact, enumValue, field, mapEntries, normalizeSerdeValue, nullable, record, safeInteger, safeU8, safeU32, text, values } from "./guards.ts";
import { ProtocolError } from "./types.ts";
import {
  parseDayStatus,
  parseDailyCandle,
  parseDecimalId,
  parseDepth,
  parseDisclosureKind,
  parseInstant,
  parseIsoDate,
  parseMarket,
  parseMoney,
  parseStock,
  TRADING_PHASES,
} from "./wire-values.ts";

const EVENT_NAMES = [
  "Trade", "AuctionTick", "AuctionCompleted", "PriceTick", "DayBoundary", "CivilDateAdvanced",
  "CompanyDisclosurePublished", "IntentRejected", "SettlementError", "ResourceLimit", "OrderCanceled", "OrderAccepted",
] as const;
const EVENT_SOURCES = ["Sealed", "P0", "PriceTick", "DayEnd", "Session"] as const;
const REJECTION_REASONS = [
  "InsufficientCash", "InsufficientShares", "LimitExceeded", "PriceCageExceeded", "UnknownStock",
  "AuctionLimitOrderRequired", "AuctionOrderNotCancelable", "AuctionOrderEntryClosed", "InvalidQuantity",
  "ResourceLimitExceeded", "OrderNotFound", "SameTickOrderNotCancelable", "NotOrderOwner",
] as const;
const CIVIL_KINDS = ["AfterClose", "BeforeOpen", "CivilAdvance"] as const;

function malformed(path: string, message: string): never {
  throw new ProtocolError("PROTOCOL_MALFORMED", path, message);
}

function entity(value: unknown, path: string): EntityTag {
  if (value === "Session") return "Session";
  const source = record(value, path);
  const entries = Object.entries(source);
  if (entries.length !== 1) malformed(path, "实体必须是单一外部标签");
  const [kind, payload] = entries[0] ?? malformed(path, "实体缺失");
  switch (kind) {
    case "Stock":
      return { Stock: text(payload, `${path}.Stock`) };
    case "Account":
      return { Account: safeInteger(payload, `${path}.Account`) };
    default:
      return malformed(path, `未知实体标签：${kind}`);
  }
}

function stableKey(value: unknown, path: string): EventStableKey {
  const source = record(value, path);
  exact(source, ["phase_rank", "entity", "source", "local_event_index"], path);
  return {
    phase_rank: safeU8(field(source, "phase_rank", path), `${path}.phase_rank`),
    entity: entity(field(source, "entity", path), `${path}.entity`),
    source: enumValue(field(source, "source", path), EVENT_SOURCES, `${path}.source`),
    local_event_index: safeInteger(field(source, "local_event_index", path), `${path}.local_event_index`),
  };
}

function event(value: unknown, path: string): Event {
  const source = record(value, path);
  const entries = Object.entries(source);
  if (entries.length !== 1 || !isEventName(entries[0]?.[0])) {
    malformed(path, "事件必须是一个已知外部标签变体");
  }
  const [kind, raw] = entries[0] ?? malformed(path, "事件标签缺失");
  const payload = record(raw, `${path}.${kind}`);
  const seq = safeInteger(field(payload, "seq", `${path}.${kind}`), `${path}.${kind}.seq`);
  switch (kind) {
    case "Trade":
      exact(payload, ["seq", "code", "price", "qty", "maker", "taker"], `${path}.Trade`);
      return { Trade: { seq, code: text(field(payload, "code", `${path}.Trade`), `${path}.Trade.code`), price: parseMoney(field(payload, "price", `${path}.Trade`), `${path}.Trade.price`), qty: safeU32(field(payload, "qty", `${path}.Trade`), `${path}.Trade.qty`), maker: safeInteger(field(payload, "maker", `${path}.Trade`), `${path}.Trade.maker`), taker: safeInteger(field(payload, "taker", `${path}.Trade`), `${path}.Trade.taker`) } };
    case "AuctionTick":
      exact(payload, ["seq", "tick", "phase", "code", "indicative_price", "matched_volume", "imbalance"], `${path}.AuctionTick`);
      return { AuctionTick: { seq, tick: safeInteger(field(payload, "tick", `${path}.AuctionTick`), `${path}.AuctionTick.tick`), phase: enumValue(field(payload, "phase", `${path}.AuctionTick`), TRADING_PHASES, `${path}.AuctionTick.phase`), code: text(field(payload, "code", `${path}.AuctionTick`), `${path}.AuctionTick.code`), indicative_price: nullable(field(payload, "indicative_price", `${path}.AuctionTick`), parseMoney, `${path}.AuctionTick.indicative_price`), matched_volume: safeInteger(field(payload, "matched_volume", `${path}.AuctionTick`), `${path}.AuctionTick.matched_volume`), imbalance: safeInteger(field(payload, "imbalance", `${path}.AuctionTick`), `${path}.AuctionTick.imbalance`) } };
    case "AuctionCompleted":
      exact(payload, ["seq", "tick", "phase", "code", "clearing_price", "matched_volume"], `${path}.AuctionCompleted`);
      return { AuctionCompleted: { seq, tick: safeInteger(field(payload, "tick", `${path}.AuctionCompleted`), `${path}.AuctionCompleted.tick`), phase: enumValue(field(payload, "phase", `${path}.AuctionCompleted`), TRADING_PHASES, `${path}.AuctionCompleted.phase`), code: text(field(payload, "code", `${path}.AuctionCompleted`), `${path}.AuctionCompleted.code`), clearing_price: nullable(field(payload, "clearing_price", `${path}.AuctionCompleted`), parseMoney, `${path}.AuctionCompleted.clearing_price`), matched_volume: safeInteger(field(payload, "matched_volume", `${path}.AuctionCompleted`), `${path}.AuctionCompleted.matched_volume`) } };
    case "PriceTick":
      exact(payload, ["seq", "tick", "code", "last_price", "daily_candle", "bids", "asks"], `${path}.PriceTick`);
      return { PriceTick: { seq, tick: safeInteger(field(payload, "tick", `${path}.PriceTick`), `${path}.PriceTick.tick`), code: text(field(payload, "code", `${path}.PriceTick`), `${path}.PriceTick.code`), last_price: parseMoney(field(payload, "last_price", `${path}.PriceTick`), `${path}.PriceTick.last_price`), daily_candle: parseDailyCandle(field(payload, "daily_candle", `${path}.PriceTick`), `${path}.PriceTick.daily_candle`), bids: parseDepth(field(payload, "bids", `${path}.PriceTick`), `${path}.PriceTick.bids`), asks: parseDepth(field(payload, "asks", `${path}.PriceTick`), `${path}.PriceTick.asks`) } };
    case "DayBoundary":
      exact(payload, ["seq", "day", "closed_daily_candles"], `${path}.DayBoundary`);
      return { DayBoundary: { seq, day: safeU32(field(payload, "day", `${path}.DayBoundary`), `${path}.DayBoundary.day`), closed_daily_candles: mapEntries(field(payload, "closed_daily_candles", `${path}.DayBoundary`), `${path}.DayBoundary.closed_daily_candles`, parseDailyCandle) } };
    case "CivilDateAdvanced":
      exact(payload, ["seq", "settled_date", "next_date", "next_status"], `${path}.CivilDateAdvanced`);
      return { CivilDateAdvanced: { seq, settled_date: parseIsoDate(field(payload, "settled_date", `${path}.CivilDateAdvanced`), `${path}.CivilDateAdvanced.settled_date`), next_date: parseIsoDate(field(payload, "next_date", `${path}.CivilDateAdvanced`), `${path}.CivilDateAdvanced.next_date`), next_status: parseDayStatus(field(payload, "next_status", `${path}.CivilDateAdvanced`), `${path}.CivilDateAdvanced.next_status`) } };
    case "CompanyDisclosurePublished":
      exact(payload, ["seq", "publication_id", "company", "published_at", "kind"], `${path}.CompanyDisclosurePublished`);
      return { CompanyDisclosurePublished: { seq, publication_id: safeU32(field(payload, "publication_id", `${path}.CompanyDisclosurePublished`), `${path}.CompanyDisclosurePublished.publication_id`), company: text(field(payload, "company", `${path}.CompanyDisclosurePublished`), `${path}.CompanyDisclosurePublished.company`), published_at: parseInstant(field(payload, "published_at", `${path}.CompanyDisclosurePublished`), `${path}.CompanyDisclosurePublished.published_at`), kind: parseDisclosureKind(field(payload, "kind", `${path}.CompanyDisclosurePublished`), `${path}.CompanyDisclosurePublished.kind`) } };
    case "IntentRejected":
      exact(payload, ["seq", "account", "code", "reason"], `${path}.IntentRejected`);
      return { IntentRejected: { seq, account: safeInteger(field(payload, "account", `${path}.IntentRejected`), `${path}.IntentRejected.account`), code: text(field(payload, "code", `${path}.IntentRejected`), `${path}.IntentRejected.code`), reason: enumValue(field(payload, "reason", `${path}.IntentRejected`), REJECTION_REASONS, `${path}.IntentRejected.reason`) } };
    case "SettlementError":
      exact(payload, ["seq", "account", "code", "reason"], `${path}.SettlementError`);
      return { SettlementError: { seq, account: safeInteger(field(payload, "account", `${path}.SettlementError`), `${path}.SettlementError.account`), code: text(field(payload, "code", `${path}.SettlementError`), `${path}.SettlementError.code`), reason: text(field(payload, "reason", `${path}.SettlementError`), `${path}.SettlementError.reason`) } };
    case "ResourceLimit":
      exact(payload, ["seq", "resource", "limit"], `${path}.ResourceLimit`);
      return { ResourceLimit: { seq, resource: enumValue(field(payload, "resource", `${path}.ResourceLimit`), ["PendingPlanEvents"], `${path}.ResourceLimit.resource`), limit: safeU32(field(payload, "limit", `${path}.ResourceLimit`), `${path}.ResourceLimit.limit`) } };
    case "OrderCanceled":
      exact(payload, ["seq", "account", "code", "id", "remaining_qty"], `${path}.OrderCanceled`);
      return { OrderCanceled: { seq, account: safeInteger(field(payload, "account", `${path}.OrderCanceled`), `${path}.OrderCanceled.account`), code: text(field(payload, "code", `${path}.OrderCanceled`), `${path}.OrderCanceled.code`), id: safeInteger(field(payload, "id", `${path}.OrderCanceled`), `${path}.OrderCanceled.id`), remaining_qty: safeU32(field(payload, "remaining_qty", `${path}.OrderCanceled`), `${path}.OrderCanceled.remaining_qty`) } };
    case "OrderAccepted":
      exact(payload, ["seq", "account", "code", "id", "side", "price", "remaining_qty"], `${path}.OrderAccepted`);
      return { OrderAccepted: { seq, account: safeInteger(field(payload, "account", `${path}.OrderAccepted`), `${path}.OrderAccepted.account`), code: text(field(payload, "code", `${path}.OrderAccepted`), `${path}.OrderAccepted.code`), id: safeInteger(field(payload, "id", `${path}.OrderAccepted`), `${path}.OrderAccepted.id`), side: enumValue(field(payload, "side", `${path}.OrderAccepted`), ["Buy", "Sell"], `${path}.OrderAccepted.side`), price: parseMoney(field(payload, "price", `${path}.OrderAccepted`), `${path}.OrderAccepted.price`), remaining_qty: safeU32(field(payload, "remaining_qty", `${path}.OrderAccepted`), `${path}.OrderAccepted.remaining_qty`) } };
    default:
      return malformed(path, `未处理事件变体：${kind}`);
  }
}

function isEventName(value: string | undefined): value is typeof EVENT_NAMES[number] {
  return value !== undefined && EVENT_NAMES.some((candidate) => candidate === value);
}

function fact(value: unknown, path: string): EventFact {
  const source = record(value, path);
  exact(source, ["key", "event", "canonical_payload"], path);
  return { key: stableKey(field(source, "key", path), `${path}.key`), event: event(field(source, "event", path), `${path}.event`), canonical_payload: text(field(source, "canonical_payload", path), `${path}.canonical_payload`) };
}

function continuous(value: unknown, path: string): ContinuousPoint {
  const source = record(value, path);
  exact(source, ["tick", "phase", "last_price", "cumulative_volume", "bids", "asks"], path);
  return { tick: safeInteger(field(source, "tick", path), `${path}.tick`), phase: enumValue(field(source, "phase", path), TRADING_PHASES, `${path}.phase`), last_price: parseMoney(field(source, "last_price", path), `${path}.last_price`), cumulative_volume: safeInteger(field(source, "cumulative_volume", path), `${path}.cumulative_volume`), bids: parseDepth(field(source, "bids", path), `${path}.bids`), asks: parseDepth(field(source, "asks", path), `${path}.asks`) };
}

function auction(value: unknown, path: string): AuctionPoint {
  const source = record(value, path);
  exact(source, ["key", "tick", "kind", "phase", "indicative_price", "matched_volume", "imbalance"], path);
  return { key: stableKey(field(source, "key", path), `${path}.key`), tick: safeInteger(field(source, "tick", path), `${path}.tick`), kind: enumValue(field(source, "kind", path), ["Indication", "Completion"], `${path}.kind`), phase: enumValue(field(source, "phase", path), TRADING_PHASES, `${path}.phase`), indicative_price: nullable(field(source, "indicative_price", path), parseMoney, `${path}.indicative_price`), matched_volume: safeInteger(field(source, "matched_volume", path), `${path}.matched_volume`), imbalance: nullable(field(source, "imbalance", path), safeInteger, `${path}.imbalance`) };
}

function payload(value: unknown, path: string): TickTimeseriesPayload {
  const source = record(value, path);
  exact(source, ["markets", "active_daily_candles", "closed_daily_candles", "auction_points", "continuous_points"], path);
  return {
    markets: mapEntries(field(source, "markets", path), `${path}.markets`, parseMarket),
    active_daily_candles: mapEntries(field(source, "active_daily_candles", path), `${path}.active_daily_candles`, parseDailyCandle),
    closed_daily_candles: mapEntries(field(source, "closed_daily_candles", path), `${path}.closed_daily_candles`, parseDailyCandle),
    auction_points: mapEntries(field(source, "auction_points", path), `${path}.auction_points`, (points, pointPath) => values(points, pointPath).map((point, index) => auction(point, `${pointPath}[${index}]`))),
    continuous_points: mapEntries(field(source, "continuous_points", path), `${path}.continuous_points`, continuous),
  };
}

export function parseProtocolSnapshot(value: unknown, path = "Snapshot"): Snapshot {
  const source = record(normalizeSerdeValue(value, path), path);
  exact(source, ["seq", "tick", "day", "phase", "markets", "accounts", "daily_candles", "active_daily_candles"], path);
  return {
    seq: safeInteger(field(source, "seq", path), `${path}.seq`),
    tick: safeInteger(field(source, "tick", path), `${path}.tick`),
    day: safeU32(field(source, "day", path), `${path}.day`),
    phase: enumValue(field(source, "phase", path), TRADING_PHASES, `${path}.phase`),
    markets: mapEntries(field(source, "markets", path), `${path}.markets`, parseMarket),
    accounts: mapEntries(field(source, "accounts", path), `${path}.accounts`, account),
    daily_candles: mapEntries(field(source, "daily_candles", path), `${path}.daily_candles`, (candles, candlePath) => values(candles, candlePath).map((candle, index) => parseDailyCandle(candle, `${candlePath}[${index}]`))),
    active_daily_candles: mapEntries(field(source, "active_daily_candles", path), `${path}.active_daily_candles`, parseDailyCandle),
  };
}

function account(value: unknown, path: string): AccountSnap {
  const source = record(value, path);
  exact(source, ["cash", "positions", "reserved_cash", "reserved_sell_qty"], path);
  return {
    cash: parseMoney(field(source, "cash", path), `${path}.cash`),
    positions: mapEntries(field(source, "positions", path), `${path}.positions`, (position, positionPath) => {
      const parsed = record(position, positionPath);
      exact(parsed, ["qty", "t1_locked", "invested_cents", "recovered_cents"], positionPath);
      return { qty: safeU32(field(parsed, "qty", positionPath), `${positionPath}.qty`), t1_locked: safeU32(field(parsed, "t1_locked", positionPath), `${positionPath}.t1_locked`), invested_cents: parseMoney(field(parsed, "invested_cents", positionPath), `${positionPath}.invested_cents`), recovered_cents: parseMoney(field(parsed, "recovered_cents", positionPath), `${positionPath}.recovered_cents`) };
    }),
    reserved_cash: parseMoney(field(source, "reserved_cash", path), `${path}.reserved_cash`),
    reserved_sell_qty: mapEntries(field(source, "reserved_sell_qty", path), `${path}.reserved_sell_qty`, safeU32),
  };
}

function frame(value: unknown, path: string): TickFrame {
  const source = record(value, path);
  exact(source, ["tick", "events", "facts", "timeseries_payload", "seq_from", "seq_to"], path);
  return { tick: safeInteger(field(source, "tick", path), `${path}.tick`), events: values(field(source, "events", path), `${path}.events`).map((entry, index) => event(entry, `${path}.events[${index}]`)), facts: values(field(source, "facts", path), `${path}.facts`).map((entry, index) => fact(entry, `${path}.facts[${index}]`)), timeseries_payload: payload(field(source, "timeseries_payload", path), `${path}.timeseries_payload`), seq_from: safeInteger(field(source, "seq_from", path), `${path}.seq_from`), seq_to: safeInteger(field(source, "seq_to", path), `${path}.seq_to`) };
}

function refresh(value: unknown, path: string): CivilRefresh {
  const source = record(value, path);
  exact(source, ["ticks_per_day", "snapshot", "securities", "intraday", "public_publication_ids"], path);
  return { ticks_per_day: safeInteger(field(source, "ticks_per_day", path), `${path}.ticks_per_day`, 1), snapshot: parseProtocolSnapshot(field(source, "snapshot", path), `${path}.snapshot`), securities: values(field(source, "securities", path), `${path}.securities`).map((entry, index) => parseStock(entry, `${path}.securities[${index}]`)), intraday: values(field(source, "intraday", path), `${path}.intraday`).map((entry, index) => frame(entry, `${path}.intraday[${index}]`)), public_publication_ids: values(field(source, "public_publication_ids", path), `${path}.public_publication_ids`).map((entry, index) => parseDecimalId(entry, `${path}.public_publication_ids[${index}]`)) };
}

function civil(value: unknown, path: string): CivilUpdate {
  const source = record(value, path);
  exact(source, ["boundary", "kinds", "tick", "civil_date", "events", "facts", "seq_from", "seq_to", "refresh"], path);
  const boundary = record(field(source, "boundary", path), `${path}.boundary`);
  exact(boundary, ["settled_date", "settled_phase", "next_date", "next_status"], `${path}.boundary`);
  return {
    boundary: { settled_date: parseIsoDate(field(boundary, "settled_date", `${path}.boundary`), `${path}.boundary.settled_date`), settled_phase: enumValue(field(boundary, "settled_phase", `${path}.boundary`), ["IntradayTrading", "ClosedDay"], `${path}.boundary.settled_phase`), next_date: parseIsoDate(field(boundary, "next_date", `${path}.boundary`), `${path}.boundary.next_date`), next_status: parseDayStatus(field(boundary, "next_status", `${path}.boundary`), `${path}.boundary.next_status`) },
    kinds: values(field(source, "kinds", path), `${path}.kinds`).map((entry, index) => enumValue(entry, CIVIL_KINDS, `${path}.kinds[${index}]`)),
    tick: safeInteger(field(source, "tick", path), `${path}.tick`),
    civil_date: parseIsoDate(field(source, "civil_date", path), `${path}.civil_date`),
    events: values(field(source, "events", path), `${path}.events`).map((entry, index) => event(entry, `${path}.events[${index}]`)),
    facts: values(field(source, "facts", path), `${path}.facts`).map((entry, index) => fact(entry, `${path}.facts[${index}]`)),
    seq_from: safeInteger(field(source, "seq_from", path), `${path}.seq_from`),
    seq_to: safeInteger(field(source, "seq_to", path), `${path}.seq_to`),
    refresh: refresh(field(source, "refresh", path), `${path}.refresh`),
  };
}

export function parseEngineUpdate(value: unknown): EngineUpdate {
  const source = record(normalizeSerdeValue(value, "EngineUpdate"), "EngineUpdate");
  const entries = Object.entries(source);
  if (entries.length !== 1) malformed("EngineUpdate", "EngineUpdate 必须恰有一个外部标签");
  const [kind, payloadValue] = entries[0] ?? malformed("EngineUpdate", "EngineUpdate 标签缺失");
  switch (kind) {
    case "TickBatch": {
      const payload = record(payloadValue, "EngineUpdate.TickBatch");
      exact(payload, ["frames", "runtime_snapshot"], "EngineUpdate.TickBatch");
      return { TickBatch: { frames: values(field(payload, "frames", "EngineUpdate.TickBatch"), "EngineUpdate.TickBatch.frames").map((entry, index) => frame(entry, `EngineUpdate.TickBatch.frames[${index}]`)), runtime_snapshot: nullable(field(payload, "runtime_snapshot", "EngineUpdate.TickBatch"), parseProtocolSnapshot, "EngineUpdate.TickBatch.runtime_snapshot") } };
    }
    case "CivilUpdate":
      return { CivilUpdate: civil(payloadValue, "EngineUpdate.CivilUpdate") };
    default:
      return malformed("EngineUpdate", `未知 EngineUpdate 变体：${kind}`);
  }
}
