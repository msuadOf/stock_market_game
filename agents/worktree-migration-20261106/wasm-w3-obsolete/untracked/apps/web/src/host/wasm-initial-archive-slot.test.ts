import assert from "node:assert/strict";
import test from "node:test";
import { WasmSessionSlot } from "./wasm-session-slot.ts";

function fixture() {
  const calls: string[] = [];
  let failed: Error | null = null;
  const bindings = {
    create_session: () => { calls.push("create"); return 7; },
    restore_json: (text: string) => { calls.push(`restore:${text}`); if (failed !== null) throw failed; return 11; },
    snapshot: () => { calls.push("snapshot"); return {}; },
    drop_session: () => { calls.push("drop"); },
  };
  const slot = new WasmSessionSlot(() => bindings);
  return { slot, bindings, calls, fail: (error: Error) => { failed = error; } };
}
test("新鲜slot直接restore唯一目标，不先create空局或构造旧局candidate", () => {
  const f = fixture(); const archive = { selected: "X" };
  f.slot.restoreInitial(archive);
  assert.deepEqual(f.calls, ['restore:{"selected":"X"}']);
  assert.deepEqual(f.slot.requireHandle(), [11, f.bindings]);
  assert.equal(f.slot.requireGeneration(1), 1);
});
test("restoreInitial拒绝已有或已drop的World，不推进generation或调用FFI", () => {
  for (const first of ["create", "restore"] as const) {
    const f = fixture();
    if (first === "create") f.slot.create({}, 1n); else f.slot.restoreInitial({ selected: "X" });
    const initial = [...f.calls];
    assert.throws(() => f.slot.restoreInitial({ selected: "Y" }), /新鲜.*slot/);
    assert.deepEqual(f.calls, initial);
    f.slot.drop(); const afterDrop = [...f.calls];
    assert.throws(() => f.slot.restoreInitial({ selected: "Y" }), /新鲜.*slot/);
    assert.deepEqual(f.calls, afterDrop); assert.equal(f.slot.readGeneration(), 1);
  }
});
test("restoreInitial保留真实FFI失败，不伪造handle或drop未创建candidate", () => {
  const f = fixture(); const error = new Error("restore构造失败"); f.fail(error);
  assert.throws(() => f.slot.restoreInitial({ selected: "X" }), (caught) => caught === error);
  assert.deepEqual(f.calls, ['restore:{"selected":"X"}']);
  assert.equal(f.slot.readGeneration(), 0);
  assert.throws(() => f.slot.requireHandle(), /尚未就绪/);
});
test("restoreInitial在bindings不存在时显式拒绝", () => {
  const slot = new WasmSessionSlot(() => null);
  assert.throws(() => slot.restoreInitial({}), /wasm 未初始化/);
});
