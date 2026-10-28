import assert from "node:assert/strict";
import test from "node:test";
import { WasmMemoryStorageError } from "./wasm-memory-storage.ts";
import { wasmMemoryLedgerKey } from "./wasm-memory-schema.ts";
import { fixture, MiB, CHAIN, CHAIN_KEY, SNAPSHOT_KEY, FORK_A, snapshot, ledgerEntry } from "./wasm-memory-storage.test-support.ts";

test("仅本人fork写入，seq本键递增，highWater承接，重试同encoded不重复分配", () => {
  const f = fixture(); f.existing(); const owner = f.owner(); owner.open();
  const own = wasmMemoryLedgerKey(CHAIN, owner.forkId);
  const transient = new Error("transient quota");
  let failOnce = true; f.ledger.onCall((call) => { if (call.operation === "set" && failOnce) { failOnce = false; throw transient; } });
  owner.append("reload", 1024 * MiB, 7); owner.append("f5", 512 * MiB, 1);
  const sets = f.ledger.calls.filter((call) => call.operation === "set");
  assert.equal(sets.length, 3); assert.equal(sets[0].value, sets[1].value);
  const retried = owner.diagnostics().find((item) => item.code === "MEMORY_STORAGE_WRITE_RETRIED");
  assert.equal(retried?.cause, transient);
  assert.ok(retried?.message.includes(own)); assert.match(retried!.message, /重试.*成功/);
  assert.ok(sets.every((call) => call.key === own));
  const rows = JSON.parse(f.ledger.data.get(own)!);
  assert.deepEqual(rows.map((row: { seq: number }) => row.seq), [0, 1]);
  assert.deepEqual(rows[1].highWater, { carry: 1024 * MiB, reloadCount: 7 });
});

test("per-key UTF8字节FIFO保留末条highWater，单条超限显式失败不丢旧账", () => {
  const f = fixture({ ledgerMaxBytes: 650 }); f.existing(); const owner = f.owner(); owner.open();
  owner.append("reload", 1024 * MiB, 7);
  for (let i = 0; i < 8; i += 1) owner.append("f5", 512 * MiB, 1);
  const encoded = f.ledger.data.get(wasmMemoryLedgerKey(CHAIN, owner.forkId))!;
  assert.ok(new TextEncoder().encode(encoded).byteLength <= 650);
  const rows = JSON.parse(encoded); assert.ok(rows[0].seq > 0); assert.equal(rows.at(-1).seq, 8);
  assert.equal(rows.at(-1).highWater.carry, 1024 * MiB);
  const g = fixture({ ledgerMaxBytes: 32 }); g.existing(); const short = g.owner(); short.open();
  assert.throws(() => short.append("reload", 1, 1), (error) => error instanceof WasmMemoryStorageError && error.code === "MEMORY_HANDOVER_WRITE_FAILED");
  assert.equal(g.ledger.data.size, 1);
});

test("genesis或chain建立写败启动红，不删除已存在的部分证据", () => {
  for (const channel of ["session", "ledger"] as const) {
    const f = fixture(); const cause = new Error(`${channel} quota`);
    f[channel].onCall((call) => { if (call.operation === "set") throw cause; });
    assert.throws(() => f.owner().open(), (error) => error instanceof WasmMemoryStorageError && error.code === "MEMORY_CHAIN_UNAVAILABLE" && error.cause instanceof Error);
    assert.equal(f.session.data.has(CHAIN_KEY), false, "双genesis均成功前不得发布chain身份");
  }
});

test("F5正推进append两败置恢复门，旧快照不清除也不允许继续开局", () => {
  const f = fixture(); f.existing(snapshot(512 * MiB, 0, 256 * MiB)); f.put(FORK_A, [ledgerEntry()]);
  f.ledger.onCall((call) => { if (call.operation === "set") throw new Error("ledger quota"); });
  const owner = f.owner();
  assert.throws(() => owner.open(), (error) => error instanceof WasmMemoryStorageError && error.code === "MEMORY_HANDOVER_WRITE_FAILED" && error.context.channel === "ledger" && error.context.context === "recovery");
  assert.throws(() => owner.assertAdmissionWritable(), /恢复|写入/);
  assert.equal(f.ledger.calls.filter((call) => call.operation === "set").length, 2);
  assert.equal(JSON.parse(f.session.data.get(SNAPSHOT_KEY)!).liveUpperBound, 256 * MiB);
});

test("快照写败dirty并落snapshot-write-fail，重开时旧绿快照落后必重建", () => {
  const f = fixture(); f.existing(); const owner = f.owner(); owner.open();
  f.session.onCall((call) => { if (call.operation === "set" && call.key === SNAPSHOT_KEY) throw new Error("session quota"); });
  owner.writeSnapshot({ carryBase: 0, sessionResident: 0, liveUpperBound: 512 * MiB, reloadCount: 0 });
  assert.doesNotThrow(() => owner.assertAdmissionWritable());
  assert.ok(owner.diagnostics().some((item) => item.code === "MEMORY_SNAPSHOT_WRITE_FAILED"));
  const rows = JSON.parse(f.ledger.data.get(wasmMemoryLedgerKey(CHAIN, owner.forkId))!);
  assert.equal(rows.at(-1).outcome, "snapshot-write-fail"); assert.equal(rows.at(-1).carriedResidualBytes, 512 * MiB);
  f.session.onCall(null);
  const restored = f.owner().open(); assert.equal(restored.source, "ledger"); assert.equal(restored.carriedResidualBytes, 1280 * MiB);
});

test("快照与ledger双败置anchor门，后续偶然写成功不静默解门", () => {
  const f = fixture(); f.existing(); const owner = f.owner(); owner.open();
  f.session.onCall((call) => { if (call.operation === "set") throw new Error("session fail"); });
  f.ledger.onCall((call) => { if (call.operation === "set") throw new Error("ledger fail"); });
  assert.throws(() => owner.writeSnapshot({ carryBase: 0, sessionResident: 0, liveUpperBound: 256 * MiB, reloadCount: 0 }),
    (error) => error instanceof WasmMemoryStorageError && error.code === "MEMORY_HANDOVER_WRITE_FAILED" && error.context.context === "anchor");
  f.session.onCall(null); f.ledger.onCall(null);
  assert.throws(() => owner.assertAdmissionWritable(), /写入/);
});

test("所有写入先校验schema/bytes/outcome，非法输入不触storage mutation", () => {
  const f = fixture(); f.existing(); const owner = f.owner(); owner.open();
  const before = [...f.session.data, ...f.ledger.data];
  for (const bad of [-1, 1.5, Infinity, Number.MAX_SAFE_INTEGER + 1]) assert.throws(() => owner.append("f5", bad, 0));
  assert.throws(() => owner.append("unsupported" as "f5", 0, 0));
  assert.throws(() => owner.writeSnapshot({ carryBase: 0, sessionResident: 0, liveUpperBound: 0, reloadCount: 0, extra: true } as never));
  assert.deepEqual([...f.session.data, ...f.ledger.data], before);
});

test("open只可一次，未open不得append/写锚/准入，fork碰撞不覆盖已有写者", () => {
  const f = fixture(); f.existing(); const owner = f.owner();
  assert.throws(() => owner.assertAdmissionWritable(), /open|初始化/);
  assert.throws(() => owner.append("f5", 0, 0), /open|初始化/);
  assert.throws(() => owner.writeSnapshot({ carryBase: 0, sessionResident: 0, liveUpperBound: 0, reloadCount: 0 }), /open|初始化/);
  owner.open(); assert.throws(() => owner.open(), /一次|重复/);
  f.options.randomValues = (bytes) => bytes.fill(17);
  const first = f.owner(); first.open(); first.append("f5", 1, 0);
  const second = f.owner(); assert.throws(() => second.open(), /fork|写者/);
});

test("本文档carryBase固定与sessionResident只增，schema非法锚不污染尝试基线", () => {
  const f = fixture(); f.existing(snapshot(512 * MiB)); f.put(FORK_A, [ledgerEntry()]); const owner = f.owner(); owner.open();
  assert.throws(() => owner.writeSnapshot({ carryBase: 0, sessionResident: 0, liveUpperBound: 0, reloadCount: 0 }), /carryBase/);
  owner.writeSnapshot({ carryBase: 512 * MiB, sessionResident: 128 * MiB, liveUpperBound: 0, reloadCount: 0 });
  assert.throws(() => owner.writeSnapshot({ carryBase: 512 * MiB, sessionResident: 127 * MiB, liveUpperBound: 0, reloadCount: 0 }), /sessionResident/);
  assert.throws(() => owner.writeSnapshot({ carryBase: 512 * MiB, sessionResident: 2048 * MiB, liveUpperBound: 0, reloadCount: 0 }), /maximumBytes/);
  assert.doesNotThrow(() => owner.writeSnapshot({ carryBase: 512 * MiB, sessionResident: 129 * MiB, liveUpperBound: 0, reloadCount: 0 }));
});

test("合法锚写败仍保留内存尝试基线，不能后续少记同文档drop累计", () => {
  const f = fixture(); f.existing(); const owner = f.owner(); owner.open();
  f.session.onCall((call) => { if (call.operation === "set") throw new Error("snapshot fail"); });
  owner.writeSnapshot({ carryBase: 0, sessionResident: 128 * MiB, liveUpperBound: 0, reloadCount: 0 });
  f.session.onCall(null);
  assert.throws(() => owner.writeSnapshot({ carryBase: 0, sessionResident: 127 * MiB, liveUpperBound: 0, reloadCount: 0 }), /sessionResident/);
});

test("首次发布chain失败保留孤儿genesis但不形成可继承半链", () => {
  const f = fixture(); f.session.onCall((call) => { if (call.operation === "set" && call.key === CHAIN_KEY) throw new Error("chain publish fail"); });
  assert.throws(() => f.owner().open(), (error) => error instanceof WasmMemoryStorageError && error.code === "MEMORY_CHAIN_UNAVAILABLE");
  assert.equal(f.session.data.has(CHAIN_KEY), false); assert.equal(f.ledger.data.size, 1);
  f.session.onCall(null); assert.equal(f.owner().open().source, "new-chain");
});

test("锚入参不能覆盖chain/schema/genesis/checksum内部身份字段", () => {
  const f = fixture(); f.existing(); const owner = f.owner(); owner.open();
  for (const [key, value] of [["chainId", CHAIN], ["schemaVersion", 1], ["genesis", false], ["checksum", "bad"]]) {
    assert.throws(() => owner.writeSnapshot({ carryBase: 0, sessionResident: 0, liveUpperBound: 0, reloadCount: 0, [key as string]: value } as never), /未知|字段/);
  }
});

test("本人fork末seq被外部降序或删除时append拒绝，不覆盖已确认历史", () => {
  for (const deleted of [false, true]) {
    const f = fixture(); f.existing(); const owner = f.owner(); owner.open();
    owner.append("f5", 512 * MiB, 1); owner.append("f5", 1024 * MiB, 1);
    const key = wasmMemoryLedgerKey(CHAIN, owner.forkId);
    if (deleted) f.ledger.data.delete(key);
    else f.ledger.data.set(key, JSON.stringify([ledgerEntry(0, 512 * MiB)]));
    const before = f.ledger.data.get(key);
    assert.throws(() => owner.append("f5", 1280 * MiB, 1));
    assert.equal(f.ledger.data.get(key), before);
  }
});
