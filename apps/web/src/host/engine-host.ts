import type { EngineEvent, Intent, SaveSlot, Snapshot } from "../types/engine";

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
  submitIntent(intent: Intent): void;
  snapshot(): Snapshot;
  tick(): number;
  day(): number;
  save(): Promise<SaveSlot>;
  load(slot: SaveSlot): Promise<void>;
}
