import { parseSetup } from "../save/schema/market.ts";
import { parseSaveSlot } from "../save/save-schema.ts";
import { validateDayEndArchive } from "../save/day-end-candidate.ts";
import { parsePlanU64 } from "../utils/plan-identity.ts";
import type { SessionSetup } from "../types/engine.ts";
import type { StrictSaveEnvelope } from "../save/schema/root.ts";

export type WasmMemoryPendingIntent = null
  | { readonly kind: "new"; readonly setup: SessionSetup; readonly seed: string }
  | { readonly kind: "load"; readonly archive: StrictSaveEnvelope };
export type WasmMemoryHandover = {
  readonly kind: "frozen";
  readonly code: "MEMORY_RESIDUAL_RELOAD_HANDOVER";
  readonly residualBytes: number;
  readonly carriedResidualBytes: number;
  readonly thresholdEffective: number;
  readonly reloadCount: number;
  readonly pendingIntent: WasmMemoryPendingIntent;
  readonly last_committed_tick: number;
  readonly day: number;
  readonly handoverNonce: string;
} | {
  readonly kind: "lost";
  readonly reason: "checksum-invalid" | "schema-invalid" | "intent-invalid" | "carrier-unavailable";
};

function record(value: unknown, path: string): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error(`${path} 必须为JSON对象`);
  return value as Record<string, unknown>;
}
function exact(row: Record<string, unknown>, keys: readonly string[], path: string): void {
  if (Object.keys(row).some((key) => !keys.includes(key)) || keys.some((key) => !Object.hasOwn(row, key))) {
    throw new Error(`${path} 字段缺失或含未知字段`);
  }
}
function integer(value: unknown, path: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) throw new Error(`${path} 必须为非负safe integer`);
  return value;
}
// storage输入虽应来自JSON，公开parser仍须在clone前拒绝会被静默丢弃的字段。
function checkJson(value: unknown, path: string, ancestors = new Set<object>()): void {
  if (value === null || typeof value === "string" || typeof value === "boolean") return;
  if (typeof value === "number" && Number.isFinite(value)) return;
  if (typeof value !== "object" || value === null) throw new Error(`${path} 不是JSON值`);
  if (ancestors.has(value)) throw new Error(`${path} 包含循环引用`);
  const array = Array.isArray(value);
  if (!array && Object.getPrototypeOf(value) !== Object.prototype && Object.getPrototypeOf(value) !== null) throw new Error(`${path} 不是普通JSON对象`);
  ancestors.add(value);
  try {
    const descriptors = Object.getOwnPropertyDescriptors(value);
    for (const key of Reflect.ownKeys(descriptors)) {
      if (typeof key === "symbol") throw new Error(`${path} 含Symbol字段`);
      if (array && key === "length") continue;
      const descriptor = descriptors[key]!;
      if (!Object.hasOwn(descriptor, "value")) throw new Error(`${path}.${key} 是访问器字段`);
      if (!descriptor.enumerable || (array && (!/^(0|[1-9][0-9]*)$/.test(key) || Number(key) >= value.length))) throw new Error(`${path}.${key} 不是JSON字段`);
      checkJson(descriptor.value, `${path}.${key}`, ancestors);
    }
    if (array && Object.keys(value).length !== value.length) throw new Error(`${path} 是稀疏JSON数组`);
  } finally { ancestors.delete(value); }
}
function pendingIntent(value: unknown): WasmMemoryPendingIntent {
  if (value === null) return null;
  const row = record(value, "pendingIntent");
  if (row.kind === "new") {
    exact(row, ["kind", "setup", "seed"], "pendingIntent.new");
    const setup = parseSetup(row.setup, "pendingIntent.setup");
    if (setup.simulation_policy_id !== "a-share-simulation") throw new Error("pendingIntent.setup.simulation_policy_id 必须为 a-share-simulation");
    return { kind: "new", setup, seed: parsePlanU64(row.seed, "pendingIntent.seed") };
  }
  if (row.kind === "load") {
    exact(row, ["kind", "archive"], "pendingIntent.load");
    return { kind: "load", archive: validateDayEndArchive(parseSaveSlot(row.archive)) };
  }
  throw new Error("pendingIntent.kind 必须为new或load");
}

/** 在root checksum前取得无访问器、无静默丢字段的隔离JSON副本。 */
export function copyWasmMemoryHandoverJson(value: unknown): unknown {
  checkJson(value, "handover");
  return structuredClone(value);
}

/** 仅解析nested冻结历史或目标丢失标记；root账面与checksum归单键codec。 */
export function parseWasmMemoryHandover(value: unknown, maximumBytes: number): WasmMemoryHandover {
  if (integer(maximumBytes, "maximumBytes") === 0) throw new Error("maximumBytes 必须为正bytes");
  const row = record(copyWasmMemoryHandoverJson(value), "handover");
  if (row.kind === "lost") {
    exact(row, ["kind", "reason"], "handover.lost");
    const reason = row.reason;
    if (reason !== "checksum-invalid" && reason !== "schema-invalid" && reason !== "intent-invalid" && reason !== "carrier-unavailable") throw new Error("handover.lost.reason 非法");
    return { kind: "lost", reason };
  }
  exact(row, ["kind", "code", "residualBytes", "carriedResidualBytes", "thresholdEffective", "reloadCount", "pendingIntent", "last_committed_tick", "day", "handoverNonce"], "handover.frozen");
  if (row.kind !== "frozen" || row.code !== "MEMORY_RESIDUAL_RELOAD_HANDOVER") throw new Error("handover kind/code 非法");
  const bytes = (value: unknown, field: string) => {
    const result = integer(value, field);
    if (result > maximumBytes) throw new Error(`${field} 超过maximumBytes`);
    return result;
  };
  const residualBytes = bytes(row.residualBytes, "residualBytes");
  const carriedResidualBytes = bytes(row.carriedResidualBytes, "carriedResidualBytes");
  const thresholdEffective = bytes(row.thresholdEffective, "thresholdEffective");
  if (thresholdEffective === 0 || residualBytes < thresholdEffective) throw new Error("residualBytes 必须达到正thresholdEffective");
  if (carriedResidualBytes < residualBytes) throw new Error("carriedResidualBytes 不能小于residualBytes");
  if (typeof row.handoverNonce !== "string" || !/^[0-9a-f]{32}$/.test(row.handoverNonce)) throw new Error("handoverNonce 必须为128bit小写hex");
  return { kind: "frozen", code: "MEMORY_RESIDUAL_RELOAD_HANDOVER", residualBytes, carriedResidualBytes, thresholdEffective,
    reloadCount: integer(row.reloadCount, "reloadCount"), pendingIntent: pendingIntent(row.pendingIntent),
    last_committed_tick: integer(row.last_committed_tick, "last_committed_tick"), day: integer(row.day, "day"), handoverNonce: row.handoverNonce };
}
