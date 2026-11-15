import type { RequestedSpeed, SpeedMetrics } from "./engine-host.ts";

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
  // 普通暂停清空窗口；停止于故障则保留最后一次完整权威采样。
  if (value.sample_duration_ms === 0 && (value.sample_ticks !== 0 || (actual !== null && actual !== 0))) {
    throw new RangeError("未采样时的 tick 数和实测倍率无效");
  }
  return value as unknown as SpeedMetrics;
}
