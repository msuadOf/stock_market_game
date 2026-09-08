import type { EngineEvent, Intent, SaveSlot, Snapshot } from "../types/engine";

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

/** 各部署宿主必须遵守的异步应用层契约。 */
export interface EngineHost {
  start(
    onEvents: (events: EngineEvent[]) => void,
    onSnapshot?: (snapshot: Snapshot) => void,
    onFatalError?: (message: string) => void,
  ): void;
  stop(): void;
  dispose(): void;
  setSpeed(multiplier: number): void;
  setFrameRate(fps: number): void;
  /** 所有部署宿主都必须返回同形的权威 tick/现实秒采样；通信方式对 UI 透明。 */
  readSpeedMetrics(): Promise<SpeedMetrics>;
  submitIntent(intent: Intent): Promise<void>;
  snapshot(): Snapshot;
  tick(): number;
  day(): number;
  save(): Promise<SaveSlot>;
  load(slot: SaveSlot): Promise<void>;
}
