import assert from "node:assert/strict";
import test from "node:test";
import { parseVerifiedRestoreCapability } from "./wasm-restore-package-capability.ts";

test("restore capability 强制共享 replacement ABI，不接受仅能创建第二会话的旧包", { timeout: 10000 }, () => {
  const value = { kind: "verified-package", wasmSha256: "a".repeat(64), glueSha256: "b".repeat(64), sourceSha256: "c".repeat(64), buildManifestSha256: "d".repeat(64), exports: ["restore_json", "facade_slot_init_for_restore_json", "restore_prepared", "clear_prepared_restore", "wasm_tombstone_init_primary", "wasm_tombstone_bind_ingress", "start_wasm_reclaimer", "close_wasm_reclaimer"] };
  assert.throws(() => parseVerifiedRestoreCapability(value), /commit_session_replacement/);
  assert.equal(parseVerifiedRestoreCapability({ ...value, exports: [...value.exports, "commit_session_replacement", "prepare_session_replacement", "fence_session_replacement"] }).kind, "verified-package");
});
