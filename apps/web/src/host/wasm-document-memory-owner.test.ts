import assert from "node:assert/strict";
import test from "node:test";
import { WasmDocumentMemoryOwner, getDocumentWasmMemoryOwner } from "./wasm-document-memory-owner.ts";
import type { WasmFinalRelease, WasmMemoryAdmissionTicket } from "./wasm-document-memory-owner.ts";
import { WasmMemoryStorage } from "./wasm-memory-storage.ts";
import { fixture, MiB, SNAPSHOT_KEY } from "./wasm-memory-storage.test-support.ts";
import { DEFAULT_SETUP } from "../config/defaults.ts";
import { commandDayEndArchiveFixture } from "../app/command-host-test-fixture.ts";

function deferred<Value>() {
  let resolve!: (value: Value) => void, reject!: (error: unknown) => void;
  const promise = new Promise<Value>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
const verified = { kind: "verified-package" as const, wasmSha256: "a".repeat(64), glueSha256: "b".repeat(64), sourceSha256: "c".repeat(64), buildManifestSha256: "d".repeat(64), exports: ["restore_json", "facade_slot_init_for_restore_json", "restore_prepared", "clear_prepared_restore"] };
const intent = () => ({ kind: "new" as const, setup: structuredClone(DEFAULT_SETUP), seed: "42" });
function setup(globalMax: number | null = 256 * MiB) {
  const f = fixture(); let opened = 0;
  const readCapacity = () => globalMax === null ? null : ({ kind: "enforced-capacity" as const, initialPages: 1024, maximumPages: globalMax / 65536, liveUpperBoundBytes: globalMax });
  const restoreCapabilities = { forArchive: () => verified };
  const owner = new WasmDocumentMemoryOwner({ maximumBytes: f.options.maximumBytes, reclaimBytes: f.options.reclaimBytes,
    readCapacity, restoreCapabilities, createStorage: (liveUpperBoundBytes) => { opened++; return new WasmMemoryStorage({ ...f.options, liveUpperBoundBytes }); },
    randomNonce: () => "1234567890abcdef1234567890abcdef" });
  const anchor = () => JSON.parse(f.session.data.get(SNAPSHOT_KEY)!);
  const admit = () => { const result = owner.admit(intent()); assert.equal(result.kind, "admitted"); if (result.kind !== "admitted") throw new Error("预期admitted"); return result.ticket; };
  return { f, owner, anchor, admit, opened: () => opened, readCapacity, restoreCapabilities, setGlobal(value: number | null) { globalMax = value; } };
}
function released(finalBytes: number): WasmFinalRelease {
  return { kind: "released", finalBytes, growthStopped: true, referencesDropped: true };
}

test("document对象唯一owner且Storage open一次、fork跨宿主稳定", { timeout: 10000 }, async () => {
  const s = setup(), documentKey = {};
  assert.equal(getDocumentWasmMemoryOwner(documentKey, () => s.owner), s.owner);
  assert.equal(getDocumentWasmMemoryOwner(documentKey, () => { throw new Error("禁止第二owner"); }), s.owner);
  const first = s.admit(), fork = s.owner.forkId;
  assert.equal(s.anchor().liveUpperBound, 256 * MiB);
  first.registerRelease(Promise.resolve(released(64 * MiB))); await s.owner.finish(first);
  assert.equal(s.anchor().liveUpperBound, 0); assert.equal(s.anchor().sessionResident, 64 * MiB);
  const second = s.admit(); second.registerRelease(Promise.resolve({ kind: "not-allocated" })); await s.owner.finish(second);
  assert.equal(s.opened(), 1); assert.equal(s.owner.forkId, fork);
});

test("未知构造上界不构造Storage、不写admission、不分配，可信capacity后到可重试", { timeout: 10000 }, () => {
  const s = setup(null);
  assert.throws(() => s.owner.admit(intent()), (error: unknown) => (error as { code: string }).code === "MEMORY_MEASUREMENT_NOT_READY");
  assert.equal(s.opened(), 0); assert.equal(s.f.session.calls.length, 0);
  s.setGlobal(256 * MiB); assert.equal(s.owner.admit(intent()).kind, "admitted");
});

test("receipt未完成时保留live锚，禁止下一票；最终bytes恰一次计入drop锚", { timeout: 10000 }, async () => {
  const s = setup(), completion = deferred<WasmFinalRelease>(), ticket = s.admit();
  ticket.registerRelease(completion.promise);
  assert.throws(() => ticket.registerRelease(completion.promise), /一次/);
  const finishing = s.owner.finish(ticket); assert.equal(s.anchor().liveUpperBound, 256 * MiB);
  assert.throws(() => s.owner.admit(intent()), /上一/);
  completion.resolve(released(128 * MiB)); await finishing; await s.owner.finish(ticket);
  assert.equal(s.anchor().sessionResident, 128 * MiB); assert.equal(s.anchor().liveUpperBound, 0);
  s.admit(); assert.equal(s.anchor().sessionResident, 128 * MiB); assert.equal(s.anchor().liveUpperBound, 256 * MiB);
});

test("create失败无release注册或receipt拒绝，保live锚与原cause并锁存", { timeout: 10000 }, async () => {
  const missing = setup(), ticket = missing.admit();
  await assert.rejects(missing.owner.finish(ticket), /登记/); assert.equal(missing.anchor().liveUpperBound, 256 * MiB);
  assert.throws(() => missing.owner.admit(intent()), /登记/);
  const s = setup(), failedTicket = s.admit(), cause = new Error("Memory未知，无法确认终值");
  failedTicket.registerRelease(Promise.reject(cause));
  await assert.rejects(s.owner.finish(failedTicket), (error) => error === cause);
  assert.equal(s.anchor().liveUpperBound, 256 * MiB);
  assert.throws(() => s.owner.admit(intent()), (error) => error === cause);
});

test("drop后才判阈值，冻结完整X；reload后未确认不消费、不换quickslot", { timeout: 10000 }, async () => {
  const s = setup(), archive = commandDayEndArchiveFixture();
  for (let index = 0; index < 2; index++) {
    const t = s.admit(); s.owner.recordCommittedBoundary(t, { tick: 7, day: 3 });
    t.registerRelease(Promise.resolve(released(256 * MiB))); await s.owner.finish(t);
  }
  const result = s.owner.admit({ kind: "load", archive });
  assert.equal(result.kind, "reload"); assert.equal(s.anchor().liveUpperBound, 0);
  assert.equal(s.anchor().handover.pendingIntent.archive.seed, archive.seed);
  const restored = new WasmDocumentMemoryOwner({ maximumBytes: s.f.options.maximumBytes, reclaimBytes: s.f.options.reclaimBytes,
    readCapacity: s.readCapacity, restoreCapabilities: s.restoreCapabilities, createStorage: (liveUpperBoundBytes) => new WasmMemoryStorage({ ...s.f.options, liveUpperBoundBytes }) });
  const pending = restored.pendingHandover(); assert.equal(pending?.kind, "frozen");
  assert.throws(() => restored.admit(intent()), (error: unknown) => (error as { code: string }).code === "MEMORY_HANDOVER_CONFIRMATION_REQUIRED");
  assert.equal(s.anchor().liveUpperBound, 0); assert.equal(s.anchor().handover.pendingIntent.archive.seed, archive.seed);
  const admitted = restored.admit({ kind: "load", archive }, { confirmedHandover: true });
  assert.equal(admitted.kind, "admitted"); assert.equal(s.anchor().handover, undefined); assert.equal(s.anchor().liveUpperBound, 256 * MiB);
});

test("pending消费主写失败即使ledger保账成功仍不发票；修复前件重试同X", { timeout: 10000 }, async () => {
  const s = setup(), archive = commandDayEndArchiveFixture();
  for (let index = 0; index < 2; index++) {
    const t = s.admit(); s.owner.recordCommittedBoundary(t, { tick: 8, day: 2 });
    t.registerRelease(Promise.resolve(released(256 * MiB))); await s.owner.finish(t);
  }
  s.owner.admit({ kind: "load", archive });
  const next = new WasmDocumentMemoryOwner({ maximumBytes: s.f.options.maximumBytes, reclaimBytes: s.f.options.reclaimBytes,
    readCapacity: s.readCapacity, restoreCapabilities: s.restoreCapabilities, createStorage: (liveUpperBoundBytes) => new WasmMemoryStorage({ ...s.f.options, liveUpperBoundBytes }) });
  next.pendingHandover();
  s.f.session.onCall((call) => { if (call.operation === "set" && call.key === SNAPSHOT_KEY) throw new Error("主通道拒绝"); });
  assert.throws(() => next.admit({ kind: "load", archive }, { confirmedHandover: true }), (error: unknown) => (error as { context: { context: string } }).context.context === "consume");
  assert.equal(s.anchor().handover.pendingIntent.archive.seed, archive.seed);
  s.f.session.onCall(null);
  assert.equal(next.admit({ kind: "load", archive }, { confirmedHandover: true }).kind, "admitted");
});

test("真实final超过构造上界或总预算时锁存，不能将current长度冒作下一局上界", { timeout: 10000 }, async () => {
  const s = setup(), ticket: WasmMemoryAdmissionTicket = s.admit();
  ticket.registerRelease(Promise.resolve(released(320 * MiB)));
  await assert.rejects(s.owner.finish(ticket), (error: unknown) => (error as { code: string }).code === "MEMORY_CAPACITY_BOUND_EXCEEDED");
  assert.equal(s.anchor().sessionResident, 320 * MiB); assert.equal(s.anchor().liveUpperBound, 0);
  assert.throws(() => s.owner.admit(intent()), (error: unknown) => (error as { code: string }).code === "MEMORY_CAPACITY_BOUND_EXCEEDED");
});

test("零分配证明不触发实际drop顶点终局，仍保继承账与后继admission", { timeout: 10000 }, async () => {
  const s = setup(); s.f.existing((await import("./wasm-memory-storage.test-support.ts")).snapshot(1792 * MiB));
  const ticket = s.admit(); ticket.registerRelease(Promise.resolve({ kind: "not-allocated" }));
  await s.owner.finish(ticket); assert.equal(s.anchor().sessionResident, 0);
  assert.equal(s.owner.admit(intent()).kind, "admitted");
});

test("真实终值突破总预算仍落最坏maximum锚，F5不能继承旧小账后续开局", { timeout: 10000 }, async () => {
  const s = setup(); s.f.existing((await import("./wasm-memory-storage.test-support.ts")).snapshot(1792 * MiB));
  const ticket = s.admit(); ticket.registerRelease(Promise.resolve(released(512 * MiB)));
  await assert.rejects(s.owner.finish(ticket), (error: unknown) => (error as { code: string }).code === "MEMORY_BUDGET_EXCEEDED");
  const root = s.anchor(); assert.equal(root.liveUpperBound, 0);
  assert.equal(root.carryBase + root.sessionResident, s.f.options.maximumBytes);
  const next = new WasmDocumentMemoryOwner({ maximumBytes: s.f.options.maximumBytes, reclaimBytes: s.f.options.reclaimBytes,
    readCapacity: s.readCapacity, restoreCapabilities: s.restoreCapabilities, createStorage: (liveUpperBoundBytes) => new WasmMemoryStorage({ ...s.f.options, liveUpperBoundBytes }) });
  assert.throws(() => next.admit(intent()), (error: unknown) => (error as { code: string }).code === "MEMORY_RESIDUAL_EXHAUSTED");
});

test("可信1GiB构造capacity无samples即可ordinary READY，活动F5一次仍准入、两次耗尽", { timeout: 10000 }, () => {
  const s = setup(1024 * MiB), first = s.owner.admit(intent());
  assert.equal(first.kind, "admitted"); if (first.kind !== "admitted") throw new Error("预期admitted");
  assert.deepEqual(first.ticket.selection, { kind: "enforced-capacity", initialPages: 1024, maximumPages: 16384, liveUpperBoundBytes: 1024 * MiB });
  const nextOwner = () => new WasmDocumentMemoryOwner({ maximumBytes: s.f.options.maximumBytes, reclaimBytes: s.f.options.reclaimBytes,
    readCapacity: s.readCapacity, createStorage: (liveUpperBoundBytes) => new WasmMemoryStorage({ ...s.f.options, liveUpperBoundBytes }) });
  const second = nextOwner(); assert.equal(second.admit(intent()).kind, "admitted");
  assert.equal(s.anchor().carryBase, 1024 * MiB); assert.equal(s.anchor().liveUpperBound, 1024 * MiB);
  assert.throws(() => nextOwner().admit(intent()), (error: unknown) => (error as { code: string }).code === "MEMORY_RESIDUAL_EXHAUSTED");
});

test("JS面真实totalJSHeapSize超过256MiB在admission锚前终局，未知noise不冒填", { timeout: 10000 }, () => {
  const f = fixture();
  const owner = new WasmDocumentMemoryOwner({ maximumBytes: f.options.maximumBytes, reclaimBytes: f.options.reclaimBytes,
    readCapacity: () => ({ kind: "enforced-capacity", initialPages: 1024, maximumPages: 4096, liveUpperBoundBytes: 256 * MiB }),
    observeJsHeap: () => ({ kind: "observed", source: "performance.memory.totalJSHeapSize", bytes: 256 * MiB + 1 }),
    createStorage: (liveUpperBoundBytes) => new WasmMemoryStorage({ ...f.options, liveUpperBoundBytes }) });
  assert.throws(() => owner.admit(intent()), (error: unknown) => (error as { code: string }).code === "MEMORY_BUDGET_EXCEEDED");
  assert.equal(JSON.parse(f.session.data.get(SNAPSHOT_KEY)!).liveUpperBound, 0);
  assert.throws(() => owner.admit(intent()), (error: unknown) => (error as { code: string }).code === "MEMORY_BUDGET_EXCEEDED");
});

test("JS不可观测登记能力位而不伪造0bytes，getter失败保持typed原因不准入", { timeout: 10000 }, () => {
  for (const unavailable of [true, false]) {
    const f = fixture(), cause = new Error("JS getter拒绝");
    const owner = new WasmDocumentMemoryOwner({ maximumBytes: f.options.maximumBytes, reclaimBytes: f.options.reclaimBytes,
      readCapacity: () => ({ kind: "enforced-capacity", initialPages: 1024, maximumPages: 4096, liveUpperBoundBytes: 256 * MiB }),
      observeJsHeap: () => { if (!unavailable) throw cause; return { kind: "unobservable", capability: "js_face_unobservable", reason: "浏览器未提供" }; },
      createStorage: (liveUpperBoundBytes) => new WasmMemoryStorage({ ...f.options, liveUpperBoundBytes }) });
    if (unavailable) {
      assert.equal(owner.admit(intent()).kind, "admitted"); assert.deepEqual(owner.latestJsObservation(), { kind: "unobservable", capability: "js_face_unobservable", reason: "浏览器未提供" });
    } else assert.throws(() => owner.admit(intent()), (error: unknown) => (error as { cause: unknown }).cause === cause);
  }
});

test("freeze主载荷写失败后只重试同一冻结事件，不换X/nonce/count或重复计费", { timeout: 10000 }, async () => {
  const s = setup(), x = commandDayEndArchiveFixture();
  for (let index = 0; index < 2; index++) {
    const ticket = s.admit(); s.owner.recordCommittedBoundary(ticket, { tick: 4, day: 1 });
    ticket.registerRelease(Promise.resolve(released(256 * MiB))); await s.owner.finish(ticket);
  }
  s.f.session.onCall((call) => { if (call.operation === "set" && call.key === SNAPSHOT_KEY) throw new Error("主载荷写失败"); });
  assert.throws(() => s.owner.admit({ kind: "load", archive: x }), (error: unknown) => { const failure = error as { code: string; context: { channel: string; context: string } }; return failure.code === "MEMORY_HANDOVER_WRITE_FAILED" && failure.context.channel === "session" && failure.context.context === "freeze"; });
  s.f.session.onCall(null);
  assert.throws(() => s.owner.admit({ kind: "new", setup: DEFAULT_SETUP, seed: "99" }), (error: unknown) => (error as { code: string }).code === "MEMORY_RELOAD_RETRY_CONFIRMATION_REQUIRED");
  const result = s.owner.retryReloadHandover();
  assert.equal(result.kind, "reload"); if (result.kind !== "reload") throw new Error("预期reload");
  assert.equal(result.handover.kind, "frozen");
  if (result.handover.kind !== "frozen") throw new Error("预期frozen");
  assert.deepEqual(result.handover.pendingIntent, { kind: "load", archive: x });
  assert.equal(result.handover.handoverNonce, "1234567890abcdef1234567890abcdef"); assert.equal(result.handover.reloadCount, 1);
  assert.equal(s.anchor().sessionResident, 512 * MiB); assert.equal(s.anchor().liveUpperBound, 0);
});

test("freeze的ledger准备读失败锁存同一原错误，不随恢复读取而自动重试", { timeout: 10000 }, async () => {
  const s = setup();
  for (let index = 0; index < 2; index++) {
    const ticket = s.admit(); s.owner.recordCommittedBoundary(ticket, { tick: 4, day: 1 });
    ticket.registerRelease(Promise.resolve(released(256 * MiB))); await s.owner.finish(ticket);
  }
  s.f.ledger.onCall((call) => { if (call.operation === "get") throw new Error("ledger读失败"); });
  let original: unknown;
  assert.throws(() => s.owner.admit(intent()), (error: unknown) => { original = error; assert.equal((error as { context: { stage: string } }).context.stage, "prepare"); return true; });
  s.f.ledger.onCall(null);
  assert.throws(() => s.owner.admit(intent()), (error: unknown) => error === original);
  assert.throws(() => s.owner.retryReloadHandover(), (error: unknown) => error === original);
});

test("需要用户确认的同document重试错误明确recoverable", { timeout: 10000 }, async () => {
  const { WasmDocumentMemoryError } = await import("./wasm-document-memory-owner.ts");
  assert.equal(new WasmDocumentMemoryError("MEMORY_RELOAD_RETRY_CONFIRMATION_REQUIRED", "请确认").recoverable, true);
});
