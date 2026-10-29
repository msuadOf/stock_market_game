import assert from "node:assert/strict";
import test from "node:test";
import {
  WASM_MEMORY_MAXIMUM_BYTES, WASM_MEMORY_RECLAIM_BYTES,
  evaluateWasmMemoryBudget, addWasmMemoryBytes, rebuildWasmResidualBytes,
} from "./wasm-memory-budget.ts";

const MiB = 1024 * 1024;
const maximumBytes = 2048 * MiB;
const reclaimBytes = 512 * MiB;
const input = (carryBase = 0, sessionResident = 0, liveUpperBoundBytes: number | null = 768 * MiB) => ({
  maximumBytes, reclaimBytes, liveUpperBoundBytes, carryBase, sessionResident, liveBytes: 0,
});

test("历史算法fixture仍按2GiB / 512MiB计算，缺少可信 live 上界不准入", () => {
  assert.equal(WASM_MEMORY_MAXIMUM_BYTES, 3072 * MiB);
  assert.equal(WASM_MEMORY_RECLAIM_BYTES, reclaimBytes);
  assert.equal(evaluateWasmMemoryBudget(input(0, 0, null), "admission").status, "NOT_READY");
});

test("carry、live 与 drop 终值只累加，输入不接 GC 或 ACK", () => {
  let resident = 0;
  for (const terminal of [64, 128, 256]) resident = addWasmMemoryBytes(resident, terminal * MiB);
  const state = { ...input(512 * MiB, resident), liveBytes: 128 * MiB };
  const before = structuredClone(state);
  const verdict = evaluateWasmMemoryBudget(state, "admission");
  assert.equal(verdict.accountedBytes, (512 + 448 + 128) * MiB);
  assert.deepEqual(state, before);
  assert.equal(addWasmMemoryBytes(0, 0), 0);
});

test("drop 在 R_eff 两侧和恰等阈值分别继续与 reload", () => {
  assert.equal(evaluateWasmMemoryBudget(input(0, reclaimBytes - 1), "drop").status, "READY");
  for (const residual of [reclaimBytes, reclaimBytes + 1]) {
    assert.equal(evaluateWasmMemoryBudget(input(0, residual), "drop").status, "RELOAD");
  }
});

test("非整除收缩链按 512、512、256MiB reload，顶点首 drop 耗尽且不 reload", () => {
  let carry = 0;
  for (const residual of [512, 512, 256]) {
    const result = evaluateWasmMemoryBudget(input(carry, residual * MiB), "drop");
    assert.equal(result.thresholdEffective, Math.min(reclaimBytes, maximumBytes - carry - 768 * MiB));
    assert.equal(result.status, "RELOAD");
    carry = addWasmMemoryBytes(carry, residual * MiB);
  }
  assert.equal(carry, 1280 * MiB);
  assert.equal(evaluateWasmMemoryBudget(input(carry), "admission").status, "READY");
  assert.equal(evaluateWasmMemoryBudget(input(carry, 1), "drop").status, "EXHAUSTED");
  assert.equal(evaluateWasmMemoryBudget(input(carry, 0), "drop").status, "EXHAUSTED");
});

test("整除边界允许恰等准入，过冲直接 EXHAUSTED，起步超线亦耗尽", () => {
  assert.equal(evaluateWasmMemoryBudget(input(1024 * MiB, 0, 1024 * MiB), "admission").status, "READY");
  assert.equal(evaluateWasmMemoryBudget(input(1200 * MiB, 81 * MiB), "drop").status, "EXHAUSTED");
  assert.equal(evaluateWasmMemoryBudget(input(1280 * MiB + 1), "admission").status, "EXHAUSTED");
  assert.equal(evaluateWasmMemoryBudget({ ...input(), liveBytes: maximumBytes + 1 }, "admission").status, "BUDGET_EXCEEDED");
});

test("重建使用 highWater + 收缩补足 + live 上界，并保序封顶，不用 reloadCount", () => {
  assert.equal(rebuildWasmResidualBytes(512 * MiB, input()), 1792 * MiB);
  assert.equal(rebuildWasmResidualBytes(1536 * MiB, input(0, 0, 256 * MiB)), maximumBytes);
  assert.equal(rebuildWasmResidualBytes(maximumBytes, input()), maximumBytes);
  assert.equal(rebuildWasmResidualBytes(0, input(0, 0, null)), null);
});

test("所有 bytes 都拒绝错误单位、负数、非整数、溢出与无效上界", () => {
  for (const bad of [-1, 1.5, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1, "512MiB"]) {
    assert.throws(() => evaluateWasmMemoryBudget({ ...input(), carryBase: bad as number }, "admission"), /carryBase/);
    assert.throws(() => addWasmMemoryBytes(0, bad as number), /bytes/);
  }
  assert.throws(() => addWasmMemoryBytes(Number.MAX_SAFE_INTEGER, 1), /溢出/);
  assert.throws(() => evaluateWasmMemoryBudget(input(0, 0, 0), "admission"), /liveUpperBoundBytes/);
  assert.throws(() => evaluateWasmMemoryBudget(input(0, 0, maximumBytes), "admission"), /live 上界/);
  assert.throws(() => evaluateWasmMemoryBudget({ ...input(), liveBytes: 1 }, "drop"), /liveBytes/);
  assert.throws(() => evaluateWasmMemoryBudget(input(), "unknown" as "drop"), /phase/);
  assert.throws(() => evaluateWasmMemoryBudget({ ...input(), carryBase: Number.MAX_SAFE_INTEGER, sessionResident: 1 }, "admission"), /溢出/);
});


test("本次已选局级 live 上界 256MiB 在 carry=1536MiB 时准入", () => {
  const selected = input(1536 * MiB, 0, 256 * MiB);
  assert.equal(selected.liveBytes, 0);
  assert.equal(evaluateWasmMemoryBudget(selected, "admission").status, "READY");
});

test("同一 carry 与 actual live 下已选上界 768MiB 仍耗尽，actual=0 不替代峰值", () => {
  const selected = input(1536 * MiB, 0, 768 * MiB);
  assert.equal(selected.liveBytes, 0);
  assert.equal(evaluateWasmMemoryBudget(selected, "admission").status, "EXHAUSTED");
});
