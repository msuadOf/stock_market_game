import type { PausePreferences } from "../types/generated/PausePreferences.ts";
import { UI_TARGET_HZ } from "./host-update.ts";
import { HostSpeedMeter, assertValidSpeedMultiplier } from "./speed.ts";
import { inspectWasmUpdateDelivery } from "./wasm-update-delivery.ts";

const TICK_MS = 1_000;
const FRAME_MS = 16;

interface WasmStepSlot {
  requireHandle(): readonly [number, { readonly step: (handle: number) => unknown }];
  readGeneration(): number;
}

export interface WasmTickLoopDependencies<TTimer> {
  readonly slot: WasmStepSlot;
  readonly now: () => number;
  readonly schedule: (callback: () => void, delay: number) => TTimer;
  readonly cancel: (timer: TTimer) => void;
  readonly post: (message: unknown) => void;
  readonly failure: (where: string, error: unknown) => void;
}

/** 循环拥有 timer、节拍、暂停策略和测速；会话句柄由 slot 独立拥有。 */
export class WasmTickLoop<TTimer> {
  private timer: TTimer | null = null;
  private running = false;
  private speed = 1;
  private flushMs = 1_000 / UI_TARGET_HZ;
  private lastStepAt = 0;
  private pausePreferences: PausePreferences = { pause_after_close: false, pause_before_open: false };
  private readonly speedMeter: HostSpeedMeter;
  private readonly dependencies: WasmTickLoopDependencies<TTimer>;

  constructor(dependencies: WasmTickLoopDependencies<TTimer>) {
    this.dependencies = dependencies;
    this.speedMeter = new HostSpeedMeter(dependencies.now);
  }

  isRunning(): boolean {
    return this.running;
  }

  readSpeedMetrics(): ReturnType<HostSpeedMeter["read"]> {
    return this.speedMeter.read();
  }

  setSpeed(speed: number): void {
    assertValidSpeedMultiplier(speed);
    this.speed = speed;
    this.speedMeter.setSpeed(speed);
    this.lastStepAt = this.dependencies.now();
  }

  setFrameRate(fps: unknown): void {
    if (!Number.isFinite(fps) || Number(fps) <= 0) throw new Error("帧率必须是正有限数");
    this.flushMs = 1_000 / Number(fps);
  }

  setPausePreferences(preferences: PausePreferences): void {
    this.pausePreferences = {
      pause_after_close: preferences.pause_after_close,
      pause_before_open: preferences.pause_before_open,
    };
  }

  publish(rawUpdate: unknown): boolean {
    const delivery = inspectWasmUpdateDelivery(rawUpdate, this.pausePreferences);
    this.dependencies.post({ type: "protocol", generation: this.dependencies.slot.readGeneration(), update: rawUpdate, civilDate: null, revision: null });
    if (delivery.pausesAtBarrier) {
      this.stop();
      this.dependencies.post({ type: "barrierPaused", generation: this.dependencies.slot.readGeneration() });
    }
    return delivery.recordsMarketTick;
  }

  stepOnce(): boolean {
    try {
      const [session, wasm] = this.dependencies.slot.requireHandle();
      const rawUpdate = wasm.step(session);
      if (this.publish(rawUpdate)) this.speedMeter.recordTicks();
      return true;
    } catch (error) {
      this.stop();
      this.dependencies.failure("wasm-worker.step", error);
      return false;
    }
  }

  frame(): void {
    if (!this.running) return;
    const now = this.dependencies.now();
    if (this.speed === Infinity) {
      // Worker 只能在 task 之间处理消息，每次 step 后让出执行权。
      if (!this.stepOnce()) return;
    } else {
      const interval = TICK_MS / this.speed;
      if (this.lastStepAt + interval <= now) {
        this.lastStepAt += interval;
        if (!this.stepOnce()) return;
      }
    }
    if (!this.running) return;
    const untilNextTick = this.speed === Infinity ? 0 : this.lastStepAt + TICK_MS / this.speed - this.dependencies.now();
    const delay = Math.max(0, Math.min(FRAME_MS, this.flushMs, untilNextTick));
    this.timer = this.dependencies.schedule(() => this.frame(), delay);
  }

  start(): void {
    if (this.running) return;
    this.running = true;
    this.speedMeter.setRunning(true);
    this.lastStepAt = this.dependencies.now();
    this.timer = this.dependencies.schedule(() => this.frame(), FRAME_MS);
  }

  stop(): void {
    this.running = false;
    this.speedMeter.setRunning(false);
    if (this.timer !== null) this.dependencies.cancel(this.timer);
    this.timer = null;
  }
}
