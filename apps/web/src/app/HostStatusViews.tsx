import { Button, HTMLSelect } from "@blueprintjs/core";
import { useState } from "react";
import type { DeliveryMode } from "../host/engine-host.ts";
import type { HostFailure } from "../host/host-update.ts";
import { buildErrorFeedback, copyErrorFeedback, redactFeedbackText, type FeedbackClipboard, type FeedbackCopyStatus } from "./error-details.ts";

export interface FatalHostErrorProps {
  readonly error: string | HostFailure;
  readonly onRetry: () => void;
  readonly clipboard?: FeedbackClipboard | null;
  readonly title?: string;
  readonly description?: string;
}

function FeedbackCopyButton({ onCopy }: { onCopy: (report: (status: FeedbackCopyStatus) => void) => Promise<void> }) {
  const [status, setStatus] = useState<FeedbackCopyStatus | null>(null);
  return <div>
    <Button onClick={() => void onCopy(setStatus)} disabled={status?.kind === "copying"}>复制错误反馈</Button>
    {status !== null && <p role={status.kind === "failed" ? "alert" : "status"} aria-live="polite">{status.message}</p>}
  </div>;
}

export function FatalHostError({ error, onRetry, clipboard, title = "游戏已崩溃", description = "行情引擎或协议无法继续。请查看真实错误详情并反馈；刷新重试不保证解决原因，且会丢失未保存的日内进度。" }: FatalHostErrorProps) {
  return <div className="app-error" role="alert" aria-live="assertive">
    <h2>{title}</h2>
    <p>{description}</p>
    <Button intent="primary" onClick={onRetry}>刷新页面重试</Button>
    <FeedbackCopyButton onCopy={(report) => copyErrorFeedback(error, report, clipboard)} />
    {typeof error === "string" && <pre>{redactFeedbackText(error)}</pre>}
    <pre aria-label="错误反馈详情">{buildErrorFeedback(error)}</pre>
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
