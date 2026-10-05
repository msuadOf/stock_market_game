import { useEffect, type MutableRefObject } from "react";
import type { EngineHost, SpeedMetrics } from "../host/engine-host.ts";
import type { SpeedMetricsRequestGate } from "../host/speed.ts";

interface PollingPorts<Timer> {
  host: Pick<EngineHost, "readSpeedMetrics"> | null;
  ready: boolean;
  requestGate: SpeedMetricsRequestGate;
  loadInProgress: MutableRefObject<boolean>;
  onMetrics(metrics: SpeedMetrics): void;
  onError(error: string | null): void;
  schedule(callback: () => void, delay: number): Timer;
  cancel(timer: Timer): void;
}

/** 每次 effect 单独持有取消标志与 timer，借用 AppShell 的共享请求令牌。 */
export function createSpeedMetricsPolling<Timer>(ports: PollingPorts<Timer>) {
  let cancelled = false;
  let timer: Timer | null = null;
  const requestGeneration = ports.requestGate.capture();
  const isCurrent = () => !cancelled && ports.requestGate.isCurrent(requestGeneration);
  async function poll() {
    try {
      const metrics = await ports.host!.readSpeedMetrics();
      if (isCurrent()) { ports.onMetrics(metrics); ports.onError(null); }
    } catch (metricsError) {
      if (isCurrent()) ports.onError(`实际倍速读取失败：${metricsError instanceof Error ? metricsError.message : String(metricsError)}；1 秒后自动重试`);
    } finally {
      if (isCurrent()) timer = ports.schedule(() => void poll(), 1_000);
    }
  }
  return {
    start() { if (ports.ready && ports.host && !ports.loadInProgress.current) void poll(); },
    dispose() { cancelled = true; if (timer !== null) ports.cancel(timer); },
  };
}

interface Options {
  hostRef: MutableRefObject<EngineHost | null>;
  ready: boolean;
  speed: number;
  running: boolean;
  pollingGeneration: number;
  requestGate: SpeedMetricsRequestGate;
  loadInProgress: MutableRefObject<boolean>;
  onMetrics(metrics: SpeedMetrics): void;
  onError(error: string | null): void;
}

export function useSpeedMetricsPolling(options: Options): void {
  const { ready, speed, running, pollingGeneration } = options;
  useEffect(() => {
    const polling = createSpeedMetricsPolling({ ...options, host: options.hostRef.current, schedule: (callback, delay) => window.setTimeout(callback, delay), cancel: timer => window.clearTimeout(timer) });
    polling.start();
    return () => polling.dispose();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ready, speed, running, pollingGeneration]);
}
