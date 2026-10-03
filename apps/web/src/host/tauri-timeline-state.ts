import { createBaselineUpdate, type HostUpdate } from "./host-update.ts";
import { parseProtocolSnapshot } from "./protocol/index.ts";

type Baseline = Extract<HostUpdate, { type: "baseline" }>;

export type TauriBaselineResponse = {
  readonly snapshot: unknown;
  readonly timeline_id: string;
  readonly generation: string;
};

type QueryCursor = { readonly generation: string; readonly baselineEpoch: number };

export function nextTauriGeneration(value: string): string {
  return (BigInt(value) + 1n).toString();
}

/** 只拥有 IPC 时间线与基线查询游标；会话 I/O 和运行状态由 adapter 编排。 */
export class TauriTimelineState {
  private timelineId: string | null = null;
  private generation = "1";
  private cachedBaseline: Baseline | null = null;
  private baselineEpoch = 0;

  setInitialTimeline(sessionId: string): void {
    this.timelineId = sessionId;
  }

  currentGeneration(): string {
    return this.generation;
  }

  matchesTimeline(timelineId: string): boolean {
    return this.timelineId === timelineId;
  }

  baselineForDelivery(): Baseline | null {
    return this.cachedBaseline;
  }

  baselineForRead(message = "Tauri 基线尚未就绪"): Baseline {
    if (this.cachedBaseline === null) throw new Error(message);
    return this.cachedBaseline;
  }

  captureGeneration(): string {
    return this.generation;
  }

  captureQueryCursor(): QueryCursor {
    return { generation: this.generation, baselineEpoch: this.baselineEpoch };
  }

  assertGeneration(queryGeneration: string, message: string): void {
    if (this.generation !== queryGeneration) throw new Error(message);
  }

  assertQueryCursor(cursor: QueryCursor, responseGeneration: string, message: string): void {
    if (responseGeneration !== cursor.generation || this.generation !== cursor.generation || this.baselineEpoch !== cursor.baselineEpoch) {
      throw new Error(message);
    }
  }

  installInitialBaseline(response: TauriBaselineResponse): Baseline {
    if (response.generation !== this.generation) throw new Error("Tauri 初始基线 generation 与新会话不匹配");
    this.timelineId = response.timeline_id;
    return this.installSnapshot(response.snapshot, "Tauri engine_baseline.snapshot");
  }

  replaceRefreshedBaseline(response: TauriBaselineResponse, queryGeneration: string): Baseline {
    const message = "Tauri 基线刷新响应属于已过期会话 generation";
    if (response.generation !== queryGeneration) throw new Error(message);
    this.assertGeneration(queryGeneration, message);
    this.timelineId = response.timeline_id;
    return this.installSnapshot(response.snapshot, "Tauri engine_baseline.snapshot");
  }

  replaceRestoredBaseline(response: TauriBaselineResponse): Baseline {
    if (response.generation !== nextTauriGeneration(this.generation)) throw new Error("Tauri 恢复响应没有递增 generation");
    // 保留 generation/timeline 先写、snapshot 后解析的既有部分更新边界。
    this.generation = response.generation;
    this.timelineId = response.timeline_id;
    return this.installSnapshot(response.snapshot, "Tauri restore snapshot");
  }

  clearForDispose(): void {
    this.timelineId = null;
    this.cachedBaseline = null;
  }

  private installSnapshot(snapshot: unknown, where: string): Baseline {
    this.cachedBaseline = createBaselineUpdate(this.generation, parseProtocolSnapshot(snapshot, where));
    this.baselineEpoch += 1;
    return this.cachedBaseline;
  }
}
