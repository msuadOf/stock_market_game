import type { Intent, SaveSlot, Snapshot } from "../types/engine";
import type { HostFailure, HostUpdate } from "./host-update.ts";

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
}

/** 各部署宿主必须遵守的异步应用层契约。 */
export interface EngineHost {
  start(
    onUpdate: (update: HostUpdate) => void,
    onFatalError?: (failure: HostFailure) => void,
  ): void;
  readonly capabilities: HostCapabilities;
  stop(): void;
  dispose(): void;
  setSpeed(multiplier: number): void;
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
  save(): Promise<SaveSlot>;
  load(slot: SaveSlot): Promise<void>;
}
