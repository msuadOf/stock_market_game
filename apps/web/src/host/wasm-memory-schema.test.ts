import assert from "node:assert/strict";
import test from "node:test";
import {
  WASM_MEMORY_SCHEMA_VERSION, checksumWasmMemoryRecord,
  wasmMemoryLedgerKey, wasmMemoryGenesisKey,
  parseWasmMemoryLedger, parseWasmMemoryGenesis, parseWasmMemorySnapshot,
} from "./wasm-memory-schema.ts";

const MiB = 1024 * 1024;
const chainId = "0123456789abcdef0123456789abcdef";
const forkId = "fedcba9876543210fedcba9876543210";
const otherFork = "11111111111111111111111111111111";
const handoverNonce = "22222222222222222222222222222222";
const context = { chainId, forkId, maximumBytes: 2048 * MiB };
function seal<T extends Record<string, unknown>>(body: T) { return { ...body, checksum: checksumWasmMemoryRecord(body) }; }
function entry(seq = 0, carry = 512 * MiB, reloadCount = 1, outcome = "reload") {
  return seal({ schemaVersion: 1, seq, chainId, carriedResidualBytes: carry,
    reloadCount, handoverNonce, outcome, ts: 1000 + seq, highWater: { carry, reloadCount } });
}
function snapshot() {
  return seal({ schemaVersion: 1, chainId, carryBase: 512 * MiB, sessionResident: 64 * MiB,
    liveUpperBound: 256 * MiB, reloadCount: 3, genesis: false });
}
function genesis() {
  return seal({ schemaVersion: 1, seq: 0, chainId, carriedResidualBytes: 0, reloadCount: 0,
    handoverNonce, outcome: "genesis", ts: 1000, highWater: { carry: 0, reloadCount: 0 } });
}
function reseal(record: Record<string, unknown>, changes: Record<string, unknown>) {
  const { checksum: _checksum, ...body } = record;
  return seal({ ...body, ...changes });
}
const parse = (rows: unknown, fork = forkId, key = wasmMemoryLedgerKey(chainId, fork)) =>
  parseWasmMemoryLedger(rows, { ...context, forkId: fork }, key);

test("首次实际 schema=1，fork key 明确文档写者且拒绝旧共享 key 与非法 128bit id", () => {
  assert.equal(WASM_MEMORY_SCHEMA_VERSION, 1);
  assert.equal(wasmMemoryLedgerKey(chainId, forkId), `wasm-residual-ledger:${chainId}:${forkId}`);
  assert.equal(wasmMemoryGenesisKey(chainId), `wasm-residual-genesis:${chainId}`);
  assert.throws(() => parse([entry()], forkId, `wasm-residual-ledger:${chainId}`), /key/);
  assert.throws(() => parse([entry()], otherFork, wasmMemoryLedgerKey(chainId, forkId)), /key/);
  for (const bad of ["", "a".repeat(31), "g".repeat(32), "A".repeat(32)]) {
    assert.throws(() => wasmMemoryLedgerKey(chainId, bad), /forkId/);
    assert.throws(() => wasmMemoryGenesisKey(bad), /chainId/);
  }
});

test("checksum 用稳定 canonical UTF-8 字段编码，只检测损坏，不作认证", () => {
  assert.equal(checksumWasmMemoryRecord({ b: 2, a: { y: "中文", x: 1 } }), checksumWasmMemoryRecord({ a: { x: 1, y: "中文" }, b: 2 }));
  assert.notEqual(checksumWasmMemoryRecord({ a: 1 }), checksumWasmMemoryRecord({ a: 2 }));
  for (const bad of [undefined, NaN, Infinity, 1n, () => 1, { a: undefined }]) {
    assert.throws(() => checksumWasmMemoryRecord(bad), /checksum/);
  }
  const row = entry();
  assert.throws(() => parse([{ ...row, carriedResidualBytes: 1 }]), /checksum/);
});

test("snapshot 三和含 live，genesis 只允许零锚；不实现 handover 扩展字段", () => {
  const value = snapshot();
  assert.deepEqual(parseWasmMemorySnapshot(value, context), value);
  assert.throws(() => parseWasmMemorySnapshot(reseal(value, { genesis: true }), context), /genesis/);
  assert.throws(() => parseWasmMemorySnapshot(reseal(value, { liveUpperBound: 2048 * MiB }), context), /maximumBytes/);
  assert.throws(() => parseWasmMemorySnapshot(reseal(value, { pendingIntent: "new" }), context), /未知字段/);
  assert.throws(() => parseWasmMemorySnapshot(reseal(value, { chainId: otherFork }), context), /chainId/);
  const zero = reseal(value, { carryBase: 0, sessionResident: 0, liveUpperBound: 0, reloadCount: 0, genesis: true });
  assert.deepEqual(parseWasmMemorySnapshot(zero, context), zero);
});

test("genesis 独立链级单键，不允许混入 fork ledger", () => {
  const value = genesis();
  assert.deepEqual(parseWasmMemoryGenesis(value, context, wasmMemoryGenesisKey(chainId)), value);
  assert.throws(() => parse([value]), /outcome/);
  assert.throws(() => parseWasmMemoryGenesis(entry(), context, wasmMemoryGenesisKey(chainId)), /genesis/);
  assert.throws(() => parseWasmMemoryGenesis(reseal(value, { carriedResidualBytes: 1 }), context, wasmMemoryGenesisKey(chainId)), /genesis/);
});

test("单键 seq 严格递增，FIFO 首 seq 可非零，跨键 seq 无比较", () => {
  const rows = [entry(8), entry(9)];
  assert.deepEqual(parse(rows), rows);
  assert.deepEqual(parse([entry(0)], otherFork), [entry(0)]);
  for (const invalid of [[entry(1), entry(1)], [entry(2), entry(1)]]) {
    assert.throws(() => parse(invalid), /seq/);
  }
  assert.throws(() => parse([]), /空/);
  assert.throws(() => parse({ entries: rows }), /数组/);
});

test("highWater 对本条和前条保守承接，reloadCount 本值下降不改变有效性", () => {
  const first = entry(0, 1024 * MiB, 7);
  const second = reseal(entry(1, 512 * MiB, 1, "f5"), { highWater: { carry: 1024 * MiB, reloadCount: 7 } });
  assert.deepEqual(parse([first, second]), [first, second]);
  assert.throws(() => parse([first, entry(1, 512 * MiB, 8)]), /highWater/);
  assert.throws(() => parse([reseal(first, { highWater: { carry: 1024 * MiB - 1, reloadCount: 7 } })]), /highWater/);
  const lowerCount = reseal(first, { highWater: { carry: 1024 * MiB, reloadCount: 6 } });
  assert.deepEqual(parse([lowerCount]), [lowerCount]);
  // carry 承接本键历史最大值；两项 reloadCount 仅作诊断，不校验覆盖或跨条目单调。
  const countDrop = reseal(second, { highWater: { carry: 1024 * MiB, reloadCount: 1 } });
  assert.deepEqual(parse([first, countDrop]), [first, countDrop]);
});

test("nonce 仅作合法 128bit 诊断字段，不把跨条目不配对当损坏", () => {
  const second = reseal(entry(1), { handoverNonce: otherFork });
  assert.deepEqual(parse([entry(), second]), [entry(), second]);
  assert.throws(() => parse([reseal(entry(), { handoverNonce: "bad" })]), /handoverNonce/);
});

test("拒绝未知/缺失字段、错误版本/单位/非整数/越界/矛盾与伪造 checksum 格式", () => {
  const base = entry();
  for (const changes of [{ extra: 1 }, { schemaVersion: 0 }, { schemaVersion: 2 }, { ts: "1s" },
    { seq: -1 }, { seq: 1.2 }, { carriedResidualBytes: "512MiB" },
    { carriedResidualBytes: 2048 * MiB + 1 }, { reloadCount: Number.MAX_SAFE_INTEGER + 1 },
    { outcome: "unknown" }, { highWater: { carry: 512 * MiB, reloadCount: 1, extra: 0 } }]) {
    assert.throws(() => parse([reseal(base, changes)]));
  }
  const { ts: _ts, ...missing } = base;
  assert.throws(() => parse([missing]), /ts/);
  assert.throws(() => parse([{ ...base, checksum: "00000000" }]), /checksum/);
  assert.throws(() => parse([null]), /对象/);
  assert.throws(() => parseWasmMemorySnapshot({ ...snapshot(), checksum: "bad" }, context), /checksum/);
});

test("最终补4：仅修改诊断计数不改变 parser 结果和预算/重建输出", async () => {
  const { evaluateWasmMemoryBudget, rebuildWasmResidualBytes } = await import("./wasm-memory-budget.ts");
  const lowCounter = reseal(entry(), { reloadCount: 999, highWater: { carry: 512 * MiB, reloadCount: 0 } });
  const rows = parse([lowCounter]);
  const normalRows = parse([entry()]);
  const parameters = { maximumBytes: context.maximumBytes, reclaimBytes: 512 * MiB, measuredMaxLiveBytes: 768 * MiB };
  const decision = (carryBase: number) => evaluateWasmMemoryBudget({ ...parameters, carryBase, sessionResident: 0, liveBytes: 0 }, "admission");
  assert.deepEqual(decision(rows[0].highWater.carry), decision(normalRows[0].highWater.carry));
  assert.equal(rebuildWasmResidualBytes(rows[0].highWater.carry, parameters), rebuildWasmResidualBytes(normalRows[0].highWater.carry, parameters));
  const diagnosticGenesis = reseal(genesis(), { reloadCount: 999, highWater: { carry: 0, reloadCount: 1 } });
  assert.equal(parseWasmMemoryGenesis(diagnosticGenesis, context, wasmMemoryGenesisKey(chainId)).reloadCount, 999);
  const zero = reseal(snapshot(), { carryBase: 0, sessionResident: 0, liveUpperBound: 0, reloadCount: 999, genesis: true });
  assert.equal(parseWasmMemorySnapshot(zero, context).reloadCount, 999);
});
