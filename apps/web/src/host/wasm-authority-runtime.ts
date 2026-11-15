import type { PausePreferences } from "../types/generated/PausePreferences.ts";
import { WorkerTaskScheduler } from "./worker-task-scheduler.ts";

export interface MarketBindings {
  readonly market_runner_enable: (handle: number) => void;
  readonly market_runner_set_running: (handle: number, running: boolean) => void;
  readonly market_runner_set_speed: (handle: number, multiplier: number) => void;
  readonly market_runner_set_pause: (handle: number, afterClose: boolean, beforeOpen: boolean) => void;
  readonly market_runner_speed_metrics: (handle: number) => unknown;
  readonly market_runner_turn: (handle: number) => unknown;
  readonly market_runner_step_once: (handle: number) => void;
  readonly market_runner_end_civil: (handle: number) => void;
  readonly market_runner_observer_baseline: (handle: number) => unknown;
  readonly market_runner_control_signal: (handle: number) => number;
  readonly market_runner_command_consumed: (handle: number) => void;
}

type BoundMarket = readonly [number, MarketBindings];

/** Browser ABI 接线：遵循 Rust 给出的唤醒时刻；不检查 UI 或发布端的进度。 */
export class WasmAuthorityRuntime {
  private timer: number | null = null;
  private timerEpoch = 0;
  private running = false; // Rust 在当前 Worker 的最近一次回执，不参与计算节奏。
  private current: BoundMarket | null = null;
  private speed = 1;
  private pause: PausePreferences = { pause_after_close: false, pause_before_open: false };

  private readonly scheduler: Pick<WorkerTaskScheduler, "schedule" | "cancel">;
  private readonly failure: (where: string, error: unknown) => void;

  constructor(scheduler: Pick<WorkerTaskScheduler, "schedule" | "cancel">, failure: (where: string, error: unknown) => void) {
    this.scheduler = scheduler;
    this.failure = failure;
  }

  bind(handle: number, wasm: MarketBindings): void {
    this.stop();
    wasm.market_runner_enable(handle);
    wasm.market_runner_set_speed(handle, this.speed);
    wasm.market_runner_set_pause(handle, this.pause.pause_after_close, this.pause.pause_before_open);
    this.current = [handle, wasm];
  }

  private requireMarket(): BoundMarket {
    if (this.current === null) throw new Error("市场推进没有绑定 Rust 会话");
    return this.current;
  }

  isRunning(): boolean {
    return this.running;
  }

  readSpeedMetrics(): unknown {
    const [handle, wasm] = this.requireMarket();
    return wasm.market_runner_speed_metrics(handle);
  }

  setSpeed(speed: number): void {
    const [handle, wasm] = this.requireMarket();
    wasm.market_runner_set_speed(handle, speed);
    this.speed = speed;
    this.wake();
  }

  setPausePreferences(preferences: PausePreferences): void {
    const [handle, wasm] = this.requireMarket();
    wasm.market_runner_set_pause(handle, preferences.pause_after_close, preferences.pause_before_open);
    this.pause = preferences;
  }

  start(): void {
    if (this.running) return;
    const [handle, wasm] = this.requireMarket();
    wasm.market_runner_set_running(handle, true);
    this.running = true;
    this.wake();
  }

  stop(): void {
    this.cancelPending();
    if (this.running && this.current !== null) {
      const [handle, wasm] = this.current;
      wasm.market_runner_set_running(handle, false);
    }
    this.running = false;
  }

  stepOnce(): void {
    const [handle, wasm] = this.requireMarket();
    wasm.market_runner_step_once(handle);
  }

  endCivil(): void {
    const [handle, wasm] = this.requireMarket();
    wasm.market_runner_end_civil(handle);
    this.wake();
  }

  confirmControl(handle: number): void {
    const [active, wasm] = this.requireMarket();
    if (active !== handle) throw new Error("市场控制序号来自已过期会话");
    wasm.market_runner_command_consumed(handle);
    this.wake();
  }

  private cancelPending(): void {
    this.timerEpoch += 1;
    if (this.timer !== null) this.scheduler.cancel(this.timer);
    this.timer = null;
  }

  private wake(): void {
    if (!this.running) return;
    this.cancelPending();
    this.scheduleTurn(0);
  }

  private scheduleTurn(delay: number): void {
    const epoch = this.timerEpoch;
    this.timer = this.scheduler.schedule(() => {
      if (this.timerEpoch === epoch) this.turn();
    }, delay);
  }

  private turn(): void {
    this.timer = null;
    if (!this.running) return;
    try {
      const [handle, wasm] = this.requireMarket();
      const result = wasm.market_runner_turn(handle);
      if (result === null || typeof result !== "object" || Array.isArray(result)) throw new Error("Rust 推进回执不是对象");
      const { delayMs, running, committedTicks } = result as Record<string, unknown>;
      if (typeof running !== "boolean" || typeof committedTicks !== "number" || !Number.isSafeInteger(committedTicks) || committedTicks < 0
        || (delayMs !== null && (typeof delayMs !== "number" || !Number.isFinite(delayMs) || delayMs < 0))
        || running !== (delayMs !== null)) {
        throw new Error("Rust 推进回执的 WakePlan 非法");
      }
      this.running = running;
      if (delayMs !== null) this.scheduleTurn(delayMs);
    } catch (error) {
      this.running = false;
      this.failure("wasm-worker.market-runner", error);
    }
  }
}
