import assert from "node:assert/strict";
import test from "node:test";
import { WasmMemoryStorageError } from "./wasm-memory-storage.ts";
import { wasmMemoryLedgerKey } from "./wasm-memory-schema.ts";
import { fixture, MiB, CHAIN, CHAIN_KEY, SNAPSHOT_KEY, FORK_A, FORK_B, snapshot, ledgerEntry, seal } from "./wasm-memory-storage.test-support.ts";

const code = (expected: string) => (error: unknown) => error instanceof WasmMemoryStorageError && error.code === expected && error.where.length > 0 && error.recoveryActions.length > 0;

test("新链建立chain与双genesis，fork仅内存，闲置刷新不创建分叉key", () => {
  const f = fixture(); const first = f.owner(); const opened = first.open();
  assert.equal(opened.source, "new-chain"); assert.equal(opened.carriedResidualBytes, 0);
  assert.match(opened.chainId, /^[0-9a-f]{32}$/); assert.equal(f.session.data.get(CHAIN_KEY), opened.chainId);
  assert.equal(f.ledger.data.size, 1); assert.equal(f.session.data.size, 2);
  assert.ok(!f.session.data.has("forkId")); assert.ok(!f.ledger.data.has(wasmMemoryLedgerKey(opened.chainId, first.forkId)));
  const next = f.owner(); assert.notEqual(next.forkId, first.forkId); assert.equal(next.open().carriedResidualBytes, 0);
  assert.equal(f.ledger.data.size, 1);
});

test("活动live三和F5只折叠一次，正推进落f5，起步锚清session/live", () => {
  const f = fixture(); f.existing(snapshot(512 * MiB, 64 * MiB, 256 * MiB, 3)); f.put(FORK_A, [ledgerEntry(0, 512 * MiB, 3)]);
  const owner = f.owner(), opened = owner.open();
  assert.equal(opened.source, "snapshot"); assert.equal(opened.carriedResidualBytes, 832 * MiB); assert.equal(opened.reloadCount, 3);
  const rows = JSON.parse(f.ledger.data.get(wasmMemoryLedgerKey(CHAIN, owner.forkId))!);
  assert.equal(rows[0].outcome, "f5"); assert.equal(rows[0].carriedResidualBytes, 832 * MiB); assert.equal(rows[0].reloadCount, 3);
  const anchor = JSON.parse(f.session.data.get(SNAPSHOT_KEY)!);
  assert.deepEqual([anchor.carryBase, anchor.sessionResident, anchor.liveUpperBound], [832 * MiB, 0, 0]);
  assert.equal(f.owner().open().carriedResidualBytes, 832 * MiB);
});

test("快照红而ledger绿用max/highWater双补足；未知真实measurement显式not-ready不写新锚", () => {
  const f = fixture(); f.existing({ broken: true }); f.put(FORK_A, [ledgerEntry(0, 512 * MiB)]); f.put(FORK_B, [ledgerEntry(0, 1024 * MiB)]);
  assert.equal(f.owner().open().carriedResidualBytes, 1792 * MiB);
  const g = fixture({ measuredMaxLiveBytes: null }); g.existing(null); g.put(FORK_A, [ledgerEntry()]);
  const before = [...g.session.data]; const result = g.owner().open();
  assert.equal(result.kind, "not-ready"); assert.equal(result.carriedResidualBytes, null); assert.deepEqual([...g.session.data], before);
});

test("仅genesis保守继承maximum，双无CORRUPT不静默归零", () => {
  const f = fixture(); f.existing(null); assert.equal(f.owner().open().carriedResidualBytes, 2048 * MiB);
  const g = fixture(); g.session.data.set(CHAIN_KEY, CHAIN);
  assert.throws(() => g.owner().open(), code("MEMORY_HANDOVER_CORRUPT"));
});

test("绿快照领先时ledger损坏/降序只诊断，不改继承；ledger领先使旧绿快照降级", () => {
  const f = fixture(); f.existing(snapshot(1024 * MiB)); f.put(FORK_A, [ledgerEntry(2), ledgerEntry(1)]);
  const green = f.owner().open(); assert.equal(green.carriedResidualBytes, 1024 * MiB); assert.ok(green.diagnostics.length > 0);
  const g = fixture(); g.existing(snapshot(512 * MiB)); g.put(FORK_A, [ledgerEntry(0, 1024 * MiB)]);
  const stale = g.owner().open(); assert.equal(stale.carriedResidualBytes, 1792 * MiB); assert.equal(stale.source, "ledger");
  assert.ok(stale.diagnostics.some((item) => item.code === "MEMORY_SNAPSHOT_STALE"));
});

test("重建时坏checksum单条显式弃用但保留绿锚，同key seq降序明确CORRUPT", () => {
  const f = fixture(); f.existing(null); const bad = { ...ledgerEntry(1, 1024 * MiB), checksum: "fnv1a32:00000000" };
  f.put(FORK_A, [ledgerEntry(), bad]); const opened = f.owner().open();
  assert.equal(opened.carriedResidualBytes, 1280 * MiB); assert.ok(opened.diagnostics.length > 0);
  const g = fixture(); g.existing(null); g.put(FORK_A, [ledgerEntry(2), ledgerEntry(1)]);
  assert.throws(() => g.owner().open(), code("MEMORY_HANDOVER_CORRUPT"));
});

test("不同fork seq不可比且计数999/下降不影响继承与写门", () => {
  const f = fixture(); f.existing(null);
  const raw = ledgerEntry(8, 512 * MiB); const { checksum: _checksum, ...body } = raw;
  f.put(FORK_A, [seal({ ...body, reloadCount: 999, highWater: { carry: 512 * MiB, reloadCount: 0 } })]);
  f.put(FORK_B, [ledgerEntry(0, 1024 * MiB, 1)]);
  const owner = f.owner(); assert.equal(owner.open().carriedResidualBytes, 1792 * MiB); assert.doesNotThrow(() => owner.assertAdmissionWritable());
});

test("chain错误或storage读取失败显式保原cause，不可自建新链洗账", () => {
  const f = fixture(); f.session.data.set(CHAIN_KEY, "bad"); assert.throws(() => f.owner().open(), code("MEMORY_HANDOVER_CORRUPT"));
  const g = fixture(); const cause = new Error("privacy mode"); g.session.onCall(() => { throw cause; });
  assert.throws(() => g.owner().open(), (error) => error instanceof WasmMemoryStorageError && error.code === "MEMORY_CHAIN_UNAVAILABLE" && error.cause === cause);
  assert.equal(g.ledger.calls.length, 0);
});

test("坏中间条不吞后续独立highWater绿锚，跨坏条不强求exact承接", () => {
  const f = fixture(); f.existing(null);
  const e0 = ledgerEntry(0, 512 * MiB); const bad = { ...ledgerEntry(1, 1536 * MiB), checksum: "fnv1a32:00000000" };
  const { checksum: _checksum, ...body } = ledgerEntry(2, 256 * MiB);
  const e2 = seal({ ...body, highWater: { carry: 1536 * MiB, reloadCount: 1 } });
  f.put(FORK_A, [e0, bad, e2]);
  assert.equal(f.owner().open().carriedResidualBytes, 2048 * MiB);
});

test("主通道绿而ledger读错仍保三和，正值无法证明已落账就写完整checkpoint", () => {
  const f = fixture(); f.existing(snapshot(1024 * MiB));
  f.ledger.onCall((call) => { if (call.operation === "keys") throw new Error("enumeration unavailable"); });
  const owner = f.owner(); const result = owner.open();
  assert.equal(result.source, "snapshot"); assert.equal(result.carriedResidualBytes, 1024 * MiB);
  assert.ok(result.diagnostics.some((item) => item.code === "MEMORY_LEDGER_READ_FAILED"));
  const rows = JSON.parse(f.ledger.data.get(wasmMemoryLedgerKey(CHAIN, owner.forkId))!);
  assert.equal(rows[0].outcome, "f5"); assert.equal(rows[0].carriedResidualBytes, 1024 * MiB);
});

test("snapshot绿而ledger读取和checkpoint写入均败只触recovery写门，不伪CORRUPT", () => {
  const f = fixture(); f.existing(snapshot(1024 * MiB));
  f.ledger.onCall((call) => { if (call.operation === "keys" || call.operation === "set") throw new Error("ledger unavailable"); });
  const owner = f.owner();
  assert.throws(() => owner.open(), (error) => error instanceof WasmMemoryStorageError && error.code === "MEMORY_HANDOVER_WRITE_FAILED" && error.context.context === "recovery");
});

test("连续活动局F5正推进都落账，随后失快照重建封顶不洗残留", () => {
  const f = fixture(); f.existing(snapshot(512 * MiB)); f.put(FORK_A, [ledgerEntry()]);
  for (let i = 0; i < 4; i += 1) {
    const owner = f.owner(); const result = owner.open();
    owner.writeSnapshot({ carryBase: result.carriedResidualBytes!, sessionResident: 0, liveUpperBound: 256 * MiB, reloadCount: result.reloadCount });
  }
  const last = f.owner().open(); assert.equal(last.carriedResidualBytes, 1536 * MiB);
  f.session.data.delete(SNAPSHOT_KEY); assert.equal(f.owner().open().carriedResidualBytes, 2048 * MiB);
});

test("重建not-ready后所有公开写门都保留同一typed measurement错误", () => {
  const f = fixture({ measuredMaxLiveBytes: null }); f.existing(null); f.put(FORK_A, [ledgerEntry()]);
  const owner = f.owner(); assert.equal(owner.open().kind, "not-ready");
  const errors: unknown[] = [];
  for (const action of [() => owner.assertAdmissionWritable(), () => owner.append("f5", 0, 0),
    () => owner.writeSnapshot({ carryBase: 0, sessionResident: 0, liveUpperBound: 0, reloadCount: 0 })]) {
    assert.throws(action, (error) => { errors.push(error); return error instanceof WasmMemoryStorageError && error.code === "MEMORY_MEASUREMENT_NOT_READY"; });
  }
  assert.equal(errors[0], errors[1]); assert.equal(errors[1], errors[2]);
});

test("主绿零快照遇本人fork键及全部ledger get抛错仍黄带继承0，不产生新键", () => {
  const f = fixture(); f.existing();
  f.ledger.onCall((call) => { if (call.operation === "get") throw new Error("ledger get unavailable"); });
  const result = f.owner().open();
  assert.equal(result.carriedResidualBytes, 0); assert.equal(result.source, "snapshot"); assert.ok(result.diagnostics.length > 0);
  assert.equal(f.ledger.data.size, 1); assert.equal(f.ledger.calls.filter((call) => call.operation === "set").length, 0);
});

test("主绿正快照遇本人fork键读败归recovery写门，不能误报chain unavailable", () => {
  const f = fixture(); f.existing(snapshot(512 * MiB));
  f.ledger.onCall((call) => { if (call.operation === "get") throw new Error("ledger get unavailable"); });
  assert.throws(() => f.owner().open(), (error) => error instanceof WasmMemoryStorageError && error.code === "MEMORY_HANDOVER_WRITE_FAILED" && error.context.context === "recovery");
});

test("highWater不承接的坏单条弃用而保留此前绿锚，仅seq回退无条件拒绝", () => {
  const f = fixture(); f.existing(null);
  f.put(FORK_A, [ledgerEntry(0, 512 * MiB), ledgerEntry(1, 256 * MiB)]);
  const result = f.owner().open();
  assert.equal(result.carriedResidualBytes, 1280 * MiB);
  assert.ok(result.diagnostics.some((item) => item.code === "MEMORY_LEDGER_ENTRY_DISCARDED"));
});

test("连续单条highWater无依据抬升也只弃该条，不吞其他key绿锚", () => {
  const f = fixture(); f.existing(null);
  const { checksum: _checksum, ...body } = ledgerEntry(1, 256 * MiB);
  f.put(FORK_A, [ledgerEntry(0, 512 * MiB), seal({ ...body, highWater: { carry: 1536 * MiB, reloadCount: 1 } })]);
  f.put(FORK_B, [ledgerEntry(0, 1024 * MiB)]);
  assert.equal(f.owner().open().carriedResidualBytes, 1792 * MiB);
});

test("独立可信高账先证明主绿快照stale，他key seq错误不能抹掉证明后放行", () => {
  for (const badFirst of [false, true]) {
    const f = fixture(); f.existing(snapshot(0));
    const { checksum: _checksum, ...body } = ledgerEntry(0, 512 * MiB);
    const good = seal({ ...body, outcome: "snapshot-write-fail" });
    const putGood = () => f.put(FORK_A, [good]);
    const putBad = () => f.put(FORK_B, [ledgerEntry(2), ledgerEntry(1)]);
    if (badFirst) { putBad(); putGood(); } else { putGood(); putBad(); }
    const owner = f.owner();
    assert.throws(() => owner.open(), (error) => error instanceof WasmMemoryStorageError && error.code === "MEMORY_HANDOVER_CORRUPT" && error.context.reason === "SEQ_REGRESSED");
    assert.equal(f.session.calls.filter((call) => call.operation === "set").length, 0);
  }
});

test("主绿零快照但整链ledger锚缺失时显式黄带，仍继承0且不建惰性fork键", () => {
  const f = fixture(); f.existing(snapshot(0)); f.ledger.data.clear();
  const owner = f.owner(); const result = owner.open();
  assert.equal(result.source, "snapshot"); assert.equal(result.carriedResidualBytes, 0);
  assert.ok(result.diagnostics.some((item) => item.code === "MEMORY_LEDGER_ANCHOR_MISSING"));
  assert.equal(f.ledger.data.size, 0); assert.doesNotThrow(() => owner.assertAdmissionWritable());
});

test("丢快照后的ledger重建与仅genesis最坏继承均明确告知补足成因", () => {
  const ledger = fixture(); ledger.existing(null); ledger.put(FORK_A, [ledgerEntry()]);
  const recovered = ledger.owner().open();
  assert.ok(recovered.diagnostics.some((item) => item.code === "MEMORY_SNAPSHOT_MISSING"));
  assert.ok(recovered.diagnostics.some((item) => item.code === "MEMORY_RESIDUAL_REBUILT" && /双上界|双补足/.test(item.message)));
  const genesis = fixture(); genesis.existing(null); const worst = genesis.owner().open();
  assert.ok(worst.diagnostics.some((item) => item.code === "MEMORY_RESIDUAL_REBUILT" && /maximum|最坏/.test(item.message)));
});
