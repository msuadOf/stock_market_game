/**
 * Web 边界类型适配层。
 *
 * Rust/serde 数据结构由 ts-rs 生成到 `./generated`；本文件只保留前端惯用别名
 * 与宿主方法契约。修改 Rust 边界类型后运行 `pnpm types:generate`，不要手改生成文件。
 */

import type { Event } from "./generated/Event";
import type { Money } from "./generated/Money";

export type { AccountId } from "./generated/AccountId";
export type { AccountSnap } from "./generated/AccountSnap";
export type { AuctionOrderSnap } from "./generated/AuctionOrderSnap";
export type { DailyTradeStats } from "./generated/DailyTradeStats";
export type { FloatAllocation } from "./generated/FloatAllocation";
export type { GameConfig } from "./generated/GameConfig";
export type { Intent } from "./generated/Intent";
export type { EngineUpdate } from "./generated/EngineUpdate";
export type { MarketSnap } from "./generated/MarketSnap";
export type { MarketMinuteClose } from "./generated/MarketMinuteClose";
export type { NpcAttentionState } from "./generated/NpcAttentionState";
export type { NpcSetup } from "./generated/NpcSetup";
export type { PositionSnap } from "./generated/PositionSnap";
export type { PublicComparativeAmount } from "./generated/PublicComparativeAmount";
export type { PublicReportAccountingSummary } from "./generated/PublicReportAccountingSummary";
export type { PublicReportKind } from "./generated/PublicReportKind";
export type { PublicReportPage } from "./generated/PublicReportPage";
export type { PublicReportQuery } from "./generated/PublicReportQuery";
export type { PublicReportSummary } from "./generated/PublicReportSummary";
export type { PublicUnavailableReason } from "./generated/PublicUnavailableReason";
export type { RejectionReason } from "./generated/RejectionReason";
export type { SaveSlot } from "./generated/SaveSlot";
export type { SecurityCategory } from "./generated/SecurityCategory";
export type { StockExchange } from "./generated/StockExchange";
export type { SessionSetup } from "./generated/SessionSetup";
export type { Side } from "./generated/Side";
export type { Snapshot } from "./generated/Snapshot";
export type { StockCode } from "./generated/StockCode";
export type { StockSpec } from "./generated/StockSpec";
export type { StrategyParams } from "./generated/StrategyParams";
export type { TradingPhase } from "./generated/TradingPhase";

/** 金额（分）。 */
export type Cents = Money;
export type EngineEvent = Event;
export type DailyCandleSnap = import("./generated/DailyCandle").DailyCandle;
export type RestingOrderSnap = import("./generated/Order").Order;
export type PriceLevel = [Cents, number];

export type TradeEvent = Extract<Event, { Trade: unknown }>["Trade"];
export type PriceTickEvent = Extract<Event, { PriceTick: unknown }>["PriceTick"];
export type AuctionTickEvent = Extract<Event, { AuctionTick: unknown }>["AuctionTick"];
export type AuctionCompletedEvent = Extract<Event, { AuctionCompleted: unknown }>["AuctionCompleted"];
export type DayBoundaryEvent = Extract<Event, { DayBoundary: unknown }>["DayBoundary"];
export type CivilDateAdvancedEvent = Extract<Event, { CivilDateAdvanced: unknown }>["CivilDateAdvanced"];
export type CompanyDisclosurePublishedEvent = Extract<Event, { CompanyDisclosurePublished: unknown }>["CompanyDisclosurePublished"];
export type IntentRejectedEvent = Extract<Event, { IntentRejected: unknown }>["IntentRejected"];
export type SettlementErrorEvent = Extract<Event, { SettlementError: unknown }>["SettlementError"];
export type OrderCanceledEvent = Extract<Event, { OrderCanceled: unknown }>["OrderCanceled"];
export type OrderAcceptedEvent = Extract<Event, { OrderAccepted: unknown }>["OrderAccepted"];

// ── WASM 句柄 API（apps/web-wasm/pkg/web_wasm.d.ts 镜像）──

export interface WasmApi {
  create_session(setup: import("./generated/SessionSetup").SessionSetup, seed: bigint): number;
  step(handle: number): import("./generated/EngineUpdate").EngineUpdate;
  snapshot(handle: number): import("./generated/Snapshot").Snapshot;
  runtime_snapshot(handle: number): import("./generated/Snapshot").Snapshot;
  tick(handle: number): bigint;
  day(handle: number): number;
  civil_date(handle: number): string;
  end_civil_day(handle: number): import("./generated/EngineUpdate").EngineUpdate;
  public_report_page(
    handle: number,
    query: import("./generated/PublicReportQuery").PublicReportQuery,
  ): import("./generated/PublicReportPage").PublicReportPage;
  public_report_by_id(
    handle: number,
    id: string,
  ): import("./generated/PublicReportSummary").PublicReportSummary;
  npc_decision_trace(
    handle: number,
    account: bigint,
  ): import("../host/npc-decision-trace").NpcDecisionTraceRecord[];
  enqueue(handle: number, intent: import("./generated/Intent").Intent): void;
  save(handle: number): import("./generated/SaveSlot").SaveSlot;
  restore(slot: import("./generated/SaveSlot").SaveSlot): number;
  restore_json(saveJson: string): number;
  drop_session(handle: number): void;
}
