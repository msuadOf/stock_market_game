import assert from "node:assert/strict";
import test from "node:test";
import { WasmSessionSlot } from "./wasm-session-slot.ts";

test("分块初始恢复只消费对应 token 一次，失败不制造 generation", { timeout: 10000 }, () => {
  const calls: number[] = []; const slot = new WasmSessionSlot(() => ({ create_session() { throw new Error("不可 create"); }, restore_json() { throw new Error("不可 String"); }, restore_prepared_chunks(token: number) { calls.push(token); if (token !== 7) throw new Error("过期 token"); return 9; }, snapshot: () => ({}), drop_session() {} }));
  assert.throws(() => slot.restoreInitialChunked(8), /过期 token/);
  assert.deepEqual(calls, [8]); assert.equal(slot.readGeneration(), 0);
  assert.throws(() => slot.restoreInitialChunked(7), /一次/);
  const next = new WasmSessionSlot(() => ({ create_session() { throw new Error("不可 create"); }, restore_json() { throw new Error("不可 String"); }, restore_prepared_chunks(token: number) { calls.push(token); return 9; }, snapshot: () => ({}), drop_session() {} }));
  next.restoreInitialChunked(7); assert.equal(next.requireHandle()[0], 9); assert.equal(next.readGeneration(), 1);
  assert.throws(() => next.restoreInitial(), /一次/);
});

test("restoreInitial 只消费同一完整 X，一次 restore_prepared，无空 create 或旧 drop", { timeout: 10000 }, () => {
  const calls: string[] = [];
  const slot = new WasmSessionSlot(() => ({ create_session() { calls.push("create"); return 9; }, restore_json() { throw new Error("不可重复解码"); }, restore_prepared() { calls.push("consume"); return 7; }, snapshot: () => ({}), drop_session() { calls.push("drop"); } }));
  slot.restoreInitial();
  assert.deepEqual(calls, ["consume"]); assert.equal(slot.requireHandle()[0], 7); assert.equal(slot.readGeneration(), 1);
  assert.throws(() => slot.restoreInitial(), /一次|已有/);
  assert.deepEqual(calls, ["consume"]);
});
test("restoreInitial Rust 原错不改写为新局，不制造 handle/generation", { timeout: 10000 }, () => {
  const original = { code: "RESTORE_PREFIX", message: "prefix rejected" };
  const slot = new WasmSessionSlot(() => ({ create_session() { throw new Error("不可 create"); }, restore_json() { throw new Error("不可重复解码"); }, restore_prepared() { throw original; }, snapshot: () => ({}), drop_session() { throw new Error("不可 drop"); } }));
  assert.throws(() => slot.restoreInitial(), (error) => error === original);
  assert.equal(slot.readGeneration(), 0); assert.throws(() => slot.requireHandle(), /尚未就绪/);
  assert.throws(() => slot.restoreInitial(), /一次/);
});

test("初始恢复只消费已预检的同一份档案，不再次序列化", { timeout: 10000 }, () => {
  const calls: string[] = [];
  const slot = new WasmSessionSlot(() => ({
    create_session() { throw new Error("不可 create"); },
    restore_json() { throw new Error("不可重复解码"); },
    restore_prepared() { calls.push("consume"); return 7; },
    snapshot: () => ({}), drop_session() {},
  }));
  slot.restoreInitial();
  assert.deepEqual(calls, ["consume"]);
});

for (const initial of ["create", "restoreInitial"] as const) {
  test(`${initial} 后 drop 不重置初始恢复机会或 generation`, { timeout: 10000 }, () => {
    const calls: string[] = [];
    const slot = new WasmSessionSlot(() => ({
      create_session() { calls.push("create"); return 9; },
      restore_json() { calls.push("restore_json"); return 11; },
      restore_prepared() { calls.push("restore_prepared"); return 7; },
      snapshot: () => ({}),
      drop_session(handle: number) { calls.push(`drop:${handle}`); },
    }));
    if (initial === "create") slot.create({}, 42n);
    else slot.restoreInitial();
    slot.drop();
    assert.deepEqual(calls, initial === "create" ? ["create", "drop:9"] : ["restore_prepared", "drop:7"]);
    const before = [...calls];
    const generation = slot.readGeneration();
    assert.equal(generation, 1);
    assert.throws(() => slot.restoreInitial(), /首次空槽调用一次/);
    assert.deepEqual(calls, before, "drop 后禁止再次进入初始恢复 FFI");
    assert.equal(slot.readGeneration(), generation);
    assert.throws(() => slot.requireHandle(), /尚未就绪/);
  });
}

test("restoreInitial 在 bindings 未初始化时显式拒绝且不制造会话", { timeout: 10000 }, () => {
  const slot = new WasmSessionSlot(() => null);
  assert.throws(() => slot.restoreInitial(), /wasm 未初始化/);
  assert.equal(slot.readGeneration(), 0);
  assert.throws(() => slot.requireHandle(), /尚未就绪/);
});
