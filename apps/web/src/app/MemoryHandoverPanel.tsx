import type { WasmMemoryHandover, WasmMemoryPendingIntent } from "../host/wasm-memory-handover.ts";

export interface MemoryHandoverPanelProps {
  handover: WasmMemoryHandover;
  selectedIntent: Exclude<WasmMemoryPendingIntent, null> | null;
  busy: boolean;
  error: string | null;
  onContinue(): void;
  onSelectFile(): void;
  onSelectNew(): void;
  onCancel(): void;
}

function intentSummary(intent: Exclude<WasmMemoryPendingIntent, null>) {
  if (intent.kind === "new") {
    return <><span>新局</span><span>start date {intent.setup.start_date}</span><span>seed {intent.seed}</span></>;
  }
  return <>
    <span>日终存档</span>
    <span>settled date {intent.archive.civil_clock.settled_through}</span>
    <span>seed {intent.archive.seed}</span>
    <span>tick {intent.archive.snapshot.tick}</span>
  </>;
}

function frozenSummary(handover: WasmMemoryHandover) {
  if (handover.kind === "lost") return <span>目标不可用（{handover.reason}）</span>;
  if (handover.pendingIntent === null) return <span>没有冻结的存档或新局目标</span>;
  if (handover.pendingIntent.kind === "new") return intentSummary(handover.pendingIntent);
  return <>
    <span>日终存档</span>
    <span>settled date {handover.pendingIntent.archive.civil_clock.settled_through}</span>
    <span>seed {handover.pendingIntent.archive.seed}</span>
    <span>tick {handover.pendingIntent.archive.snapshot.tick}</span>
  </>;
}

export function MemoryHandoverPanel({ handover, selectedIntent, busy, error, onContinue, onSelectFile, onSelectNew, onCancel }: MemoryHandoverPanelProps) {
  return <section aria-label="重载后存档确认">
    <h2>重载后存档确认</h2>
    <p>{handover.kind === "frozen"
      ? `Memory 残留已计入保守账：本次 ${handover.residualBytes} bytes，累计 ${handover.carriedResidualBytes} bytes。`
      : "交接记录不可用，无法展示 Memory 残留数值；继续前仍须通过保守账校验。"}</p>
    <section aria-label="重载前的选择">
      <h3>重载前的选择</h3>
      <p>{frozenSummary(handover)}</p>
    </section>
    <section aria-label="当前选择">
      <h3>当前选择</h3>
      <p>{selectedIntent === null ? "没有当前选择目标" : intentSummary(selectedIntent)}</p>
    </section>
    {error !== null && <p role="alert">{error}</p>}
    <div>
      <button type="button" disabled={busy || selectedIntent === null} onClick={onContinue}>继续所选目标</button>
      <button type="button" disabled={busy} onClick={onSelectFile}>选择其他日终存档</button>
      <button type="button" disabled={busy} onClick={onSelectNew}>选择当前配置新局</button>
      <button type="button" disabled={busy} onClick={onCancel}>取消，返回启动选择</button>
    </div>
  </section>;
}
