import type { EngineEvent, Snapshot } from "../types/engine";

interface WorkerFlushPort {
  postMessage(message: unknown): void;
}

/** Preserve the causal order expected by the UI: events first, resulting snapshot second. */
export function postWorkerFlush(
  port: WorkerFlushPort,
  events: EngineEvent[],
  runtimeSnapshot?: Snapshot,
): void {
  port.postMessage({ type: "events", events });
  if (runtimeSnapshot !== undefined) {
    port.postMessage({ type: "snapshot", snapshot: runtimeSnapshot });
  }
}
