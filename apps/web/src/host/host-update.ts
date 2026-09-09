import type { EngineEvent, Snapshot } from "../types/engine.ts";
import { requiresRuntimeSnapshot } from "./runtime-snapshot-policy.ts";

export const UI_UPDATE_INTERVAL_MS = 16;
export const UI_TARGET_HZ = 1_000 / UI_UPDATE_INTERVAL_MS;

export type HostUpdate =
  | { type: "baseline"; snapshot: Snapshot }
  | {
      type: "delta";
      fromSeq: number;
      toSeq: number;
      events: EngineEvent[];
      runtimeSnapshot?: Snapshot;
    };

export interface HostFailure {
  code: string;
  message: string;
}

export interface SeqCoverage {
  fromSeq: number;
  toSeq: number;
}

export function hostEventSeq(event: EngineEvent): number {
  const payload = Object.values(event)[0] as { seq?: unknown } | undefined;
  if (!Number.isSafeInteger(payload?.seq) || Number(payload?.seq) < 0) {
    throw new Error("宿主事件缺少非负安全整数 seq");
  }
  return Number(payload!.seq);
}

export function createBaselineUpdate(snapshot: Snapshot): HostUpdate {
  if (!Number.isSafeInteger(snapshot.seq) || snapshot.seq < 0) {
    throw new Error("宿主基线快照缺少非负安全整数 seq");
  }
  return { type: "baseline", snapshot };
}

export function createDeltaUpdate(
  events: EngineEvent[],
  runtimeSnapshot?: Snapshot,
  coverage?: SeqCoverage,
): Extract<HostUpdate, { type: "delta" }> {
  if (events.length === 0) throw new Error("宿主 delta 必须包含至少一个事件");
  const seqs = events.map(hostEventSeq);
  for (let index = 1; index < seqs.length; index += 1) {
    if (seqs[index]! <= seqs[index - 1]!) {
      throw new Error("宿主 delta 事件 seq 必须严格递增");
    }
  }
  const fromSeq = coverage?.fromSeq ?? seqs[0]!;
  const toSeq = coverage?.toSeq ?? seqs.at(-1)!;
  if (!Number.isSafeInteger(fromSeq) || !Number.isSafeInteger(toSeq)
    || fromSeq < 0 || toSeq < fromSeq
    || seqs.some((seq) => seq < fromSeq || seq > toSeq)) {
    throw new Error("宿主 delta 的 seq 覆盖区间无效");
  }
  if (requiresRuntimeSnapshot(events) && runtimeSnapshot === undefined) {
    throw new Error("会改变权威状态的宿主 delta 缺少权威运行快照");
  }
  if (runtimeSnapshot !== undefined && runtimeSnapshot.seq !== toSeq) {
    throw new Error("宿主 delta 的运行快照 seq 必须等于覆盖区间末尾");
  }
  return { type: "delta", fromSeq, toSeq, events, runtimeSnapshot };
}
