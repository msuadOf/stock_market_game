import assert from "node:assert/strict";
import test from "node:test";
import { commandDayEndArchiveFixture } from "../app/command-host-test-fixture.ts";
import { WasmMemoryStorageError } from "./wasm-memory-storage.ts";
import { fixture, seal, snapshot, ledgerEntry, FORK_A, SNAPSHOT_KEY, MiB } from "./wasm-memory-storage.test-support.ts";

function pending() {
  return { kind: "frozen", code: "MEMORY_RESIDUAL_RELOAD_HANDOVER", residualBytes: 512 * MiB, carriedResidualBytes: 512 * MiB,
    thresholdEffective: 512 * MiB, reloadCount: 1, pendingIntent: { kind: "load", archive: commandDayEndArchiveFixture() },
    last_committed_tick: 0, day: 0, handoverNonce: "ab".repeat(16) };
}
function carrier() {
  const row = { ...snapshot(512 * MiB, 0, 0, 1), handover: pending() } as Record<string, unknown>; delete row.checksum;
  return seal(row);
}
function prepared() { const f = fixture(); f.existing(carrier()); f.put(FORK_A, [ledgerEntry()]); return f; }

test("确认前连续F5保同一X与计数，root三和继承幂等不重复历史carry", () => {
  const f = prepared(); const expected = pending();
  for (let count = 0; count < 3; count++) {
    const restored = f.owner().open();
    assert.deepEqual(restored.handover, expected); assert.equal(restored.carriedResidualBytes, 512 * MiB); assert.equal(restored.reloadCount, 1);
    const stored = JSON.parse(f.session.data.get(SNAPSHOT_KEY)!);
    assert.deepEqual(stored.handover, expected); assert.equal(stored.carryBase, 512 * MiB); assert.equal(stored.liveUpperBound, 0);
  }
});
test("ledger高账只重建root账，不把有效X误判损坏或丢弃", () => {
  const f = prepared(); f.put(FORK_A, [ledgerEntry(0, 1024 * MiB, 2)]);
  const restored = f.owner().open(); assert.equal(restored.source, "ledger"); assert.equal(restored.carriedResidualBytes, 1792 * MiB);
  assert.deepEqual(restored.handover, pending()); assert.deepEqual(JSON.parse(f.session.data.get(SNAPSHOT_KEY)!).handover, pending());
});
test("损坏carrier但ledger可恢复时lost跨F5持久，不伪造原事件", () => {
  const f = prepared(); const raw = JSON.parse(f.session.data.get(SNAPSHOT_KEY)!); raw.checksum = "fnv1a32:00000000";
  f.session.data.set(SNAPSHOT_KEY, JSON.stringify(raw));
  for (let count = 0; count < 2; count++) {
    const restored = f.owner().open(); assert.deepEqual(restored.handover, { kind: "lost", reason: "schema-invalid" });
    assert.deepEqual(JSON.parse(f.session.data.get(SNAPSHOT_KEY)!).handover, restored.handover);
  }
});
test("carrier缺失的lost明确记不可用，有效普通快照不误变lost", () => {
  const missing = prepared(); missing.session.data.delete(SNAPSHOT_KEY);
  assert.deepEqual(missing.owner().open().handover, { kind: "lost", reason: "carrier-unavailable" });
  const ordinary = fixture(); ordinary.existing(snapshot(512 * MiB)); ordinary.put(FORK_A, [ledgerEntry()]);
  assert.equal(Object.hasOwn(ordinary.owner().open(), "handover"), false);
  assert.equal(Object.hasOwn(JSON.parse(ordinary.session.data.get(SNAPSHOT_KEY)!), "handover"), false);
});
test("返回的X隔离caller修改，普通锚更新仍保私有pending", () => {
  const f = prepared(); const owner = f.owner(); const result = owner.open();
  assert.ok(result.handover?.kind === "frozen" && result.handover.pendingIntent?.kind === "load");
  (result.handover.pendingIntent.archive.setup as { start_date: string }).start_date = "2031-02-03";
  owner.writeSnapshot({ carryBase: 512 * MiB, sessionResident: 0, liveUpperBound: 0, reloadCount: 1 });
  assert.deepEqual(JSON.parse(f.session.data.get(SNAPSHOT_KEY)!).handover, pending());
});
test("普通writeSnapshot不能在pending未消费时写活动live锚", () => {
  const f = prepared(); const owner = f.owner(); owner.open();
  const before = [...f.session.data];
  assert.throws(() => owner.writeSnapshot({ carryBase: 512 * MiB, sessionResident: 0, liveUpperBound: 256 * MiB, reloadCount: 1 }), /handover.*liveUpperBound/);
  assert.deepEqual([...f.session.data], before);
});
test("消费X与开局live锚单次同key成功更新，之后才明确无pending", () => {
  const f = prepared(); const owner = f.owner(); owner.open(); const before = f.session.calls.length;
  owner.writeAdmissionSnapshot({ carryBase: 512 * MiB, sessionResident: 0, liveUpperBound: 256 * MiB, reloadCount: 1 });
  const writes = f.session.calls.slice(before).filter((call) => call.operation === "set");
  assert.equal(writes.length, 1); assert.equal(writes[0]?.key, SNAPSHOT_KEY);
  const stored = JSON.parse(f.session.data.get(SNAPSHOT_KEY)!); assert.equal(Object.hasOwn(stored, "handover"), false); assert.equal(stored.liveUpperBound, 256 * MiB);
});
test("消费主通道两败即使ledger备份成功也抛错/保X，F5仍待确认", () => {
  const f = prepared(); const owner = f.owner(); owner.open();
  f.session.onCall((call) => { if (call.operation === "set" && call.key === SNAPSHOT_KEY) throw new Error("主carrier quota"); });
  assert.throws(() => owner.writeAdmissionSnapshot({ carryBase: 512 * MiB, sessionResident: 0, liveUpperBound: 256 * MiB, reloadCount: 1 }), (error) => error instanceof WasmMemoryStorageError && error.code === "MEMORY_HANDOVER_WRITE_FAILED" && error.context.channel === "session" && error.context.context === "consume");
  assert.deepEqual(JSON.parse(f.session.data.get(SNAPSHOT_KEY)!).handover, pending());
  f.session.onCall(null);
  assert.deepEqual(f.owner().open().handover, pending());
});
test("消费拒绝零live，不以空写清pending", () => {
  const f = prepared(); const owner = f.owner(); owner.open(); const before = [...f.session.data];
  assert.throws(() => owner.writeAdmissionSnapshot({ carryBase: 512 * MiB, sessionResident: 0, liveUpperBound: 0, reloadCount: 1 }), /liveUpperBound/);
  assert.deepEqual([...f.session.data], before);
});

test("消费主写失败后的普通更新和重试仍以原X待确认，成功重试才清pending", () => {
  const f = prepared(); const owner = f.owner(); owner.open();
  const admission = { carryBase: 512 * MiB, sessionResident: 0, liveUpperBound: 256 * MiB, reloadCount: 1 };
  f.session.onCall((call) => { if (call.operation === "set" && call.key === SNAPSHOT_KEY) throw new Error("主carrier quota"); });
  assert.throws(() => owner.writeAdmissionSnapshot(admission), /尚未受理/);
  f.session.onCall(null);
  owner.writeSnapshot({ ...admission, liveUpperBound: 0 }); assert.deepEqual(JSON.parse(f.session.data.get(SNAPSHOT_KEY)!).handover, pending());
  owner.writeAdmissionSnapshot(admission); assert.equal(Object.hasOwn(JSON.parse(f.session.data.get(SNAPSHOT_KEY)!), "handover"), false);
});
test("无pending的普通开局锚仍允许主写两败但安全ledger备份成功", () => {
  const f = fixture(); f.existing(); const owner = f.owner(); owner.open();
  f.session.onCall((call) => { if (call.operation === "set" && call.key === SNAPSHOT_KEY) throw new Error("主carrier quota"); });
  assert.doesNotThrow(() => owner.writeAdmissionSnapshot({ carryBase: 0, sessionResident: 0, liveUpperBound: 256 * MiB, reloadCount: 0 }));
  assert.ok(owner.diagnostics().some((entry) => entry.code === "MEMORY_SNAPSHOT_WRITE_FAILED"));
});
test("pending消费双败保持原门和X，不给失败后再次写入旁路", () => {
  const f = prepared(); const owner = f.owner(); owner.open();
  f.session.onCall((call) => { if (call.operation === "set") throw new Error("session拒绝"); });
  f.ledger.onCall((call) => { if (call.operation === "set") throw new Error("ledger拒绝"); });
  assert.throws(() => owner.writeAdmissionSnapshot({ carryBase: 512 * MiB, sessionResident: 0, liveUpperBound: 256 * MiB, reloadCount: 1 }), (error) => error instanceof WasmMemoryStorageError && error.context.channel === "both");
  f.session.onCall(null); f.ledger.onCall(null);
  assert.throws(() => owner.assertAdmissionWritable(), (error) => error instanceof WasmMemoryStorageError && error.context.channel === "both");
  assert.deepEqual(JSON.parse(f.session.data.get(SNAPSHOT_KEY)!).handover, pending());
});
