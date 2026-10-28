import assert from "node:assert/strict";
import test from "node:test";
import { WasmMemoryStorageError } from "./wasm-memory-storage.ts";
import { wasmMemoryLedgerKey } from "./wasm-memory-schema.ts";
import { fixture, MiB, CHAIN, FORK_A, FORK_B, SNAPSHOT_KEY, ledgerEntry } from "./wasm-memory-storage.test-support.ts";

const corrupt = (reason: string) => (error: unknown) => error instanceof WasmMemoryStorageError && error.code === "MEMORY_HANDOVER_CORRUPT" && error.context.reason === reason;

test("静态Object.keys快照不受别键删除导致活体下标左移漏读", () => {
  const f = fixture(); f.existing(null); f.put(FORK_A, [ledgerEntry(0, 512 * MiB)]); f.put(FORK_B, [ledgerEntry(0, 1792 * MiB)]);
  f.ledger.data.set("unrelated", "value");
  f.ledger.onCall((call) => { if (call.operation === "get") f.ledger.data.delete("unrelated"); });
  assert.equal(f.owner().open().carriedResidualBytes, 2048 * MiB);
});

test("快照后出现新fork必须定差追读，跨fork seq不比较", () => {
  const f = fixture(); f.existing(null); f.put(FORK_A, [ledgerEntry(8, 512 * MiB)]);
  let inserted = false;
  f.ledger.onCall((call) => { if (call.operation === "get" && call.key === wasmMemoryLedgerKey(CHAIN, FORK_A) && !inserted) { inserted = true; f.put(FORK_B, [ledgerEntry(0, 1536 * MiB)]); } });
  assert.equal(f.owner().open().carriedResidualBytes, 2048 * MiB);
});

test("已枚举最大键null必须弃整轮缓存，整链删空后CORRUPT不能拿旧小账放行", () => {
  const f = fixture(); f.existing(null); f.put(FORK_A, [ledgerEntry(0, 512 * MiB)]); f.put(FORK_B, [ledgerEntry(0, 1792 * MiB)]);
  f.ledger.onCall((call) => { if (call.operation === "get" && call.key === wasmMemoryLedgerKey(CHAIN, FORK_B)) f.ledger.data.clear(); });
  assert.throws(() => f.owner().open(), corrupt("NO_VALID_ANCHOR"));
  assert.equal(f.session.data.has(SNAPSHOT_KEY), false);
});

test("null骑跨后只剩最大键，重枚举收敛并保留保守最大账", () => {
  const f = fixture(); f.existing(null); f.put(FORK_A, [ledgerEntry(0, 512 * MiB)]); f.put(FORK_B, [ledgerEntry(0, 1792 * MiB)]);
  let removed = false;
  f.ledger.onCall((call) => { if (call.operation === "get" && call.key === wasmMemoryLedgerKey(CHAIN, FORK_A) && !removed) { removed = true; f.ledger.data.delete(call.key); } });
  const result = f.owner().open(); assert.equal(result.carriedResidualBytes, 2048 * MiB);
  assert.ok(result.diagnostics.some((item) => item.code === "MEMORY_LEDGER_STRADDLE_RETRY"));
});

test("持续定差新键三轮耗尽显式拒绝，不返回半份并集", () => {
  const f = fixture(); f.existing(null);
  let keys = 0;
  f.ledger.onCall((call) => { if (call.operation === "keys") { keys += 1; const fork = keys.toString(16).padStart(32, "0"); f.put(fork, [ledgerEntry(0, 512 * MiB)]); } });
  assert.throws(() => f.owner().open(), corrupt("ENUMERATION_UNSTABLE"));
});

test("持续骑跨三轮耗尽成因独立于定差风暴", () => {
  const f = fixture(); f.existing(null);
  f.ledger.onCall((call) => {
    if (call.operation === "keys") f.put(FORK_A, [ledgerEntry()]);
    if (call.operation === "get" && call.key === wasmMemoryLedgerKey(CHAIN, FORK_A)) f.ledger.data.delete(call.key);
  });
  assert.throws(() => f.owner().open(), corrupt("STRADDLE_LIMIT"));
});

test("两文档交错写本人key无覆盖，后开文档按全局max继承", () => {
  const f = fixture(); f.existing(); const a = f.owner(), b = f.owner(); a.open(); b.open();
  a.append("reload", 1024 * MiB, 2); b.append("reload", 512 * MiB, 1); a.append("exhausted", 1536 * MiB, 2);
  assert.equal(JSON.parse(f.ledger.data.get(wasmMemoryLedgerKey(CHAIN, a.forkId))!).at(-1).carriedResidualBytes, 1536 * MiB);
  assert.equal(JSON.parse(f.ledger.data.get(wasmMemoryLedgerKey(CHAIN, b.forkId))!).at(-1).carriedResidualBytes, 512 * MiB);
  f.session.data.delete(SNAPSHOT_KEY); assert.equal(f.owner().open().carriedResidualBytes, 2048 * MiB);
});
