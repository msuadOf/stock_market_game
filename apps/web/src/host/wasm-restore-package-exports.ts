import { parseVerifiedRestoreCapability } from "./wasm-restore-package-capability.ts";
import type { WasmRestoreMode } from "./wasm-restore-package-capability.ts";

/** 在相同 package 的 WASM 导出表与当前 glue 上复验所需能力。 */
export function verifyRestorePackageExports(value: unknown, wasmExports: ReadonlySet<string>, glue: Record<string, unknown>, mode: WasmRestoreMode = "string"): void {
  const capability = parseVerifiedRestoreCapability(value, undefined, mode);
  for (const name of capability.exports) {
    if (!wasmExports.has(name) || typeof glue[name] !== "function") throw Object.assign(new Error(`实际 package 缺少 verified 导出 ${name}，尚未分配 Memory`), { code: "WASM_RESTORE_TOPOLOGY_NOT_READY", where: "wasm-worker.restore-package", context: { operation: "restore", missing: name } });
  }
}
