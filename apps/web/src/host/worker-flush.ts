import type { EngineEvent, Snapshot } from "../types/engine";
import { createDeltaUpdate, type SeqCoverage } from "./host-update.ts";

interface WorkerFlushPort {
  postMessage(message: unknown): void;
}

export function shouldFlushWorkerEvents(
  pendingCount: number,
  awaitingUiFrame: boolean,
  force: boolean,
): boolean {
  return pendingCount > 0 && (force || !awaitingUiFrame);
}

/** 把 Worker 私有消息转换为三宿主共享的原子 HostUpdate。 */
export function postWorkerFlush(
  port: WorkerFlushPort,
  events: EngineEvent[],
  runtimeSnapshot?: Snapshot,
  coverage?: SeqCoverage,
): void {
  port.postMessage({ type: "hostUpdate", update: createDeltaUpdate(events, runtimeSnapshot, coverage) });
}
