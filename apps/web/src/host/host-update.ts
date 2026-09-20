import type { Snapshot, StockSpec } from "../types/engine.ts";

export const UI_UPDATE_INTERVAL_MS = 16;
export const UI_TARGET_HZ = 1_000 / UI_UPDATE_INTERVAL_MS;

export type PublicMetadata = {
  readonly civilDate: string | null;
  readonly revision: string | null;
};

export type HostUpdate =
  | ({
      readonly type: "baseline";
      readonly generation: string;
      readonly snapshot: Snapshot;
      readonly securities: readonly StockSpec[];
      readonly publicPublicationIds: readonly string[];
    } & PublicMetadata)
  | ({
      readonly type: "protocol";
      readonly generation: string;
      readonly update: unknown;
    } & PublicMetadata);

export type HostFailure = {
  readonly code: string;
  readonly where: string;
  readonly message: string;
};

export function createBaselineUpdate(
  generation: string,
  snapshot: Snapshot,
  metadata: PublicMetadata = { civilDate: null, revision: null },
  securities: readonly StockSpec[] = [],
  publicPublicationIds: readonly string[] = [],
): Extract<HostUpdate, { type: "baseline" }> {
  if (!/^(0|[1-9]\d*)$/.test(generation)) {
    throw new Error("宿主基线 generation 必须是规范非负十进制整数");
  }
  if (!Number.isSafeInteger(snapshot.seq) || snapshot.seq < 0) {
    throw new Error("宿主基线快照缺少非负安全整数 seq");
  }
  return { type: "baseline", generation, snapshot, ...metadata, securities, publicPublicationIds };
}

export function createProtocolUpdate(
  generation: string,
  update: unknown,
  metadata: PublicMetadata = { civilDate: null, revision: null },
): Extract<HostUpdate, { type: "protocol" }> {
  if (!/^(0|[1-9]\d*)$/.test(generation)) {
    throw new Error("宿主协议更新 generation 必须是规范非负十进制整数");
  }
  return { type: "protocol", generation, update, ...metadata };
}
