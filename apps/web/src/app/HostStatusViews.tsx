import { Button, HTMLSelect } from "@blueprintjs/core";
import type { DeliveryMode } from "../host/engine-host.ts";

export function FatalHostError({ error, onRetry }: { error: string; onRetry: () => void }) {
  return <div className="app-error" role="alert" aria-live="assertive">
    <h2>游戏已崩溃</h2>
    <p>行情引擎或协议无法继续。请根据下方原因修复运行环境后刷新页面。</p>
    <Button intent="primary" onClick={onRetry}>刷新页面重试</Button>
    <pre>{error}</pre>
  </div>;
}

export function SpeedMetricsAlert({ error }: { error: string | null }) {
  return error === null ? null : <div className="speed-metrics-error" role="alert">{error}</div>;
}

interface DeliveryModeControlProps {
  mode: DeliveryMode | null;
  modes: readonly DeliveryMode[];
  labels: Record<DeliveryMode, string>;
  onChange: (mode: DeliveryMode) => void;
}

export function DeliveryModeControl({ mode, modes, labels, onChange }: DeliveryModeControlProps) {
  if (mode === null || modes.length === 0) return null;
  return <label className="delivery-mode-control">
    <span>刷新</span>
    <HTMLSelect
      className="delivery-select"
      aria-label="客户端 Publisher 刷新模式"
      value={mode}
      onChange={(event) => onChange(event.target.value as DeliveryMode)}
      options={modes.map((supportedMode) => ({ label: labels[supportedMode], value: supportedMode }))}
    />
  </label>;
}
