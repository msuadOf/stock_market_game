import assert from "node:assert/strict";
import test from "node:test";
import { parseVerifiedRestoreCapability, CHUNKED_RESTORE_EXPORTS } from "./wasm-restore-package-capability.ts";
import { verifyRestorePackageExports } from "./wasm-restore-package-exports.ts";
import { validateOwnerRestoreCapability } from "./wasm-document-memory-owner.ts";

const base = ["restore_json", "facade_slot_init_for_restore_json", "restore_prepared", "clear_prepared_restore", "wasm_tombstone_init_primary", "wasm_tombstone_bind_ingress"];
const capability = (exports: string[]) => ({ kind: "verified-package", wasmSha256: "a".repeat(64), glueSha256: "b".repeat(64), sourceSha256: "c".repeat(64), buildManifestSha256: "d".repeat(64), exports });

test("runtime 恢复预检拒绝缺少正式 LibOS 故障域的包", {timeout:10000}, () => {
  const runtime = [...base, "set_step_runtime_override", "libos_host_fault_layout", "shutdown_session"];
  assert.throws(() => parseVerifiedRestoreCapability(capability(runtime), true), /缺少/);
  assert.doesNotThrow(() => parseVerifiedRestoreCapability(capability([...runtime, "libos_host_fault_layout"]), true));
});

test("String 包兼容旧模式，但不能冒充分块模式", { timeout: 10000 }, () => {
  assert.doesNotThrow(() => parseVerifiedRestoreCapability(capability(base), undefined, "string"));
  assert.throws(() => parseVerifiedRestoreCapability(capability(base), undefined, "chunked"), /缺少/);
  for (const name of CHUNKED_RESTORE_EXPORTS) {
    const exports = [...base, ...CHUNKED_RESTORE_EXPORTS.filter(item => item !== name)];
    assert.throws(() => parseVerifiedRestoreCapability(capability(exports), undefined, "chunked"), /缺少/);
  }
  assert.doesNotThrow(() => parseVerifiedRestoreCapability(capability([...base, ...CHUNKED_RESTORE_EXPORTS]), undefined, "chunked"));
});

test("worker 分块模式逐项核对实际 WASM 与 glue 导出", { timeout: 10000 }, () => {
  const names = [...base, ...CHUNKED_RESTORE_EXPORTS]; const cap = parseVerifiedRestoreCapability(capability(names), undefined, "chunked");
  const glue = Object.fromEntries(names.map(name => [name, () => {}]));
  verifyRestorePackageExports(cap, new Set(names), glue, "chunked");
  for (const name of CHUNKED_RESTORE_EXPORTS) {
    assert.throws(() => verifyRestorePackageExports(cap, new Set(names.filter(item => item !== name)), glue, "chunked"), /缺少/);
    assert.throws(() => verifyRestorePackageExports(cap, new Set(names), { ...glue, [name]: undefined }, "chunked"), /缺少/);
  }
  assert.throws(() => verifyRestorePackageExports(capability(base), new Set(base), glue, "chunked"), /缺少/);
});

test("owner 按显式模式验证能力且冻结身份", { timeout: 10000 }, () => {
  const old = capability(base);
  assert.doesNotThrow(() => validateOwnerRestoreCapability(old as never, "string"));
  assert.throws(() => validateOwnerRestoreCapability(old as never, "chunked"), /chunked/);
  for (const name of CHUNKED_RESTORE_EXPORTS) {
    assert.throws(() => validateOwnerRestoreCapability(capability([...base, ...CHUNKED_RESTORE_EXPORTS.filter(item => item !== name)]) as never, "chunked"));
  }
  const value = validateOwnerRestoreCapability(capability([...base, ...CHUNKED_RESTORE_EXPORTS]) as never, "chunked");
  assert.ok(Object.isFrozen(value)); assert.ok(Object.isFrozen(value.exports));
});
