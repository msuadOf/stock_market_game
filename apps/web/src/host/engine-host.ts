import type {
  Intent,
  PublicReportPage,
  PublicReportAvailability,
  PublicReportAvailabilityQuery,
  PublicReportQuery,
  PublicReportSummary,
  Snapshot,
  SessionSetup,
  StockCode,
  HistoricalStockData,
} from "../types/engine";
import type { HostFailure, HostUpdate } from "./host-update.ts";
import type { NpcDecisionTraceRecord } from "./npc-decision-trace.ts";
import type { PausePreferences } from "../types/generated/PausePreferences.ts";
import type { PlayerWorkingOrder } from "./player-working-orders.ts";
import type { IndicatorInput, IndicatorResults } from "../components/indicator-results.ts";
import type { InitialAllocation } from "./initial-allocation.ts";
import type { CompanyReportCorrection, ReportCorrectionStatus } from "./report-corrections.ts";
import type { PersonalTradeHistoryRequest } from "../types/generated/PersonalTradeHistoryRequest";
import type { PersonalTradeHistoryPage } from "../types/generated/PersonalTradeHistoryPage";
export type { PersonalTradeHistoryRequest, PersonalTradeHistoryPage };
import type { MarketHistoryRequest, MarketHistoryPage } from "./market-history.ts";
export type { MarketHistoryRequest, MarketHistoryPage };
import type { CurrentMinuteHistoryRequest, CurrentMinuteHistoryResponse } from "./current-minute-history.ts";
export type { CurrentMinuteHistoryRequest, CurrentMinuteHistoryResponse };

export type RequestedSpeed =
  | { mode: "fixed"; multiplier: number }
  | { mode: "fastest" };

export interface SpeedMetrics {
  requested: RequestedSpeed;
  actual_multiplier: number | null;
  sample_duration_ms: number;
  sample_ticks: number;
  running: boolean;
}

export type DeliveryMode = "push" | "pull";

export interface HostCapabilities {
  persistence: "browser" | "native" | "remote";
  deliveryModes: readonly DeliveryMode[];
  targetUiHz: number;
  sharedMemory: boolean;
  reconnect: boolean;
  publicCompanyReports: boolean;
  npcDecisionDiagnostics: boolean;
  indicatorCapabilities: IndicatorCapabilities;
  personalTradeHistory: boolean;
}

export interface IndicatorCapabilities {
  readonly intradayAverage: boolean;
  readonly macd: boolean;
  readonly priceKdj: boolean;
  readonly candleKdj: boolean;
}

export interface IntradayAverageInput {
  readonly turnoverCents: string;
  readonly tradeCount: number;
  readonly volumeShares: number;
}

export interface IntradayAverageResult {
  readonly turnoverCents: string;
  readonly volumeShares: number;
}

export interface IntradayAverageCurveInput {
  readonly seriesKey: string;
  readonly samples: readonly IntradayAverageInput[];
}

export interface PersonalTradeConfirmation {
  readonly receipt_id: string;
  readonly civil_date: string;
  readonly code: StockCode;
  readonly side: "Buy" | "Sell";
  readonly price: string;
  readonly quantity_shares: number;
  readonly gross: string;
  readonly actual_fees: {
    readonly commission: string;
    readonly stamp_tax: string;
    readonly transfer_fee: string;
  };
}

/** 各部署宿主必须遵守的异步应用层契约。 */
export interface EngineHost {
  marketContext?(): import("./remote-market-context.ts").RemoteMarketContext;
  onMarketContext?(subscriber: (context: import("./remote-market-context.ts").RemoteMarketContext) => void): () => void;
  rejoinMarket?(): Promise<void>;
  resetMarket?(setup: SessionSetup, seed: bigint): Promise<void>;
  setRunning?(running: boolean): Promise<void>;
  setAdmissionCash?(cash: string): Promise<void>;
  archiveStore?: import("../save/archive-store.ts").ArchiveStore;
  startupContext?: { readonly setup: SessionSetup; readonly seed: string; readonly resumed: boolean };
  start(
    onUpdate: (update: HostUpdate) => void | boolean,
    onFatalError?: (failure: HostFailure) => void,
  ): Promise<void>;
  readonly capabilities: HostCapabilities;
  stop(): Promise<void>;
  dispose(): Promise<void>;
  setSpeed(multiplier: number): Promise<void>;
  setPausePreferences(preferences: PausePreferences): Promise<void>;
  setFrameRate(fps: number): void;
  /** 所有部署宿主都必须返回同形的权威 tick/现实秒采样；通信方式对 UI 透明。 */
  readSpeedMetrics(): Promise<SpeedMetrics>;
  /** 仅远程宿主提供；缺省表示该宿主没有网络 Publisher。 */
  getDeliveryMode?(): DeliveryMode;
  setDeliveryMode?(mode: DeliveryMode): void;
  submitIntent(intent: Intent): Promise<void>;
  snapshot(): Snapshot;
  tick(): number;
  day(): number;
  civilDate?(): Promise<string>;
  endCivilDay?(): Promise<void>;
  /** Captures the latest completed day, or the exact completed CivilUpdate candidate when keyed. */
  save(candidate?: { readonly seq: number; readonly settledDate: string }): Promise<unknown>;
  refreshBaseline(): Promise<void>;
  playerWorkingOrders(): Promise<readonly PlayerWorkingOrder[]>;
  queryStockHistory(code: StockCode): Promise<HistoricalStockData>;
  initialAllocation(): Promise<InitialAllocation>;
  calculateIndicators(input: IndicatorInput): Promise<IndicatorResults>;
  calculateIntradayAverage(input: IntradayAverageInput): Promise<IntradayAverageResult | null>;
  calculateIntradayAverageCurve(input: IntradayAverageCurveInput): Promise<readonly (IntradayAverageResult | null)[]>;
  queryPersonalTradeConfirmations(beforeReceipt?: string | null): Promise<readonly PersonalTradeConfirmation[]>;
  queryPersonalTradeHistory(request: PersonalTradeHistoryRequest): Promise<PersonalTradeHistoryPage>;
  queryMarketHistory(request: MarketHistoryRequest): Promise<MarketHistoryPage>;
  queryCurrentMinuteHistory(request: CurrentMinuteHistoryRequest): Promise<CurrentMinuteHistoryResponse>;
  /** 恢复提交后、发布新基线或恢复运行前通知；archiveSlotId 与回调独立，后续失败不撤销提交。 */
  load(slot: unknown, archiveSlotId?: string, onRestored?: () => void): Promise<void>;
  submitReportCorrection(request: CompanyReportCorrection): Promise<void>;
  cancelReportCorrection(operationId: string): Promise<void>;
  queryReportCorrections(): Promise<ReportCorrectionStatus>;
  queryPublicReports?(query: PublicReportQuery): Promise<PublicReportPage>;
  queryPublicReportAvailability?(query: PublicReportAvailabilityQuery): Promise<PublicReportAvailability>;
  publicReportById?(id: string): Promise<PublicReportSummary>;
  npcDecisionTrace?(account: string): Promise<readonly NpcDecisionTraceRecord[]>;
}
