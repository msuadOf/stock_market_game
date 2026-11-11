import assert from "node:assert/strict";
import test from "node:test";
import { DEFAULT_SETUP } from "../config/defaults.ts";
import { commandDayEndArchiveFixture } from "../app/command-host-test-fixture.ts";
import { currentSaveFixture } from "../save/current-save-fixture.ts";
import { parseWasmMemoryHandover } from "./wasm-memory-handover.ts";

const maximum = 1024;
function frozen() {
  return { kind: "frozen", code: "MEMORY_RESIDUAL_RELOAD_HANDOVER", residualBytes: 256, carriedResidualBytes: 512,
    thresholdEffective: 256, reloadCount: 0, pendingIntent: null, last_committed_tick: 0, day: 0, handoverNonce: "ab".repeat(16) };
}
test("冻结历史handover严格roundtrip，null意图仍是待确认事件", () => {
  assert.deepEqual(parseWasmMemoryHandover(frozen(), maximum), frozen());
});
test("固定分块源描述严格往返且拒绝伪造摘要", { timeout: 10000 }, () => {
  const descriptor = { kind: "load-chunked-source", sourceId: "ab".repeat(16), utf8Bytes: 2, seed: "1", contentDigest: { algorithm: "sha256-chain-v1", blockBytes: 65536, byteLength: "2", blocks: "1", digest: "cd".repeat(32) } };
  const input = { ...frozen(), pendingIntent: descriptor };
  assert.deepEqual(parseWasmMemoryHandover(input, maximum), input);
  assert.throws(() => parseWasmMemoryHandover({ ...input, pendingIntent: { ...descriptor, contentDigest: { ...descriptor.contentDigest, blocks: "2" } } }, maximum), /摘要/);
});
test("lost只保目标丢失原因，不伪造历史账/nonce/tick", () => {
  for (const reason of ["checksum-invalid", "schema-invalid", "intent-invalid", "carrier-unavailable"]) {
    assert.deepEqual(parseWasmMemoryHandover({ kind: "lost", reason }, maximum), { kind: "lost", reason });
  }
  assert.throws(() => parseWasmMemoryHandover({ kind: "lost", reason: "anything" }, maximum), /reason/);
  assert.throws(() => parseWasmMemoryHandover({ kind: "lost", reason: "intent-invalid", handoverNonce: "ab".repeat(16) }, maximum), /字段/);
});
test("新局保存完整setup与规范u64 seed，0和u64最大均不转Number", () => {
  for (const seed of ["0", "18446744073709551615"]) {
    const input = { ...frozen(), pendingIntent: { kind: "new", setup: DEFAULT_SETUP, seed } };
    assert.deepEqual(parseWasmMemoryHandover(input, maximum), input);
  }
});
test("实际选档X经过日终校验并隔离caller后改，不重读quickslot或替换X", () => {
  const archive = commandDayEndArchiveFixture(); const expected = structuredClone(archive);
  const input = { ...frozen(), pendingIntent: { kind: "load", archive } };
  const parsed = parseWasmMemoryHandover(input, maximum);
  (archive.setup as { start_date: string }).start_date = "2031-02-03";
  assert.equal(parsed.kind, "frozen");
  if (parsed.kind !== "frozen" || parsed.pendingIntent?.kind !== "load") throw new Error("预期同份X");
  assert.deepEqual(parsed.pendingIntent.archive, expected);
});
test("new setup也必须显式a-share-simulation，不补默认或接受另一政策", () => {
  assert.throws(() => parseWasmMemoryHandover({ ...frozen(), pendingIntent: { kind: "new", setup: { ...DEFAULT_SETUP, simulation_policy_id: "other-policy" }, seed: "1" } }, maximum), /simulation_policy_id/);
});
test("new seed拒非规范字符串及u64溢出", () => {
  for (const seed of [1, "01", "-1", "18446744073709551616"]) {
    assert.throws(() => parseWasmMemoryHandover({ ...frozen(), pendingIntent: { kind: "new", setup: DEFAULT_SETUP, seed } }, maximum), /seed/);
  }
});
test("load拒日内档和不完整X，不fallback成null意图", () => {
  for (const archive of [currentSaveFixture(), { selected: "X" }]) {
    assert.throws(() => parseWasmMemoryHandover({ ...frozen(), pendingIntent: { kind: "load", archive } }, maximum), /日终|存档|根节点/);
  }
});
test("额外/缺失字段、双布局与错误code必须拒绝", () => {
  const missing = { ...frozen() } as Record<string, unknown>; delete missing.day;
  for (const value of [{ ...frozen(), threshold: 256 }, missing, { ...frozen(), code: "MEMORY_RESIDUAL_EXHAUSTED" }, { ...frozen(), checksum: "fake" }]) {
    assert.throws(() => parseWasmMemoryHandover(value, maximum), /字段|code/);
  }
});
test("历史bytes均bounded且正常HANDOVER残留达到阈值，carried不能小于新增", () => {
  for (const patch of [{ residualBytes: -1 }, { carriedResidualBytes: 1025 }, { thresholdEffective: 0 }, { residualBytes: 255 }, { carriedResidualBytes: 255 }, { residualBytes: 1.5 }]) {
    assert.throws(() => parseWasmMemoryHandover({ ...frozen(), ...patch }, maximum), /Bytes|thresholdEffective/);
  }
  assert.equal(parseWasmMemoryHandover({ ...frozen(), carriedResidualBytes: maximum }, maximum).kind, "frozen");
});
test("历史reloadCount仅safe诊断，tick/day保持现number边界，不推导互相关系", () => {
  const value = { ...frozen(), reloadCount: 3, last_committed_tick: Number.MAX_SAFE_INTEGER, day: 1 };
  assert.deepEqual(parseWasmMemoryHandover(value, maximum), value);
  for (const field of ["reloadCount", "last_committed_tick", "day"]) {
    for (const invalid of [-1, Number.MAX_SAFE_INTEGER + 1, "1"]) {
      assert.throws(() => parseWasmMemoryHandover({ ...frozen(), [field]: invalid }, maximum), new RegExp(field));
    }
  }
});
test("nonce必须128bit小写hex，非法maximum显式拒绝", () => {
  for (const handoverNonce of ["", "A".repeat(32), "a".repeat(31)]) assert.throws(() => parseWasmMemoryHandover({ ...frozen(), handoverNonce }, maximum), /handoverNonce/);
  for (const max of [0, -1, 1.5, Infinity]) assert.throws(() => parseWasmMemoryHandover(frozen(), max), /maximumBytes/);
});
test("intent严格union，未知kind或多余归档/setup字段均拒绝", () => {
  for (const pendingIntent of [{ kind: "auto" }, { kind: "new", setup: DEFAULT_SETUP, seed: "1", archive: {} }, { kind: "load", archive: commandDayEndArchiveFixture(), seed: "1" }]) {
    assert.throws(() => parseWasmMemoryHandover({ ...frozen(), pendingIntent }, maximum), /pendingIntent|字段/);
  }
});

test("JSON预检在clone前拒绝root、setup、archive深层Symbol键，不静默丢字段", () => {
  const root = { ...frozen(), [Symbol("hidden")]: "private" };
  const setup = structuredClone(DEFAULT_SETUP); Object.defineProperty(setup.config, Symbol("hidden"), { value: 1, enumerable: true });
  const archive = commandDayEndArchiveFixture(); Object.defineProperty(archive.runtime_state, Symbol("hidden"), { value: 1, enumerable: true });
  for (const value of [root, { ...frozen(), pendingIntent: { kind: "new", setup, seed: "1" } }, { ...frozen(), pendingIntent: { kind: "load", archive } }]) {
    assert.throws(() => parseWasmMemoryHandover(value, maximum), /Symbol|symbol/);
  }
});
test("循环引用和访问器不是JSON载荷，拒绝且不执行getter", () => {
  const cycle: Record<string, unknown> = frozen(); cycle.pendingIntent = cycle;
  assert.throws(() => parseWasmMemoryHandover(cycle, maximum), /循环/);
  let reads = 0; const accessor = frozen(); Object.defineProperty(accessor, "day", { get() { reads++; return 1; }, enumerable: true });
  assert.throws(() => parseWasmMemoryHandover(accessor, maximum), /访问器/); assert.equal(reads, 0);
});

test("稀疏数组与数组伪索引不能以clone后的缺口冒合法JSON", () => {
  const setup = structuredClone(DEFAULT_SETUP);
  const stocks: unknown[] = Array(1); Object.defineProperty(stocks, "4294967295", { value: setup.stocks[0], enumerable: true });
  const input = { ...frozen(), pendingIntent: { kind: "new", setup: { ...setup, stocks }, seed: "1" } };
  assert.throws(() => parseWasmMemoryHandover(input, maximum), /JSON数组|JSON字段/);
});
