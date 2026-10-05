import type {
  Intent,
  PublicReportPage,
  PublicReportQuery,
  PublicReportSummary,
  Snapshot,
  StockCode,
  HistoricalStockData,
} from "../types/engine";
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
  /** 恢复提交后、发布新基线或恢复运行前通知；后续失败不撤销已提交的恢复。 */
  load(slot: unknown, onRestored?: () => void): Promise<void>;
  queryPublicReports?(query: PublicReportQuery): Promise<PublicReportPage>;
  publicReportById?(id: string): Promise<PublicReportSummary>;
  npcDecisionTrace?(account: number): Promise<readonly NpcDecisionTraceRecord[]>;
}
