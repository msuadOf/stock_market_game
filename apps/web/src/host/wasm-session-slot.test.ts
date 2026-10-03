import assert from "node:assert/strict";
import test from "node:test";
import { WasmSessionSlot } from "./wasm-session-slot.ts";

async function fixture() {
  const calls: string[] = [];
  let candidate = 7;
  let failure: "create" | "snapshot" | "drop" | "prepare" | null = null;
  const bindings = {
    create_session: (_setup: unknown, seed: bigint) => { calls.push(`create:${seed}`); if (failure === "create") throw new Error("create failed"); return candidate; },
    restore_json: (encoded: string) => { calls.push(`restore:${encoded}`); return candidate; },
    snapshot: (handle: number) => { calls.push(`snapshot:${handle}`); if (failure === "snapshot") throw new Error("snapshot failed"); return { handle }; },
    drop_session: (handle: number) => { calls.push(`drop:${handle}`); if (failure === "drop") throw new Error("drop failed"); },
    prepare_public_baseline: (handle: number) => { calls.push(`prepare:${handle}`); if (failure === "prepare") throw new Error("prepare failed"); },
  };
  const slot = new WasmSessionSlot(() => bindings);
  const hooks = {
    wasRunning: true,
    stop: () => { calls.push("stop"); },
    restart: () => { calls.push("restart"); },
  };
  return { slot, bindings, calls, hooks, candidate: (next: number) => { candidate = next; }, fail: (next: typeof failure) => { failure = next; } };
}

test("WasmSessionSlot 维持未就绪、generation 校验与重复 create 覆盖边界", async () => {
  const f = await fixture();
  assert.equal(f.slot.readGeneration(), 0);
  assert.throws(() => f.slot.requireHandle(), /会话尚未就绪/);
  assert.throws(() => f.slot.requireGeneration(1), /已过期会话/);
  f.slot.create({}, 1n);
  assert.deepEqual(f.slot.requireHandle(), [7, f.bindings]);
  assert.equal(f.slot.requireGeneration(1), 1);
  f.candidate(9);
  f.slot.create({}, 2n);
  assert.equal(f.slot.readGeneration(), 2);
  assert.equal(f.slot.requireHandle()[0], 9);
  assert.deepEqual(f.calls, ["create:1", "create:2"]);
  f.fail("create");
  assert.throws(() => f.slot.create({}, 3n), /create failed/);
  assert.equal(f.slot.requireHandle()[0], 9);
  assert.equal(f.slot.readGeneration(), 2);
});

test("WasmSessionSlot create 的未初始化和 seed 校验保持原错误顺序", async () => {
  const slot = new WasmSessionSlot(() => null);
  assert.throws(() => slot.create({}, 1), /wasm 未初始化/);
  const f = await fixture();
  assert.throws(() => f.slot.create({}, 1), /seed 必须是 bigint/);
  assert.equal(f.slot.readGeneration(), 0);
});

test("WasmSessionSlot restore snapshot 失败清 candidate 并恢复运行意图", async () => {
  const f = await fixture();
  f.slot.create({}, 1n);
  f.calls.length = 0;
  f.candidate(9);
  f.fail("snapshot");
  assert.throws(() => f.slot.restore({ saved: true }, f.hooks), /snapshot failed/);
  assert.equal(f.slot.requireHandle()[0], 7);
  assert.equal(f.slot.readGeneration(), 1);
  assert.deepEqual(f.calls, ['stop', 'restore:{"saved":true}', 'snapshot:9', 'drop:9', 'restart']);
});

test("WasmSessionSlot restore 先 snapshot、交换 authority、drop 旧句柄再推进 generation", async () => {
  const f = await fixture();
  f.slot.create({}, 1n);
  f.calls.length = 0;
  f.candidate(9);
  const restored = f.slot.restore({ saved: true }, f.hooks);
  assert.deepEqual(restored, { handle: 9 });
  assert.equal(f.slot.requireHandle()[0], 9);
  assert.equal(f.slot.readGeneration(), 2);
  assert.deepEqual(f.calls, ['stop', 'restore:{"saved":true}', 'snapshot:9', 'drop:7', 'restart', 'prepare:9']);
});

test("WasmSessionSlot restore 旧 drop 失败保留新 handle 与旧 generation", async () => {
  const f = await fixture();
  f.slot.create({}, 1n);
  f.calls.length = 0;
  f.candidate(9);
  f.fail("drop");
  assert.throws(() => f.slot.restore({}, f.hooks), /drop failed/);
  assert.equal(f.slot.requireHandle()[0], 9);
  assert.equal(f.slot.readGeneration(), 1);
  assert.deepEqual(f.calls, ['stop', 'restore:{}', 'snapshot:9', 'drop:7', 'restart']);
});

test("WasmSessionSlot restore prepare 失败保留新 handle 与新 generation", async () => {
  const f = await fixture();
  f.slot.create({}, 1n);
  f.candidate(9);
  f.fail("prepare");
  assert.throws(() => f.slot.restore({}, { ...f.hooks, wasRunning: false }), /prepare failed/);
  assert.equal(f.slot.requireHandle()[0], 9);
  assert.equal(f.slot.readGeneration(), 2);
  assert.ok(!f.calls.includes("restart"));
});

test("WasmSessionSlot drop 失败保留句柄，成功才清空且不改变 generation", async () => {
  const f = await fixture();
  f.slot.drop();
  f.slot.create({}, 1n);
  f.fail("drop");
  assert.throws(() => f.slot.drop(), /drop failed/);
  assert.equal(f.slot.requireHandle()[0], 7);
  f.fail(null);
  f.slot.drop();
  assert.throws(() => f.slot.requireHandle(), /会话尚未就绪/);
  assert.equal(f.slot.readGeneration(), 1);
  const count = f.calls.length;
  f.slot.drop();
  assert.equal(f.calls.length, count);
});

test("WasmSessionSlot candidate 清理失败保留原 handle，沿用 finally 的 restart 未执行边界", async () => {
  const f = await fixture();
  f.slot.create({}, 1n);
  f.calls.length = 0;
  f.candidate(9);
  f.fail("snapshot");
  f.bindings.drop_session = (handle: number) => { f.calls.push(`drop:${handle}`); throw new Error("candidate cleanup failed"); };
  assert.throws(() => f.slot.restore({}, f.hooks), /candidate cleanup failed/);
  assert.equal(f.slot.requireHandle()[0], 7);
  assert.equal(f.slot.readGeneration(), 1);
  assert.deepEqual(f.calls, ["stop", "restore:{}", "snapshot:9", "drop:9"]);
});

test("WasmSessionSlot restore 构造失败不清未创建的 candidate，仍恢复运行意图", async () => {
  const f = await fixture();
  f.slot.create({}, 1n);
  f.calls.length = 0;
  f.bindings.restore_json = () => { throw new Error("restore failed"); };
  assert.throws(() => f.slot.restore({}, f.hooks), /restore failed/);
  assert.equal(f.slot.requireHandle()[0], 7);
  assert.equal(f.slot.readGeneration(), 1);
  assert.deepEqual(f.calls, ["stop", "restart"]);
});
