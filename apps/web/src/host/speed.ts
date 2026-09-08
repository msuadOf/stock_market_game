import type { RequestedSpeed, SpeedMetrics } from "./engine-host.ts";

const SPEED_SAMPLE_MIN_DURATION_MS = 500;

export function assertValidSpeedMultiplier(speed: number): void {
  if (speed === Infinity) return;
  if (!Number.isFinite(speed) || speed <= 0) {
    throw new Error(`非法速度倍率：${speed}（必须为正数或 Infinity）`);
  }
}

export function speedMetricsMatchesRequested(requested: RequestedSpeed, speed: number): boolean {
  return speed === Infinity
    ? requested.mode === "fastest"
    : requested.mode === "fixed" && requested.multiplier === speed;
}

/** 防止调速、暂停或恢复期间把已经过期的异步采样显示成当前状态。 */
export function speedMetricsMatchesUiState(
  metrics: SpeedMetrics,
  speed: number,
  running: boolean,
): boolean {
  return metrics.running === running && speedMetricsMatchesRequested(metrics.requested, speed);
}

/**
 * 为异步测速请求提供与传输方式无关的失效令牌。读档等会替换权威时间线的操作先
 * invalidate，之前仍在网络、IPC 或 Worker 队列中的响应便不能回写新时间线。
 */
export class SpeedMetricsRequestGate {
  private generation = 0;

  capture(): number {
    return this.generation;
  }

  invalidate(): void {
    this.generation += 1;
  }

  isCurrent(generation: number): boolean {
    return generation === this.generation;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** 校验 WASM Worker、Tauri IPC 与远程 HTTP 共用的测速响应协议。 */
export function parseSpeedMetrics(value: unknown): SpeedMetrics {
  if (!isRecord(value) || !isRecord(value.requested)) {
    throw new TypeError("宿主倍速统计必须包含 requested 对象");
  }
  const requested = value.requested;
  const validRequested = requested.mode === "fastest"
    || (requested.mode === "fixed"
      && typeof requested.multiplier === "number"
      && Number.isFinite(requested.multiplier)
      && requested.multiplier > 0);
  if (!validRequested) throw new TypeError("宿主倍速统计包含非法 requested 模式");
  const actual = value.actual_multiplier;
  if (actual !== null && (typeof actual !== "number" || !Number.isFinite(actual) || actual < 0)) {
    throw new RangeError(`actual_multiplier 必须是非负有限数或 null，收到 ${String(actual)}`);
  }
  if (!Number.isSafeInteger(value.sample_duration_ms) || Number(value.sample_duration_ms) < 0) {
    throw new RangeError("sample_duration_ms 必须是非负安全整数");
  }
  if (!Number.isSafeInteger(value.sample_ticks) || Number(value.sample_ticks) < 0) {
    throw new RangeError("sample_ticks 必须是非负安全整数");
  }
  if (typeof value.running !== "boolean") throw new TypeError("running 必须是布尔值");
  if (!value.running && (actual !== 0 || value.sample_duration_ms !== 0 || value.sample_ticks !== 0)) {
    throw new RangeError("暂停状态的实测倍率、采样时长和 tick 数必须全部为 0");
  }
  return value as unknown as SpeedMetrics;
}

/**
 * 宿主侧统一实际倍率采样器。调用方记录已经完成的权威 engine step，采样器只负责
 * 将 tick 增量换算为现实秒倍率；它不感知 WASM、IPC 或网络传输方式。
 */
export class HostSpeedMeter {
  private readonly now: () => number;
  private requested: RequestedSpeed = { mode: "fixed", multiplier: 1 };
  private running = false;
  private sampleStartedAt: number;
  private pendingTicks = 0;
  private actualMultiplier: number | null = 0;
  private sampleDurationMs = 0;
  private sampleTicks = 0;

  constructor(now: () => number = () => performance.now()) {
    this.now = now;
    this.sampleStartedAt = this.readClock();
  }

  setSpeed(speed: number): void {
    assertValidSpeedMultiplier(speed);
    this.requested = speed === Infinity
      ? { mode: "fastest" }
      : { mode: "fixed", multiplier: speed };
    this.resetSample();
  }

  setRunning(running: boolean): void {
    if (this.running === running) return;
    this.running = running;
    this.resetSample();
  }

  recordTicks(count = 1): void {
    if (!Number.isSafeInteger(count) || count < 0) {
      throw new RangeError(`实际倍速采样的 tick 增量必须是非负安全整数，收到 ${String(count)}`);
    }
    this.pendingTicks += count;
    if (!Number.isSafeInteger(this.pendingTicks)) {
      throw new RangeError("实际倍速采样的 tick 计数超出安全整数范围");
    }
  }

  read(): SpeedMetrics {
    const now = this.readClock();
    const elapsed = now - this.sampleStartedAt;
    if (this.running && elapsed >= SPEED_SAMPLE_MIN_DURATION_MS) {
      this.actualMultiplier = this.pendingTicks / (elapsed / 1_000);
      this.sampleDurationMs = Math.round(elapsed);
      this.sampleTicks = this.pendingTicks;
      this.sampleStartedAt = now;
      this.pendingTicks = 0;
    }
    return {
      requested: { ...this.requested },
      actual_multiplier: this.actualMultiplier,
      sample_duration_ms: this.sampleDurationMs,
      sample_ticks: this.sampleTicks,
      running: this.running,
    };
  }

  private resetSample(): void {
    this.sampleStartedAt = this.readClock();
    this.pendingTicks = 0;
    this.actualMultiplier = this.running ? null : 0;
    this.sampleDurationMs = 0;
    this.sampleTicks = 0;
  }

  private readClock(): number {
    const value = this.now();
    if (!Number.isFinite(value) || value < 0) {
      throw new RangeError(`实际倍速采样时钟必须是非负有限数，收到 ${String(value)}`);
    }
    if (this.sampleStartedAt !== undefined && value < this.sampleStartedAt) {
      throw new RangeError("实际倍速采样时钟发生倒退");
    }
    return value;
  }
}
