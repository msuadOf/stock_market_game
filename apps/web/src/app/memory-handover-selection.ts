import type { WasmMemoryHandover, WasmMemoryPendingIntent } from "../host/wasm-memory-handover.ts";
import type { StrictSaveEnvelope } from "../save/schema/root.ts";
import { validateDayEndArchive } from "../save/day-end-candidate.ts";
import type { MemoryHandoverPanelProps } from "./MemoryHandoverPanel.tsx";

interface Ports {
  loadFromFile(): Promise<StrictSaveEnvelope | null>;
  createNewIntent(): Extract<WasmMemoryPendingIntent, { kind: "new" }>;
  onChange(panel: MemoryHandoverPanelProps | null): void;
}
/** 只把一次待确认 Promise 接到现有面板；不持有 host、不读快速槽、不写存档或残留账。 */
export function createMemoryHandoverSelection(ports: Ports) {
  type Intent = Exclude<WasmMemoryPendingIntent, null>;
  type Request = { handover: WasmMemoryHandover; selectedIntent: Intent | null; busy: boolean; error: string | null; resolve(value: WasmMemoryPendingIntent): void };
  let current: Request | null = null;
  const close = (request: Request, value: WasmMemoryPendingIntent) => {
    if (current !== request) return;
    current = null; ports.onChange(null); request.resolve(value);
  };
  const publish = (request: Request) => {
    if (current !== request) return;
    ports.onChange({ handover: request.handover, selectedIntent: request.selectedIntent, busy: request.busy, error: request.error,
      onContinue: () => { if (!request.busy && request.selectedIntent !== null) close(request, request.selectedIntent); },
      onCancel: () => { if (!request.busy) close(request, null); },
      onSelectFile: () => { void selectFile(request); },
      onSelectNew: () => {
        if (current !== request || request.busy) return;
        try { request.selectedIntent = structuredClone(ports.createNewIntent()); request.error = null; }
        catch (error) { request.error = `新局选择失败：${error instanceof Error ? error.message : String(error)}；原选择保持，请反馈此错误。`; }
        publish(request);
      },
    });
  };
  async function selectFile(request: Request) {
    if (current !== request || request.busy) return;
    request.busy = true; request.error = null; publish(request);
    try {
      const loaded = await ports.loadFromFile();
      if (current !== request) return;
      if (loaded !== null) request.selectedIntent = { kind: "load", archive: structuredClone(validateDayEndArchive(loaded)) };
    } catch (error) {
      if (current === request) request.error = `日终存档选择失败：${error instanceof Error ? error.message : String(error)}；原选择与原档保持，请检查文件或反馈。`;
    } finally {
      if (current === request) { request.busy = false; publish(request); }
    }
  }
  return {
    request(handover: WasmMemoryHandover, explicitIntent?: Intent): Promise<WasmMemoryPendingIntent> {
      if (current !== null) throw new Error("上一项 Memory 交接确认尚未完成");
      return new Promise((resolve) => {
        const selectedIntent = explicitIntent === undefined ? handover.kind === "frozen" ? handover.pendingIntent : null : explicitIntent;
        current = { handover: structuredClone(handover), selectedIntent: structuredClone(selectedIntent), busy: false, error: null, resolve };
        publish(current);
      });
    },
    dispose() { if (current !== null) close(current, null); },
  };
}
