import type { EngineEvent, Snapshot } from "../types/engine";

interface CoordinatorDependencies {
  deliverEvents(events: EngineEvent[]): void;
  deliverSnapshot(snapshot: Snapshot): void;
}

function eventSequence(event: EngineEvent): number {
  const payload = Object.values(event)[0] as { seq?: unknown };
  if (!Number.isSafeInteger(payload?.seq)) {
    throw new Error("Tauri 引擎事件缺少合法 seq，无法保证跨日顺序");
  }
  return payload.seq as number;
}

/**
 * 桌面 actor 把跨日事件与同一 tick 的轻量快照装在同一 payload 中。
 * 先交付权威 K 线事件，再用同批快照刷新账户/盘口，不存在异步乱序窗口。
 */
export function createTauriEventCoordinator(deps: CoordinatorDependencies): {
  accept(events: EngineEvent[], runtimeSnapshot?: Snapshot): void;
} {
  return {
    accept(events, runtimeSnapshot) {
      if (events.length === 0) return;
      let batchSequence = -1;
      for (const event of events) batchSequence = Math.max(batchSequence, eventSequence(event));
      const hasBoundary = events.some((event) => "DayBoundary" in event);
      if (hasBoundary && !runtimeSnapshot) {
        throw new Error("Tauri 跨日事件缺少同批 runtime snapshot");
      }
      if (runtimeSnapshot && runtimeSnapshot.seq < batchSequence) {
        throw new Error("Tauri runtime snapshot 早于同批事件，拒绝回写过期状态");
      }
      deps.deliverEvents(events);
      if (runtimeSnapshot) deps.deliverSnapshot(runtimeSnapshot);
    },
  };
}
