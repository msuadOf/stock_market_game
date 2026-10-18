import type { PausePreferences } from "../types/generated/PausePreferences.ts";
import { UI_TARGET_HZ } from "./host-update.ts";
import { HostSpeedMeter, assertValidSpeedMultiplier } from "./speed.ts";
import { inspectWasmUpdateDelivery } from "./wasm-update-delivery.ts";

const TICK_MS = 1_000;
const FRAME_MS = 16;
const MAX_BATCH_UPDATES = 64;

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
  private timerEpoch = 0;
  private running = false;
  private speed = 1;
  private flushMs = 1_000 / UI_TARGET_HZ;
  private lastStepAt = 0;
  private nextDeliveryId = 0;
  private awaitingConsumer = false;
  private readonly outstanding = new Map<number, number>();
  private pendingGeneration = 0;
  private pendingUpdates: unknown[] = [];
  private lastPublishedAt = 0;
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

  publish(rawUpdate: unknown, immediate = true): boolean {
    this.requireDeliveryCapacity();
    const delivery = inspectWasmUpdateDelivery(rawUpdate, this.pausePreferences);
    this.pendingUpdates.push(rawUpdate);
    this.pendingGeneration = this.dependencies.slot.readGeneration();
    if (immediate || delivery.pausesAtBarrier || "CivilUpdate" in delivery.update
      || this.pendingUpdates.length >= MAX_BATCH_UPDATES
      || this.dependencies.now() - this.lastPublishedAt >= this.flushMs) this.flushPending();
    if (delivery.pausesAtBarrier) {
      this.stop();
      this.dependencies.post({ type: "barrierPaused", generation: this.dependencies.slot.readGeneration() });
    }
    return delivery.recordsMarketTick;
  }

  private flushPending(): void {
    if (this.pendingUpdates.length === 0) return;
    const generation = this.dependencies.slot.readGeneration();
    const deliveryId = ++this.nextDeliveryId;
    const updates = this.pendingUpdates;
    this.pendingUpdates = [];
    this.outstanding.set(deliveryId, generation);
    this.lastPublishedAt = this.dependencies.now();
    this.dependencies.post(updates.length === 1
      ? { type: "protocol", generation, deliveryId, update: updates[0], civilDate: null, revision: null }
      : { type: "protocolBatch", generation, deliveryId, updates, civilDate: null, revision: null });
  }

  flushForControl(): void {
    this.discardObsoleteDeliveries();
    this.flushPending();
  }

  requireDeliveryCapacity(): void {
    this.discardObsoleteDeliveries();
    if (this.outstanding.size >= 2) throw new Error("Worker 消费者尚未确认协议帧，请等待 uiFrame 后重试");
  }

  acknowledge(generation: number, deliveryId: number): void {
    if (generation !== this.dependencies.slot.readGeneration() || this.outstanding.get(deliveryId) !== generation) return;
    this.outstanding.delete(deliveryId);
    if (this.running && this.awaitingConsumer && this.timer === null) {
      this.awaitingConsumer = false;
      this.scheduleFrame(0);
    }
  }

  private discardObsoleteDeliveries(): void {
    if (this.pendingGeneration !== this.dependencies.slot.readGeneration()) this.pendingUpdates = [];
    for (const [deliveryId, generation] of this.outstanding) {
      if (generation !== this.dependencies.slot.readGeneration()) this.outstanding.delete(deliveryId);
    }
  }

  stepOnce(immediate = true): boolean {
    try {
      const [session, wasm] = this.dependencies.slot.requireHandle();
      this.requireDeliveryCapacity();
      const rawUpdate = wasm.step(session);
      if (this.publish(rawUpdate, immediate)) this.speedMeter.recordTicks();
      return true;
    } catch (error) {
      this.stop();
      this.dependencies.failure("wasm-worker.step", error);
      return false;
    }
  }

  frame(): void {
    this.timer = null;
    if (!this.running) return;
    this.discardObsoleteDeliveries();
    if (this.outstanding.size >= 2) { this.awaitingConsumer = true; return; }
    const now = this.dependencies.now();
    if (now - this.lastPublishedAt >= this.flushMs) this.flushPending();
    if (this.outstanding.size >= 2) { this.awaitingConsumer = true; return; }
    if (this.speed === Infinity) {
      // Worker 只能在 task 之间处理消息，每次 step 后让出执行权。
      if (!this.stepOnce(false)) return;
    } else {
      const interval = TICK_MS / this.speed;
      if (this.lastStepAt + interval <= now) {
        this.lastStepAt += interval;
        if (!this.stepOnce(false)) return;
      }
    }
    if (!this.running) return;
    if (this.outstanding.size >= 2) { this.awaitingConsumer = true; return; }
    const untilNextTick = this.speed === Infinity ? 0 : this.lastStepAt + TICK_MS / this.speed - this.dependencies.now();
    const untilFlush = this.pendingUpdates.length === 0 ? this.flushMs : this.flushMs - (this.dependencies.now() - this.lastPublishedAt);
    const delay = Math.max(0, Math.min(FRAME_MS, untilFlush, untilNextTick));
    this.scheduleFrame(delay);
  }

  private scheduleFrame(delay: number): void {
    const epoch = this.timerEpoch;
    this.timer = this.dependencies.schedule(() => { if (epoch === this.timerEpoch) this.frame(); }, delay);
  }

  start(): void {
    if (this.running) return;
    this.running = true;
    this.speedMeter.setRunning(true);
    this.lastStepAt = this.dependencies.now();
    this.scheduleFrame(FRAME_MS);
  }

  stop(): void {
    this.flushForControl();
    this.timerEpoch += 1;
    this.running = false;
    this.speedMeter.setRunning(false);
    if (this.timer !== null) this.dependencies.cancel(this.timer);
    this.timer = null;
  }
}
