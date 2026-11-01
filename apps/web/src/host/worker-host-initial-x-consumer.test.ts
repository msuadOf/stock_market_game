import assert from "node:assert/strict";
import test from "node:test";
import { parseVerifiedRestoreCapability } from "./wasm-restore-package-capability.ts";
import { fixture, drain } from "./worker-host-w1.test-support.ts";
import { commandDayEndArchiveFixture } from "../app/command-host-test-fixture.ts";
import { WasmDocumentMemoryOwner } from "./wasm-document-memory-owner.ts";
import { WasmMemoryStorage } from "./wasm-memory-storage.ts";
import { fixture as storageFixture, MiB } from "./wasm-memory-storage.test-support.ts";

// 这是可信 producer 接口的传输 fixture，不是实际发布包能力证据。
const capability = { kind: "verified-package", wasmSha256: "a".repeat(64), glueSha256: "b".repeat(64), sourceSha256: "c".repeat(64), buildManifestSha256: "d".repeat(64), exports: ["restore_json", "facade_slot_init_for_restore_json", "restore_prepared", "clear_prepared_restore"] };
test("有 verified package capability 时 bootstrap 唯一完整 X，ready 后仅 restoreInitial", { timeout: 10000 }, async () => {
  const f = fixture(); const storage = storageFixture();
  const runtimeCapability = { ...capability, exports: [...capability.exports, "set_step_runtime_override", "init_wasm_wait", "wasm_wait_abort_layout", "shutdown_session"] };
  const owner = new WasmDocumentMemoryOwner({ maximumBytes: storage.options.maximumBytes, reclaimBytes: storage.options.reclaimBytes,
    readCapacity: () => ({ kind: "enforced-capacity", initialPages: 1024, maximumPages: 4096, liveUpperBoundBytes: 256 * MiB }),
    restoreCapabilities: { forArchive: () => runtimeCapability as never },
    createStorage: (liveUpperBoundBytes) => new WasmMemoryStorage({ ...storage.options, liveUpperBoundBytes }) });
  const admission = owner.admit({ kind: "load", archive: commandDayEndArchiveFixture() });
  if (admission.kind !== "admitted" || admission.intent.kind !== "load") throw new Error("需要载入票");
  const archive = admission.intent.archive; const ticket = admission.ticket;
  try {
    f.launch({ onSpawnFailure() {}, initialArchive: archive, memoryAdmissionTicket: ticket });
    await drain();
    assert.equal(f.workers.length, 1, "VERIFIED_RESTORE_MUST_REACH_WORKER");
    const init = f.workers[0].sent.find((message) => message.type === "init");
    assert.ok(init);
    assert.equal(Object.hasOwn(init, "setup"), false); assert.deepEqual(init.initialArchive, archive); assert.deepEqual(init.restoreCapability, runtimeCapability);
    assert.deepEqual(f.workers[0].sent.find((message) => message.type === "restoreInitial"), { type: "restoreInitial" });
    assert.equal(f.workers[0].sent.some((message) => message.type === "create" || message.type === "restore"), false);
    f.workers[0].error(new Error("只验证启动传输，缺少真实 quiescence")); await drain();
    await assert.rejects(owner.finish(ticket), /quiescence|停止 grow/);
  } finally { await f.cleanup(); }
});

for (const scenario of ["missing", "wrong-intent", "missing-export", "bad-hash", "runtime-missing-export"] as const) {
  test(`初始完整 X ${scenario} 在 Worker/Memory 前显式拒绝并结 not-allocated`, { timeout: 10000 }, async () => {
    const f = fixture(); const archive = commandDayEndArchiveFixture();
    let release!: Promise<unknown>;
    try {
      const candidate = scenario === "missing" ? null : { ...capability, exports: scenario === "missing-export" ? ["restore_json"] : [...capability.exports], ...(scenario === "bad-hash" ? { wasmSha256: "假身份" } : {}) };
      const ticket = { intent: scenario === "wrong-intent" ? { kind: "new" } : { kind: "load", archive }, restoreCapability: candidate, registerRelease(value: Promise<unknown>) { release = value; } };
      await assert.rejects(f.launch({ onSpawnFailure() {}, initialArchive: archive, memoryAdmissionTicket: ticket, ...(scenario === "runtime-missing-export" ? { runtimeOverride: true, waitSettings: { spinUs: 1, quietUs: 1, fineUs: 1, coarseUs: 1 } } : {}) }));
      assert.equal(f.workers.length, 0); assert.equal(f.channels.length, 0);
      assert.deepEqual(await release, { kind: "not-allocated" });
    } finally { await f.cleanup(); }
  });
}

for (const contradictory of [false, true]) {
  test(`主侧 pre-Memory 未分配证明 ${contradictory ? "与 early Memory 矛盾必须锁票" : "有实际关闭阶段才可结 not-allocated"}`, { timeout: 10000 }, async () => {
    const f = fixture(contradictory ? { initAfterMemoryFailure: "矛盾 fixture", claimUnallocatedAfterMemory: true } : { initBeforeMemoryFailure: "真实 pre-Memory fixture" });
    let release!: Promise<unknown>;
    try {
      const creating = f.launch({ onSpawnFailure() {}, memoryAdmissionTicket: { registerRelease(value: Promise<unknown>) { release = value; } } });
      const settled = contradictory ? assert.rejects(release, /quiescence|停止 grow/) : release.then((result) => { assert.deepEqual(result, { kind: "not-allocated" }); });
      await assert.rejects(creating); await settled;
      assert.equal(f.workers[0].terminations, 1);
    } finally { await f.cleanup(); }
  });
}

test("restore capability 保留 omitted/false/true 三态，与 initializer requested 语义一致", { timeout: 10000 }, () => {
  const withSetter = { ...capability, exports: [...capability.exports, "set_step_runtime_override"] };
  const withRuntime = { ...withSetter, exports: [...withSetter.exports, "init_wasm_wait", "wasm_wait_abort_layout", "shutdown_session"] };
  assert.deepEqual(parseVerifiedRestoreCapability(capability), capability);
  assert.throws(() => parseVerifiedRestoreCapability(capability, false), /缺少/);
  assert.deepEqual(parseVerifiedRestoreCapability(withSetter, false), withSetter);
  assert.throws(() => parseVerifiedRestoreCapability(withSetter, true), /缺少/);
  assert.deepEqual(parseVerifiedRestoreCapability(withRuntime, true), withRuntime);
});

for (const runtimeOverride of [false, true]) test(`显式 runtimeOverride:${runtimeOverride} ${runtimeOverride ? "缺 setter" : "不受支持"}时零 Worker/Channel 拒绝并结 not-allocated`, { timeout: 10000 }, async () => {
  const f = fixture(); const storage = storageFixture();
  const owner = new WasmDocumentMemoryOwner({ maximumBytes: storage.options.maximumBytes, reclaimBytes: storage.options.reclaimBytes,
    readCapacity: () => ({ kind: "enforced-capacity", initialPages: 1024, maximumPages: 4096, liveUpperBoundBytes: 256 * MiB }),
    restoreCapabilities: { forArchive: () => capability as never },
    createStorage: (liveUpperBoundBytes) => new WasmMemoryStorage({ ...storage.options, liveUpperBoundBytes }) });
  const admission = owner.admit({ kind: "load", archive: commandDayEndArchiveFixture() });
  if (admission.kind !== "admitted" || admission.intent.kind !== "load") throw new Error("需要载入票");
  try {
    await assert.rejects(f.launch({ onSpawnFailure() {}, initialArchive: admission.intent.archive, runtimeOverride, ...(runtimeOverride ? { waitSettings: { spinUs: 1, quietUs: 1, fineUs: 1, coarseUs: 1 } } : {}), memoryAdmissionTicket: admission.ticket }), runtimeOverride ? /缺少初始恢复所需导出/ : /不支持 runtimeOverride: false/);
    assert.equal(f.workers.length, 0); assert.equal(f.channels.length, 0);
    await owner.finish(admission.ticket);
  } finally { await f.cleanup(); }
});

test("初始化进度回调抛错时仍结束 admission release", { timeout: 10000 }, async () => {
  const f = fixture(); let release!: Promise<unknown>;
  try {
    const result = f.launch({ onSpawnFailure() {}, onInitializationProgress() { throw new Error("进度回调故障"); },
      memoryAdmissionTicket: { registerRelease(value: Promise<unknown>) { release = value; } } });
    const releaseFailure = assert.rejects(release, /quiescence|停止 grow|unknown/);
    await assert.rejects(result, /进度回调故障/);
    await drain(); f.clock.advance(2000); await drain();
    await releaseFailure;
  } finally { await f.cleanup(); }
});
