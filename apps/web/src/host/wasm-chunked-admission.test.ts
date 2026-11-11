import assert from "node:assert/strict";
import test from "node:test";
import { WasmDocumentMemoryOwner, requireAdmittedChunkedLease, configureDocumentWasmRestoreCapabilities, getDocumentWasmMemoryOwner } from "./wasm-document-memory-owner.ts";
import { WasmMemoryStorage } from "./wasm-memory-storage.ts";
import { fixture, MiB, SNAPSHOT_KEY } from "./wasm-memory-storage.test-support.ts";
import { captureGzipArchiveSource } from "../save/archive-source.ts";
import { createValidatedArchiveLease } from "../save/validated-archive-lease.ts";
import { gzipSaveCodec } from "../save/save-repository.ts";
import { currentSaveFixture, withSettledDay } from "../save/current-save-fixture.ts";
import { CHUNKED_RESTORE_EXPORTS } from "./wasm-restore-package-capability.ts";
import { createChunkedHandoverSources } from "../save/chunked-handover-source.ts";

const base = ["restore_json", "facade_slot_init_for_restore_json", "restore_prepared", "clear_prepared_restore", "wasm_tombstone_init_primary", "wasm_tombstone_bind_ingress"];
const cap = (names: readonly string[]) => ({ kind: "verified-package" as const, wasmSha256: "a".repeat(64), glueSha256: "b".repeat(64), sourceSha256: "c".repeat(64), buildManifestSha256: "d".repeat(64), exports: [...base, ...names] });
async function lease() { const text = JSON.stringify(withSettledDay(currentSaveFixture(), "2030-01-01", "2030-01-02")); return createValidatedArchiveLease(captureGzipArchiveSource(`gzip:${await gzipSaveCodec.encode(text)}`)); }
function create(names: readonly string[] = CHUNKED_RESTORE_EXPORTS, jsBytes = 0) {
  const f = fixture(); const owner = new WasmDocumentMemoryOwner({ maximumBytes: f.options.maximumBytes, reclaimBytes: f.options.reclaimBytes,
    readCapacity: () => ({ kind: "enforced-capacity", initialPages: 1024, maximumPages: 4096, liveUpperBoundBytes: 256 * MiB }),
    restoreCapabilities: { forArchive: () => cap(names), forLease: () => cap(names) },
    observeJsHeap: () => ({ kind: "observed", source: "performance.memory.totalJSHeapSize", bytes: jsBytes }),
    createStorage: liveUpperBoundBytes => new WasmMemoryStorage({ ...f.options, liveUpperBoundBytes }) });
  return { owner, f, anchor: () => JSON.parse(f.session.data.get(SNAPSHOT_KEY)!) };
}

test("chunked 票据绑定私有租约、能力和注册后有效的生命周期", { timeout: 10000 }, async () => {
  const a = await lease(), b = await lease(), s = create();
  const result = s.owner.admitChunked(a); const ticket = result.ticket;
  assert.equal(ticket.restoreMode, "chunked"); assert.equal(ticket.productionAdmission, false);
  assert.throws(() => requireAdmittedChunkedLease(ticket, a), /release/);
  ticket.registerRelease(Promise.resolve({ kind: "not-allocated" }));
  assert.equal(requireAdmittedChunkedLease(ticket, a), a);
  assert.throws(() => requireAdmittedChunkedLease(ticket, b), /不一致/);
  assert.throws(() => requireAdmittedChunkedLease({ ...ticket }, a), /owner/);
  await s.owner.finish(ticket); assert.throws(() => requireAdmittedChunkedLease(ticket, a), /失效/);
});

test("伪造租约或 chunk 导出缺失不能写 admission 锚", { timeout: 10000 }, async () => {
  const a = await lease(); const forged = structuredClone(a);
  const one = create(); assert.throws(() => one.owner.admitChunked(forged), /租约身份/);
  for (const missing of CHUNKED_RESTORE_EXPORTS) {
    const s = create(CHUNKED_RESTORE_EXPORTS.filter(name => name !== missing));
    assert.throws(() => s.owner.admitChunked(a), /chunked/);
    assert.equal(s.anchor().liveUpperBound, 0);
  }
});

test("chunked 遵守 release 与 WASM 预算并记录 JS 堆观测", { timeout: 10000 }, async () => {
  const a = await lease(), s = create(); const first = s.owner.admitChunked(a).ticket;
  assert.throws(() => s.owner.admitChunked(a), /上一实例/);
  first.registerRelease(Promise.resolve({ kind: "released", finalBytes: 256 * MiB, growthStopped: true, referencesDropped: true }));
  await s.owner.finish(first);
  const second = s.owner.admitChunked(a).ticket;
  second.registerRelease(Promise.resolve({ kind: "released", finalBytes: 256 * MiB, growthStopped: true, referencesDropped: true }));
  await s.owner.finish(second);
  assert.throws(() => s.owner.admitChunked(a), /不支持.*reload|不支持.*重载/);
  const over = create(CHUNKED_RESTORE_EXPORTS, 256 * MiB + 1);
  const observed = over.owner.admitChunked(a);
  assert.equal(observed.kind, "admitted");
  assert.deepEqual(over.owner.latestJsObservation(), { kind: "observed", source: "performance.memory.totalJSHeapSize", bytes: 256 * MiB + 1 });
  assert.equal(over.anchor().liveUpperBound, 256 * MiB);
  observed.ticket.registerRelease(Promise.resolve({ kind: "not-allocated" }));
  await over.owner.finish(observed.ticket);
});

test("固定原分块源后可冻结重载目标且不分配新票", { timeout: 10000 }, async () => {
  const a = await lease(), s = create(), data = new Map<string, string>();
  const sources = createChunkedHandoverSources({ async getItem(key) { return data.get(key) ?? null; }, async addItem(key, value, current) { if (!current()) return false; data.set(key, value); return true; }, async removeItem(key) { data.delete(key); } }, () => "ab".repeat(16));
  const pin = await sources.pin(a, new AbortController().signal);
  for (let i = 0; i < 2; i++) {
    const ticket = s.owner.admitChunked(a).ticket;
    s.owner.recordCommittedBoundary(ticket, { tick: 1, day: 1 });
    ticket.registerRelease(Promise.resolve({ kind: "released", finalBytes: 256 * MiB, growthStopped: true, referencesDropped: true }));
    await s.owner.finish(ticket);
  }
  const result = s.owner.admitChunked(a, { pinnedSource: pin });
  assert.equal(result.kind, "reload");
  if (result.kind !== "reload" || result.handover.kind !== "frozen") throw new Error("应冻结原目标");
  assert.equal(result.handover.pendingIntent?.kind, "load-chunked-source");
  assert.equal(data.size, 1);
});

test("默认 document owner 将租约转发给已注册的 forLease 能力源", { timeout: 10000 }, async () => {
  const a = await lease(), storage = fixture(), key = {}, names = ["sessionStorage", "localStorage"] as const;
  const originals = names.map(name => Object.getOwnPropertyDescriptor(globalThis, name)); let calls = 0;
  try {
    Object.defineProperty(globalThis, "sessionStorage", { configurable: true, value: storage.session.port });
    Object.defineProperty(globalThis, "localStorage", { configurable: true, value: storage.ledger.port });
    configureDocumentWasmRestoreCapabilities(key, { forArchive: () => cap([]), forLease: value => { assert.equal(value, a); calls++; return cap(CHUNKED_RESTORE_EXPORTS); } });
    const owner = getDocumentWasmMemoryOwner(key); const ticket = owner.admitChunked(a).ticket;
    assert.equal(calls, 1); ticket.registerRelease(Promise.resolve({ kind: "not-allocated" })); await owner.finish(ticket);
  } finally {
    names.forEach((name, index) => { const original = originals[index]; if (original === undefined) Reflect.deleteProperty(globalThis, name); else Object.defineProperty(globalThis, name, original); });
  }
});
