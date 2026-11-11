import type { VerifiedWasmRestoreCapability } from "./wasm-document-memory-owner.ts";

export type WasmRestoreMode = "string" | "chunked";
export const CHUNKED_RESTORE_EXPORTS = Object.freeze([
  "facade_restore_chunks_begin", "facade_restore_chunks_push", "facade_restore_chunks_finish",
  "facade_restore_chunks_clear", "restore_prepared_chunks",
] as const);

/** metadata 是实际 build producer 的来源事实；此 parser 不把字段标签冒作实际 ABI 实测。 */
export function parseVerifiedRestoreCapability(value: unknown, runtimeOverride: unknown = undefined, mode: WasmRestoreMode = "string"): VerifiedWasmRestoreCapability {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("restore package capability 必须为对象");
  if (runtimeOverride !== undefined && typeof runtimeOverride !== "boolean") throw new Error("runtimeOverride 必须为 boolean 或未请求");
  if (mode !== "string" && mode !== "chunked") throw new Error("restore mode 必须为 string 或 chunked");
  const copy = structuredClone(value) as Record<string, unknown>;
  if (Object.keys(copy).sort().join(",") !== "buildManifestSha256,exports,glueSha256,kind,sourceSha256,wasmSha256" || copy.kind !== "verified-package"
    || ![copy.wasmSha256, copy.glueSha256, copy.sourceSha256, copy.buildManifestSha256].every((hash) => typeof hash === "string" && /^[a-f0-9]{64}$/.test(hash))
    || !Array.isArray(copy.exports) || copy.exports.some((name) => typeof name !== "string" || name.length === 0) || new Set(copy.exports).size !== copy.exports.length) throw new Error("restore package capability 身份字段无效");
  const exports = copy.exports as string[];
  const required = ["restore_json", "facade_slot_init_for_restore_json", "restore_prepared", "clear_prepared_restore", "wasm_tombstone_init_primary", "wasm_tombstone_bind_ingress",
    ...(runtimeOverride === undefined ? [] : ["set_step_runtime_override"]),
    ...(runtimeOverride === true ? ["shutdown_session", "libos_host_fault_layout"] : []),
    ...(mode === "chunked" ? CHUNKED_RESTORE_EXPORTS : [])];
  if (required.some((name) => !exports.includes(name))) throw Object.assign(new Error("实际 WASM package capability 缺少初始恢复所需导出"), { code: "WASM_RESTORE_TOPOLOGY_NOT_READY", where: "wasm-restore-package-capability", context: { operation: "restore" } });
  return Object.freeze({ ...copy, exports: Object.freeze(exports) }) as unknown as VerifiedWasmRestoreCapability;
}
