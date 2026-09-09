import type { EngineEvent, Snapshot } from "../types/engine";
import type { HostUpdate, SeqCoverage } from "./host-update.ts";
import type { HostFailure } from "./host-update.ts";
import { createDeltaUpdate } from "./host-update.ts";

interface CoordinatorDependencies {
  deliverUpdate(update: HostUpdate): void;
}

export interface TimelineEventGate<T> {
  replaceTimeline(timelineId: string): void;
  accept(timelineId: string, payload: T): boolean;
}

/** 两条 Tauri IPC 路径可能乱序；只让当前读档时间线的事件进入协调器。 */
export function createTimelineEventGate<T>(
  initialTimelineId: string,
  deliver: (payload: T) => void,
): TimelineEventGate<T> {
  if (!initialTimelineId) throw new Error("Tauri timeline id 不能为空");
  let currentTimelineId = initialTimelineId;
  return {
    replaceTimeline(timelineId) {
      if (!timelineId) throw new Error("Tauri timeline id 不能为空");
      currentTimelineId = timelineId;
    },
    accept(timelineId, payload) {
      if (timelineId !== currentTimelineId) return false;
      deliver(payload);
      return true;
    },
  };
}

export async function resumeCommittedTimeline(
  resume: () => Promise<void>,
  reportFailure: (failure: HostFailure) => void,
): Promise<boolean> {
  try {
    await resume();
    return true;
  } catch (error) {
    reportFailure({
      code: "TAURI_RESUME_AFTER_RESTORE",
      message: `读档已经成功，但恢复运行失败；游戏保持暂停：${String(error)}`,
    });
    return false;
  }
}

export async function resumeRejectedTimeline(
  resume: () => Promise<void>,
  restoreError: unknown,
  reportFailure: (failure: HostFailure) => void,
): Promise<void> {
  try {
    await resume();
  } catch (resumeError) {
    const failure = {
      code: "TAURI_RESTORE_AND_RESUME_FAILED",
      message: `读档失败，且恢复原会话运行状态也失败；游戏保持暂停：${String(restoreError)}；${String(resumeError)}`,
    } satisfies HostFailure;
    reportFailure(failure);
    throw new Error(`${failure.code}: ${failure.message}`);
  }
}

/**
 * 桌面 actor 把账户/订单/交易阶段变化事件与同一 tick 的轻量快照装在同一 payload 中。
 * 先交付权威 K 线事件，再用同批快照刷新账户/盘口，不存在异步乱序窗口。
 */
export function createTauriEventCoordinator(deps: CoordinatorDependencies): {
  accept(events: EngineEvent[], runtimeSnapshot: Snapshot | undefined, coverage: SeqCoverage): void;
} {
  return {
    accept(events, runtimeSnapshot, coverage) {
      if (events.length === 0) return;
      deps.deliverUpdate(createDeltaUpdate(events, runtimeSnapshot, coverage));
    },
  };
}
