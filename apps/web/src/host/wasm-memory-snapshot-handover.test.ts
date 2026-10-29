import assert from "node:assert/strict";
import test from "node:test";
import { checksumWasmMemoryRecord, parseWasmMemorySnapshot } from "./wasm-memory-schema.ts";
import { commandDayEndArchiveFixture } from "../app/command-host-test-fixture.ts";

const chainId = "ab".repeat(16), maximumBytes = 2048;
const context = { chainId, maximumBytes };
function seal<T extends Record<string, unknown>>(body: T) { return { ...body, checksum: checksumWasmMemoryRecord(body) }; }
function frozen() {
  return { kind: "frozen", code: "MEMORY_RESIDUAL_RELOAD_HANDOVER", residualBytes: 256, carriedResidualBytes: 512,
    thresholdEffective: 256, reloadCount: 1, pendingIntent: { kind: "load", archive: commandDayEndArchiveFixture() }, last_committed_tick: 0, day: 0, handoverNonce: "cd".repeat(16) };
}
function base() { return { schemaVersion: 1, chainId, carryBase: 1024, sessionResident: 0, liveUpperBound: 0, reloadCount: 2, genesis: false }; }

test("单键snapshot唯一root checksum覆盖完整冻结X，历史账不约束当前账", () => {
  const body = { ...base(), handover: frozen() }; const value = seal(body);
  assert.deepEqual(parseWasmMemorySnapshot(value, context), value);
});
test("篡改完整X必须使root checksum失败，不以nested另一个checksum放行", () => {
  const value = seal({ ...base(), handover: frozen() });
  (value.handover.pendingIntent.archive.setup as { start_date: string }).start_date = "2031-02-03";
  assert.throws(() => parseWasmMemorySnapshot(value, context), /checksum/);
});
test("lost原因和null目标仍保持待确认carrier，不转普通锚", () => {
  for (const handover of [{ kind: "lost", reason: "carrier-unavailable" }, { ...frozen(), pendingIntent: null }]) {
    const value = seal({ ...base(), handover });
    assert.deepEqual(parseWasmMemorySnapshot(value, context), value);
  }
});
test("基础snapshot不增加占位字段，旧边界仍严格", () => {
  const value = seal(base()); assert.deepEqual(parseWasmMemorySnapshot(value, context), value);
  assert.equal(Object.hasOwn(parseWasmMemorySnapshot(value, context), "handover"), false);
  assert.throws(() => parseWasmMemorySnapshot(seal({ ...base(), pendingIntent: null }), context), /未知字段/);
});
test("nested必须严格codec，root不能用绿checksum容忍坏意图或第二checksum", () => {
  for (const handover of [{ ...frozen(), checksum: "fake" }, { ...frozen(), pendingIntent: { kind: "auto" } }, { kind: "lost", reason: "invalid" }]) {
    assert.throws(() => parseWasmMemorySnapshot(seal({ ...base(), handover }), context), /字段|pendingIntent|reason/);
  }
});

test("genesis零锚不能携带已发生handover事件", () => {
  const value = seal({ ...base(), carryBase: 0, reloadCount: 0, genesis: true, handover: frozen() });
  assert.throws(() => parseWasmMemorySnapshot(value, context), /genesis.*handover/);
});

test("待确认handover不得同时声称有活动World的liveUpperBound", () => {
  const value = seal({ ...base(), liveUpperBound: 256, handover: frozen() });
  assert.throws(() => parseWasmMemorySnapshot(value, context), /handover.*liveUpperBound/);
});

test("root非枚举handover不能逃过唯一checksum覆盖", () => {
  const value = seal(base()); Object.defineProperty(value, "handover", { value: frozen(), enumerable: false });
  assert.throws(() => parseWasmMemorySnapshot(value, context), /data|枚举|JSON/);
});
test("root handover访问器不能校验X却返回Y，getter不执行", () => {
  const value = seal({ ...base(), handover: frozen() }); let reads = 0;
  Object.defineProperty(value, "handover", { enumerable: true, get() { reads++; return { ...frozen(), day: reads }; } });
  assert.throws(() => parseWasmMemorySnapshot(value, context), /data|访问器|JSON/); assert.equal(reads, 0);
});
test("nested访问器在root checksum遍历前拒绝，getter不执行", () => {
  const handover = frozen(); let reads = 0;
  Object.defineProperty(handover, "day", { enumerable: true, get() { reads++; return 0; } });
  const value = { ...base(), handover, checksum: "fnv1a32:00000000" };
  assert.throws(() => parseWasmMemorySnapshot(value, context), /访问器/); assert.equal(reads, 0);
});

test("其他root访问器也在取值前拒绝，不能借计数字段改handover", () => {
  const value = seal({ ...base(), handover: frozen() }); let reads = 0;
  Object.defineProperty(value, "reloadCount", { enumerable: true, get() { reads++; value.handover.day = reads; return 2; } });
  assert.throws(() => parseWasmMemorySnapshot(value, context), /data|访问器/); assert.equal(reads, 0);
});
