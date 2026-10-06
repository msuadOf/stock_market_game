/**
 * 股票身份摄入翻译层（ADR 0041）。
 *
 * 引擎 wire 自数值化后以 `StockId`（u32）标识股票：map 键为数值的 JSON 字符串
 * 形态（"0"、"1"…），事件/历史响应的 `code` 字段为数值。本模块在摄入层把
 * 「已通过原始校验」的 wire 结构一次性翻译回 A 股代码串形态（`Coded*` 类型），
 * 既有 store、视图、排序与过滤逻辑继续面向代码串，不需要感知 StockId。
 *
 * 翻译顺序契约：事件/事实必须先以原始数值形态通过 `validate.ts` 的
 * `canonical_payload` 与稳定键比对（引擎签名字符串按数值生成），随后才能翻译；
 * 快照与 runtime delta 不含引擎签名串，可边解析边翻译。反向（请求层）翻译见
 * `wireIntentOf` 与 `config/stock-ids.ts` 的 `stockIdOf`。
 */
import { codeOfStock, stockIdOf } from "../../config/stock-ids.ts";
import { canonicalJson } from "./canonical.ts";
import type { AccountId } from "../../types/generated/AccountId.ts";
import type { AuctionPoint } from "../../types/generated/AuctionPoint.ts";
import type { AuctionPointKind } from "../../types/generated/AuctionPointKind.ts";
import type { CivilBoundary } from "../../types/generated/CivilBoundary.ts";
import type { CivilUpdateKind } from "../../types/generated/CivilUpdateKind.ts";
import type { CompanyDisclosureKind } from "../../types/generated/CompanyDisclosureKind.ts";
import type { ContinuousPoint } from "../../types/generated/ContinuousPoint.ts";
import type { DailyCandle } from "../../types/generated/DailyCandle.ts";
import type { DayStatus } from "../../types/generated/DayStatus.ts";
import type { EntityTag } from "../../types/generated/EntityTag.ts";
import type { Event } from "../../types/generated/Event.ts";
import type { EventSourceIndex } from "../../types/generated/EventSourceIndex.ts";
import type { EventStableKey } from "../../types/generated/EventStableKey.ts";
import type { Intent } from "../../types/generated/Intent.ts";
import type { LimitPrice } from "../../types/generated/LimitPrice.ts";
import type { MarketSnap } from "../../types/generated/MarketSnap.ts";
import type { Money } from "../../types/generated/Money.ts";
import type { PositionSnap } from "../../types/generated/PositionSnap.ts";
import type { RejectionReason } from "../../types/generated/RejectionReason.ts";
import type { Side } from "../../types/generated/Side.ts";
import type { StockSpec } from "../../types/generated/StockSpec.ts";
import type { TradingPhase } from "../../types/generated/TradingPhase.ts";
import type { PlayerWorkingOrder } from "../player-working-orders.ts";
import { ProtocolError } from "./types.ts";

// ── 原始 wire 形态类型（Wire*：数值 StockId，map 键为 JSON 字符串形态）──
//
// ts-rs 把 `BTreeMap<StockId, _>` 生成为数值索引签名 `{ [key in StockId]: V }`，
// 但 JSON/JS 对象键在运行期恒为字符串（"0"、"1"…）。原始解析层按运行期真实形态
// 使用 `Record<string, V>`；除 map 键与 `code` 数值外，字段与生成类型逐一对齐。

/** wire 事件：仅 DayBoundary 的 closed_daily_candles 键改为字符串形态。 */
export type WireEvent = Exclude<Event, { DayBoundary: unknown }> | { readonly DayBoundary: { readonly seq: number; readonly day: number; readonly closed_daily_candles: Readonly<Record<string, DailyCandle>> } };

/** wire 事实。 */
export type WireEventFact = {
  readonly key: EventStableKey;
  readonly event: WireEvent;
  readonly canonical_payload: string;
};

/** wire 账户快照。 */
export type WireAccountSnap = {
  cash: Money;
  positions: Record<string, PositionSnap>;
  reserved_cash: Money;
  reserved_sell_qty: Record<string, number>;
};

/** wire 快照。 */
export type WireSnapshot = {
  readonly seq: number;
  readonly tick: number;
  readonly day: number;
  readonly phase: TradingPhase;
  readonly markets: Readonly<Record<string, MarketSnap>>;
  readonly accounts: Readonly<Record<string, WireAccountSnap>>;
  readonly daily_candles: Readonly<Record<string, readonly DailyCandle[]>>;
  readonly active_daily_candles: Readonly<Record<string, DailyCandle>>;
};

/** wire 玩家活动委托（公共 DTO 字段，code 为数值 StockId）。 */
export type WirePlayerWorkingOrder = Omit<PlayerWorkingOrder, "code"> & { readonly code: number };

/** wire runtime 增量。 */
export type WireRuntimeDelta = {
  readonly seq_from: number;
  readonly seq_to: number;
  readonly tick: number;
  readonly day: number;
  readonly phase: TradingPhase;
  readonly accounts: Readonly<Record<string, WireAccountSnap>>;
  readonly working_orders: { readonly reset: boolean; readonly upserts: readonly WirePlayerWorkingOrder[]; readonly removed: readonly number[] };
};

/** wire 帧时序载荷。 */
export type WireTickTimeseriesPayload = {
  readonly markets: Readonly<Record<string, MarketSnap>>;
  readonly active_daily_candles: Readonly<Record<string, DailyCandle>>;
  readonly closed_daily_candles: Readonly<Record<string, DailyCandle>>;
  readonly auction_points: Readonly<Record<string, readonly AuctionPoint[]>>;
  readonly continuous_points: Readonly<Record<string, ContinuousPoint>>;
};

/** wire tick 帧。 */
export type WireTickFrame = {
  readonly tick: number;
  readonly events: readonly WireEvent[];
  readonly facts: readonly WireEventFact[];
  readonly timeseries_payload: WireTickTimeseriesPayload;
  readonly seq_from: number;
  readonly seq_to: number;
};

/** wire 收盘刷新。 */
export type WireCivilRefresh = {
  readonly ticks_per_day: number;
  readonly snapshot: WireSnapshot;
  readonly securities: readonly StockSpec[];
  readonly intraday: readonly WireTickFrame[];
  readonly public_publication_ids: readonly string[];
};

/** wire 自然日更新。 */
export type WireCivilUpdate = {
  readonly boundary: CivilBoundary;
  readonly kinds: readonly CivilUpdateKind[];
  readonly tick: number;
  readonly civil_date: string;
  readonly events: readonly WireEvent[];
  readonly facts: readonly WireEventFact[];
  readonly seq_from: number;
  readonly seq_to: number;
  readonly refresh: WireCivilRefresh;
};

/** wire 引擎更新：validate/canonical 阶段的输入形态。 */
export type WireEngineUpdate =
  | { readonly TickBatch: { readonly frames: readonly WireTickFrame[]; readonly runtime_snapshot: WireSnapshot | null; readonly runtime_delta?: WireRuntimeDelta } }
  | { readonly CivilUpdate: WireCivilUpdate };

// ── 代码串形态类型（Coded*：股票身份一律为 A 股代码串）──

/** 翻译后的实体标签：`Stock` 变体携带代码串。 */
export type CodedEntityTag = "Session" | { readonly Stock: string } | { readonly Account: AccountId };

/** 翻译后的事件稳定键。 */
export type CodedEventStableKey = {
  readonly phase_rank: number;
  readonly entity: CodedEntityTag;
  readonly source: EventSourceIndex;
  readonly local_event_index: number;
};

/** 翻译后的事件：携带股票身份的变体 `code` 均为代码串。 */
export type CodedEvent =
  | { readonly Trade: { readonly seq: number; readonly code: string; readonly price: Money; readonly qty: number; readonly maker: AccountId; readonly taker: AccountId } }
  | { readonly AuctionTick: { readonly seq: number; readonly tick: number; readonly phase: TradingPhase; readonly code: string; readonly indicative_price: Money | null; readonly matched_volume: number; readonly imbalance: number } }
  | { readonly AuctionCompleted: { readonly seq: number; readonly tick: number; readonly phase: TradingPhase; readonly code: string; readonly clearing_price: Money | null; readonly matched_volume: number } }
  | { readonly PriceTick: { readonly seq: number; readonly tick: number; readonly code: string; readonly last_price: Money; readonly daily_candle: DailyCandle; readonly bids: Array<[Money, number]>; readonly asks: Array<[Money, number]> } }
  | { readonly DayBoundary: { readonly seq: number; readonly day: number; readonly closed_daily_candles: Readonly<Record<string, DailyCandle>> } }
  | { readonly CivilDateAdvanced: { readonly seq: number; readonly settled_date: string; readonly next_date: string; readonly next_status: DayStatus } }
  | { readonly CompanyDisclosurePublished: { readonly seq: number; readonly publication_id: number; readonly company: string; readonly published_at: { readonly date: string; readonly second_of_day: number }; readonly kind: CompanyDisclosureKind } }
  | { readonly IntentRejected: { readonly seq: number; readonly account: AccountId; readonly code: string; readonly reason: RejectionReason } }
  | { readonly SettlementError: { readonly seq: number; readonly account: AccountId; readonly code: string; readonly reason: string } }
  | { readonly OrderCanceled: { readonly seq: number; readonly account: AccountId; readonly code: string; readonly id: number; readonly remaining_qty: number } }
  | { readonly OrderAccepted: { readonly seq: number; readonly account: AccountId; readonly code: string; readonly id: number; readonly side: Side; readonly price: Money; readonly remaining_qty: number } };

/** 翻译后的事实：`canonical_payload` 保持引擎签名的原始字符串，不重写。 */
export type CodedEventFact = {
  readonly key: CodedEventStableKey;
  readonly event: CodedEvent;
  readonly canonical_payload: string;
};

/** 翻译后的账户快照：持仓/卖出占用按代码串建键。 */
export type CodedAccountSnap = {
  cash: Money;
  positions: Readonly<Record<string, PositionSnap>>;
  reserved_cash: Money;
  reserved_sell_qty: Readonly<Record<string, number>>;
};

/** 翻译后的权威快照：markets/daily_candles/active_daily_candles 按代码串建键。
 * 字段保持与生成类型一致的可变性（store 的 Immer draft 会原位更新 tick/seq/markets）；
 * daily_candles 的元素数组进一步放宽为可变（与 WireAccountSnap 放宽先例一致），
 * 否则只读数组无法整体赋值进 Immer 的 WritableNonArrayDraft。 */
export type CodedSnapshot = {
  seq: number;
  tick: number;
  day: number;
  phase: TradingPhase;
  markets: Record<string, MarketSnap>;
  accounts: Record<string, CodedAccountSnap>;
  daily_candles: Record<string, DailyCandle[]>;
  active_daily_candles: Record<string, DailyCandle>;
};

/** 翻译后的玩家活动委托增量（公共 DTO 形态，code 为代码串）。 */
export type CodedPlayerOrderDelta = {
  readonly reset: boolean;
  readonly upserts: readonly PlayerWorkingOrder[];
  readonly removed: readonly number[];
};

/** 翻译后的 runtime 增量。 */
export type CodedRuntimeDelta = {
  readonly seq_from: number;
  readonly seq_to: number;
  readonly tick: number;
  readonly day: number;
  readonly phase: TradingPhase;
  readonly accounts: Readonly<Record<string, CodedAccountSnap>>;
  readonly working_orders: CodedPlayerOrderDelta;
};

/** 翻译后的竞价时序点（稳定键实体已译为代码串）。 */
export type CodedAuctionPoint = {
  readonly key: CodedEventStableKey;
  readonly tick: number;
  readonly kind: AuctionPointKind;
  readonly phase: TradingPhase;
  readonly indicative_price: Money | null;
  readonly matched_volume: number;
  readonly imbalance: number | null;
};

/** 翻译后的帧时序载荷：全部按代码串建键。 */
export type CodedTickTimeseriesPayload = {
  readonly markets: Readonly<Record<string, MarketSnap>>;
  readonly active_daily_candles: Readonly<Record<string, DailyCandle>>;
  readonly closed_daily_candles: Readonly<Record<string, DailyCandle>>;
  readonly auction_points: Readonly<Record<string, readonly CodedAuctionPoint[]>>;
  readonly continuous_points: Readonly<Record<string, ContinuousPoint>>;
};

/** 翻译后的 tick 帧。 */
export type CodedTickFrame = {
  readonly tick: number;
  readonly events: readonly CodedEvent[];
  readonly facts: readonly CodedEventFact[];
  readonly timeseries_payload: CodedTickTimeseriesPayload;
  readonly seq_from: number;
  readonly seq_to: number;
};

/** 翻译后的收盘刷新。 */
export type CodedCivilRefresh = {
  readonly ticks_per_day: number;
  readonly snapshot: CodedSnapshot;
  readonly securities: readonly StockSpec[];
  readonly intraday: readonly CodedTickFrame[];
  readonly public_publication_ids: readonly string[];
};

/** 翻译后的自然日更新。 */
export type CodedCivilUpdate = {
  readonly boundary: CivilBoundary;
  readonly kinds: readonly CivilUpdateKind[];
  readonly tick: number;
  readonly civil_date: string;
  readonly events: readonly CodedEvent[];
  readonly facts: readonly CodedEventFact[];
  readonly seq_from: number;
  readonly seq_to: number;
  readonly refresh: CodedCivilRefresh;
};

/** 翻译后的引擎更新（normalize/reduce/effects/store 消费的形态）。 */
export type CodedEngineUpdate =
  | { readonly TickBatch: { readonly frames: readonly CodedTickFrame[]; readonly runtime_snapshot: CodedSnapshot | null; readonly runtime_delta?: CodedRuntimeDelta } }
  | { readonly CivilUpdate: CodedCivilUpdate };

/** 请求层公共 Intent 形态：`code` 为代码串，宿主实现负责经 `wireIntentOf` 译回数值。 */
export type CodedIntent =
  | { readonly PlaceLimit: { readonly code: string; readonly side: Side; readonly price: LimitPrice; readonly qty: number } }
  | { readonly PlaceMarket: { readonly code: string; readonly side: Side; readonly qty: number } }
  | { readonly Cancel: { readonly code: string; readonly id: number } };

/** 成交事件效果载荷（与 types/engine.ts 的 TradeEvent 语义一致，code 为代码串）。 */
export type CodedTradeEvent = Extract<CodedEvent, { Trade: unknown }>["Trade"] & { readonly tick?: number };

// ── 翻译函数（对未知/非法身份一律显式抛 ProtocolError）──

function identityError(path: string, message: string): never {
  throw new ProtocolError("PROTOCOL_MALFORMED", path, message);
}

/** 数值 StockId → 代码串；未知 id 带路径抛错。 */
export function stockCodeOf(id: number, path: string): string {
  try {
    return codeOfStock(id);
  } catch (error) {
    return identityError(path, error instanceof Error ? `${path}：${error.message}` : String(error));
  }
}

function codedStockKeys<T>(source: Readonly<Record<string, T>>, path: string): Record<string, T>;
function codedStockKeys<T, U>(source: Readonly<Record<string, T>>, path: string, transform: (value: T, valuePath: string) => U): Record<string, U>;
function codedStockKeys<T, U>(source: Readonly<Record<string, T>>, path: string, transform?: (value: T, valuePath: string) => U): Record<string, U> {
  const translated: Record<string, U> = Object.create(null);
  for (const [key, value] of Object.entries(source)) {
    if (!/^(0|[1-9]\d*)$/.test(key)) identityError(path, `${path} 键 ${key} 不是数值 StockId 的字符串形态`);
    const code = stockCodeOf(Number(key), path);
    // 无 transform 的重载保证 U === T，此处即恒等填充；实现签名无法表达该约束，故经 unknown 断言。
    translated[code] = transform === undefined ? value as unknown as U : transform(value, `${path}.${code}`);
  }
  return translated;
}

function codedEntity(entity: EntityTag, path: string): CodedEntityTag {
  if (entity === "Session") return "Session";
  if ("Stock" in entity) return { Stock: stockCodeOf(entity.Stock, `${path}.Stock`) };
  return { Account: entity.Account };
}

function codedStableKey(key: EventStableKey, path: string): CodedEventStableKey {
  return { phase_rank: key.phase_rank, entity: codedEntity(key.entity, `${path}.entity`), source: key.source, local_event_index: key.local_event_index };
}

function codedEvent(event: WireEvent, path: string): CodedEvent {
  if ("Trade" in event) return { Trade: { ...event.Trade, code: stockCodeOf(event.Trade.code, `${path}.Trade.code`) } };
  if ("AuctionTick" in event) return { AuctionTick: { ...event.AuctionTick, code: stockCodeOf(event.AuctionTick.code, `${path}.AuctionTick.code`) } };
  if ("AuctionCompleted" in event) return { AuctionCompleted: { ...event.AuctionCompleted, code: stockCodeOf(event.AuctionCompleted.code, `${path}.AuctionCompleted.code`) } };
  if ("PriceTick" in event) return { PriceTick: { ...event.PriceTick, code: stockCodeOf(event.PriceTick.code, `${path}.PriceTick.code`) } };
  if ("DayBoundary" in event) {
    return { DayBoundary: { ...event.DayBoundary, closed_daily_candles: codedStockKeys(event.DayBoundary.closed_daily_candles, `${path}.DayBoundary.closed_daily_candles`) } };
  }
  if ("CivilDateAdvanced" in event) return { CivilDateAdvanced: { ...event.CivilDateAdvanced } };
  if ("CompanyDisclosurePublished" in event) return { CompanyDisclosurePublished: { ...event.CompanyDisclosurePublished } };
  if ("IntentRejected" in event) return { IntentRejected: { ...event.IntentRejected, code: stockCodeOf(event.IntentRejected.code, `${path}.IntentRejected.code`) } };
  if ("SettlementError" in event) return { SettlementError: { ...event.SettlementError, code: stockCodeOf(event.SettlementError.code, `${path}.SettlementError.code`) } };
  if ("OrderCanceled" in event) return { OrderCanceled: { ...event.OrderCanceled, code: stockCodeOf(event.OrderCanceled.code, `${path}.OrderCanceled.code`) } };
  if ("OrderAccepted" in event) return { OrderAccepted: { ...event.OrderAccepted, code: stockCodeOf(event.OrderAccepted.code, `${path}.OrderAccepted.code`) } };
  return identityError(path, `${path} 包含未处理事件变体：${String(event)}`);
}

function codedFact(fact: WireEventFact, path: string): CodedEventFact {
  return {
    key: codedStableKey(fact.key, `${path}.key`),
    event: codedEvent(fact.event, `${path}.event`),
    // canonical_payload 是引擎按数值身份生成的签名字符串，原样保留，不随翻译重写。
    canonical_payload: fact.canonical_payload,
  };
}

/** 翻译后事实的稳定身份：与 canonical.factIdentity 相同口径（稳定键 canonical JSON）。
 * CodedEventStableKey 的实体已译为代码串，canonicalJson 按值序列化，Coded 形态内部排序/去重自洽。 */
export function codedFactIdentity(fact: CodedEventFact): string {
  return canonicalJson(fact.key);
}

function codedAccount(account: WireAccountSnap, path: string): CodedAccountSnap {
  return {
    cash: account.cash,
    positions: codedStockKeys(account.positions, `${path}.positions`),
    reserved_cash: account.reserved_cash,
    reserved_sell_qty: codedStockKeys(account.reserved_sell_qty, `${path}.reserved_sell_qty`),
  };
}

/** 快照摄入翻译：markets/daily_candles/active_daily_candles/positions 等键全部译为代码串。 */
export function translateSnapshot(snapshot: WireSnapshot, path: string): CodedSnapshot {
  const accounts: Record<string, CodedAccountSnap> = Object.create(null);
  for (const [accountId, account] of Object.entries(snapshot.accounts)) {
    accounts[accountId] = codedAccount(account, `${path}.accounts.${accountId}`);
  }
  return {
    seq: snapshot.seq,
    tick: snapshot.tick,
    day: snapshot.day,
    phase: snapshot.phase,
    markets: codedStockKeys(snapshot.markets, `${path}.markets`),
    accounts,
    // 元素数组复制为可变：CodedSnapshot.daily_candles 已为 Immer draft 放宽为可变数组。
    daily_candles: codedStockKeys(snapshot.daily_candles, `${path}.daily_candles`, (items) => [...items]),
    active_daily_candles: codedStockKeys(snapshot.active_daily_candles, `${path}.active_daily_candles`),
  };
}

function codedFrame(frame: WireTickFrame, path: string): CodedTickFrame {
  const payload = frame.timeseries_payload;
  return {
    tick: frame.tick,
    events: frame.events.map((event, index) => codedEvent(event, `${path}.events[${index}]`)),
    facts: frame.facts.map((fact, index) => codedFact(fact, `${path}.facts[${index}]`)),
    timeseries_payload: {
      markets: codedStockKeys(payload.markets, `${path}.timeseries_payload.markets`),
      active_daily_candles: codedStockKeys(payload.active_daily_candles, `${path}.timeseries_payload.active_daily_candles`),
      closed_daily_candles: codedStockKeys(payload.closed_daily_candles, `${path}.timeseries_payload.closed_daily_candles`),
      auction_points: codedStockKeys(payload.auction_points, `${path}.timeseries_payload.auction_points`, (points, pointsPath) => points.map((point, index) => ({ ...point, key: codedStableKey(point.key, `${pointsPath}[${index}].key`) }))),
      continuous_points: codedStockKeys(payload.continuous_points, `${path}.timeseries_payload.continuous_points`),
    },
    seq_from: frame.seq_from,
    seq_to: frame.seq_to,
  };
}

/** runtime 增量摄入翻译：账户持仓键与活动委托 code 译为代码串。 */
export function translateRuntimeDelta(delta: WireRuntimeDelta, path: string): CodedRuntimeDelta {
  const accounts: Record<string, CodedAccountSnap> = Object.create(null);
  for (const [accountId, account] of Object.entries(delta.accounts)) {
    accounts[accountId] = codedAccount(account, `${path}.accounts.${accountId}`);
  }
  return {
    seq_from: delta.seq_from,
    seq_to: delta.seq_to,
    tick: delta.tick,
    day: delta.day,
    phase: delta.phase,
    accounts,
    working_orders: {
      reset: delta.working_orders.reset,
      upserts: delta.working_orders.upserts.map((order, index) => ({
        ...order,
        code: stockCodeOf(order.code, `${path}.working_orders.upserts[${index}].code`),
      })),
      removed: delta.working_orders.removed,
    },
  };
}

/** 引擎更新摄入翻译：事件/事实/帧/快照/增量一次翻齐。 */
export function translateEngineUpdate(update: WireEngineUpdate, path = "EngineUpdate"): CodedEngineUpdate {
  if ("TickBatch" in update) {
    const batch = update.TickBatch;
    return {
      TickBatch: {
        frames: batch.frames.map((frame, index) => codedFrame(frame, `${path}.TickBatch.frames[${index}]`)),
        runtime_snapshot: batch.runtime_snapshot === null ? null : translateSnapshot(batch.runtime_snapshot, `${path}.TickBatch.runtime_snapshot`),
        ...(batch.runtime_delta === undefined ? {} : { runtime_delta: translateRuntimeDelta(batch.runtime_delta, `${path}.TickBatch.runtime_delta`) }),
      },
    };
  }
  const civil = update.CivilUpdate;
  return {
    CivilUpdate: {
      ...civil,
      events: civil.events.map((event, index) => codedEvent(event, `${path}.CivilUpdate.events[${index}]`)),
      facts: civil.facts.map((fact, index) => codedFact(fact, `${path}.CivilUpdate.facts[${index}]`)),
      refresh: {
        ...civil.refresh,
        snapshot: translateSnapshot(civil.refresh.snapshot, `${path}.CivilUpdate.refresh.snapshot`),
        intraday: civil.refresh.intraday.map((frame, index) => codedFrame(frame, `${path}.CivilUpdate.refresh.intraday[${index}]`)),
      },
    },
  };
}

/** 请求层翻译：公共 Intent（code 代码串）→ wire Intent（code 数值 StockId）。 */
export function wireIntentOf(intent: CodedIntent, path = "Intent"): Intent {
  if ("PlaceLimit" in intent) {
    return { PlaceLimit: { ...intent.PlaceLimit, code: stockIdOf(intent.PlaceLimit.code) } };
  }
  if ("PlaceMarket" in intent) {
    return { PlaceMarket: { ...intent.PlaceMarket, code: stockIdOf(intent.PlaceMarket.code) } };
  }
  if ("Cancel" in intent) {
    return { Cancel: { ...intent.Cancel, code: stockIdOf(intent.Cancel.code) } };
  }
  throw new ProtocolError("PROTOCOL_MALFORMED", path, `${path} 包含未处理意图变体：${String(intent)}`);
}
