import type {
  PublicReportPage,
  PublicReportQuery,
  PublicReportSummary,
  StockCode,
  HistoricalStockData,
} from "../types/engine";
import type { CodedIntent, CodedSnapshot } from "./protocol/stock-identity.ts";
import type { HostFailure, HostUpdate } from "./host-update.ts";
import type { NpcDecisionTraceRecord } from "./npc-decision-trace.ts";
import type { PausePreferences } from "../types/generated/PausePreferences.ts";
import type { PlayerWorkingOrder } from "./player-working-orders.ts";
import type { IndicatorInput, IndicatorResults } from "../components/indicator-results.ts";
import type { InitialAllocation } from "./initial-allocation.ts";

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

/** 指标源能力面：宿主按指标名声明 Rust 侧是否可算；前端源能力由 indicator-source-policy 固定。 */
export interface IndicatorCapabilities {
  readonly intradayAverage: boolean;
  readonly macd: boolean;
  readonly priceKdj: boolean;
  readonly candleKdj: boolean;
}

/** Rust 分时均价单点输入：真实累计成交额（分）、成交笔数与成交股数。 */
export interface IntradayAverageInput {
  readonly turnoverCents: string;
  readonly tradeCount: number;
  readonly volumeShares: number;
}

/** Rust 分时均价单点结果：回传经宿主核对的累计额与量，供前端换算均价。 */
export interface IntradayAverageResult {
  readonly turnoverCents: string;
  readonly volumeShares: number;
}

/** Rust 分时均价曲线请求：seriesKey 标识幂等去重，samples 为按时间排序的累计样本。 */
export interface IntradayAverageCurveInput {
  readonly seriesKey: string;
  readonly samples: readonly IntradayAverageInput[];
}

export interface HostCapabilities {
  deliveryModes: readonly DeliveryMode[];
  targetUiHz: number;
  sharedMemory: boolean;
  reconnect: boolean;
  publicCompanyReports: boolean;
  npcDecisionDiagnostics: boolean;
}

/** 各部署宿主必须遵守的异步应用层契约。 */
export interface EngineHost {
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
  submitIntent(intent: CodedIntent): Promise<void>;
  snapshot(): CodedSnapshot;
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
  /** 恢复提交后、发布新基线或恢复运行前通知；后续失败不撤销已提交的恢复。 */
  load(slot: unknown, onRestored?: () => void): Promise<void>;
  queryPublicReports?(query: PublicReportQuery): Promise<PublicReportPage>;
  publicReportById?(id: string): Promise<PublicReportSummary>;
  npcDecisionTrace?(account: number): Promise<readonly NpcDecisionTraceRecord[]>;
}
