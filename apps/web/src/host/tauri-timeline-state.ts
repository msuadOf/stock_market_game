import { prepareBaselineUpdate, type HostUpdate } from "./host-update.ts";
import { parseIsoDate } from "./protocol/wire-values.ts";

type Baseline = Extract<HostUpdate, { type: "baseline" }>;

export type TauriBaselineResponse = {
  readonly snapshot: unknown;
  readonly stock_catalog: unknown;
  readonly company_catalog: unknown;
  readonly timeline_id: string;
  readonly generation: string;
  readonly civil_date: unknown;
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
    return this.installSnapshot(response, "Tauri engine_baseline");
  }

  replaceRefreshedBaseline(response: TauriBaselineResponse, queryGeneration: string): Baseline {
    const message = "Tauri 基线刷新响应属于已过期会话 generation";
    if (response.generation !== queryGeneration) throw new Error(message);
    this.assertGeneration(queryGeneration, message);
    return this.installSnapshot(response, "Tauri engine_baseline");
  }

  replaceRestoredBaseline(response: TauriBaselineResponse): Baseline {
    if (response.generation !== nextTauriGeneration(this.generation)) throw new Error("Tauri 恢复响应没有递增 generation");
    return this.installSnapshot(response, "Tauri restore snapshot");
  }

  clearForDispose(): void {
    this.timelineId = null;
    this.cachedBaseline = null;
  }

  private installSnapshot(response: TauriBaselineResponse, where: string): Baseline {
    const candidate = prepareBaselineUpdate(response.generation, response.snapshot, response.stock_catalog, response.company_catalog, {
      civilDate: parseIsoDate(response.civil_date, `${where}.civil_date`), revision: null,
    });
    this.generation = response.generation;
    this.timelineId = response.timeline_id;
    this.cachedBaseline = candidate;
    this.baselineEpoch += 1;
    return candidate;
  }
}
