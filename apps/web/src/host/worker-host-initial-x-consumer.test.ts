import assert from "node:assert/strict";
import test from "node:test";
import { parseVerifiedRestoreCapability } from "./wasm-restore-package-capability.ts";
import { fixture, drain } from "./worker-host-w1.test-support.ts";
import { commandDayEndArchiveFixture } from "../app/command-host-test-fixture.ts";

// 这是可信 producer 接口的传输 fixture，不是实际发布包能力证据。
const capability = { kind: "verified-package", wasmSha256: "a".repeat(64), glueSha256: "b".repeat(64), sourceSha256: "c".repeat(64), buildManifestSha256: "d".repeat(64), exports: ["restore_json", "facade_slot_init_for_restore_json"] };
test("有 verified package capability 时 bootstrap 唯一完整 X，ready 后仅 restoreInitial", { timeout: 10000 }, async () => {
  const f = fixture(); const archive = commandDayEndArchiveFixture();
  try {
    let release!: Promise<unknown>;
    const ticket = { intent: { kind: "load", archive }, restoreCapability: capability, registerRelease(value: Promise<unknown>) { release = value; } };
    f.launch({ onSpawnFailure() {}, initialArchive: archive, memoryAdmissionTicket: ticket });
    await drain();
    assert.equal(f.workers.length, 1, "VERIFIED_RESTORE_MUST_REACH_WORKER");
    const init = f.workers[0].sent[0];
    assert.equal(Object.hasOwn(init, "setup"), false); assert.deepEqual(init.initialArchive, archive); assert.deepEqual(init.restoreCapability, capability);
    assert.deepEqual(f.workers[0].sent.find((message) => message.type === "restoreInitial"), { type: "restoreInitial" });
    assert.equal(f.workers[0].sent.some((message) => message.type === "create" || message.type === "restore"), false);
    const releaseFailure = assert.rejects(release, /quiescence|停止 grow/);
    f.workers[0].emit({ type: "failure", code: "FIXTURE_CLEANUP", where: "fixture", message: "只验证启动传输" }); await drain(); await releaseFailure;
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

test("显式 runtimeOverride:false 缺 setter 时零 Worker/Channel 拒绝并结 not-allocated", { timeout: 10000 }, async () => {
  const f = fixture(); const archive = commandDayEndArchiveFixture();
  let observedWorkers = -1, observedChannels = -1;
  let release!: Promise<{ kind: "receipt"; receipt: unknown } | { kind: "error"; error: unknown }>;
  let outcome: Awaited<typeof release> | undefined;
  try {
    const creating = f.launch({ onSpawnFailure() {}, initialArchive: archive, runtimeOverride: false, memoryAdmissionTicket: {
      intent: { kind: "load", archive }, restoreCapability: capability,
      registerRelease(value: Promise<unknown>) { release = value.then((receipt) => ({ kind: "receipt", receipt }), (error: unknown) => ({ kind: "error", error })); },
    } });
    const rejected = assert.rejects(creating);
    await drain(); observedWorkers = f.workers.length; observedChannels = f.channels.length;
    // 红测也明确收尾，让错误准入表现为业务断言失败而非等到硬 deadline。
    if (f.workers[0] !== undefined) f.workers[0].emit({ type: "failure", code: "RUNTIME_FALSE_FIXTURE_CLEANUP", where: "fixture", message: "错误准入的测试收尾" });
    await rejected; outcome = await release;
  } finally { await f.cleanup(); }
  assert.equal(observedWorkers, 0, "显式 false 缺少 setter 不得创建 Worker");
  assert.equal(observedChannels, 0);
  assert.deepEqual(outcome, { kind: "receipt", receipt: { kind: "not-allocated" } });
});
