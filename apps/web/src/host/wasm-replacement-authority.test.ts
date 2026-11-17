import assert from "node:assert/strict";
import test from "node:test";
import { WasmSessionSlot } from "./wasm-session-slot.ts";

function fixture(commitFailure = false) {
  const trace: string[] = [];
  const bindings = {
    create_session: () => 7,
    restore_json: () => 9,
    snapshot: (handle: number) => ({ handle }),
    prepare_public_baseline: () => {},
    drop_session: (handle: number) => { trace.push(`drop:${handle}`); },
    fence_session_replacement: () => {},
    prepare_session_replacement: (_previous: number, _candidate: number, _generation: bigint) => {},
    commit_session_replacement: (previous: number, candidate: number, generation: bigint) => {
      trace.push(`fence:${previous}:${candidate}:${generation}`);
      if (commitFailure) throw new Error("旧入口 fence 拒绝");
      return 2n;
    },
  };
  const slot = new WasmSessionSlot(() => bindings);
  slot.create({}, 1n);
  return { slot, bindings, trace };
}

test("恢复先经 Rust authority fence，返回的 generation 是唯一来源", { timeout: 10000 }, () => {
  const f = fixture();
  f.slot.restore({}, { wasRunning: false, stop() {}, restart() {} });
  assert.equal(f.slot.readGeneration(), 2);
  assert.equal(f.slot.requireHandle()[0], 9);
  assert.deepEqual(f.trace, ["fence:7:9:1", "drop:7"]);
});

test("commit fence 拒绝保留旧 handle/代际并释放 candidate", { timeout: 10000 }, () => {
  const f = fixture(true);
  assert.throws(() => f.slot.restore({}, { wasRunning: false, stop() {}, restart() {} }), /fence 拒绝/);
  assert.equal(f.slot.readGeneration(), 1);
  assert.equal(f.slot.requireHandle()[0], 7);
  assert.deepEqual(f.trace, ["fence:7:9:1", "drop:9"]);
});

 test("commit 后 prepare 失败不得重启或销毁新 owner", { timeout: 10000 }, () => {
  const f = fixture();
  f.bindings.prepare_public_baseline = () => { throw new Error("commit 后发布准备失败"); };
  let restarted = false;
  assert.throws(() => f.slot.restore({}, { wasRunning: true, stop() {}, restart() { restarted = true; } }), /发布准备失败/);
  assert.equal(f.slot.readGeneration(), 2);
  assert.equal(f.slot.requireHandle()[0], 9);
  assert.equal(restarted, false);
  assert.deepEqual(f.trace, ["fence:7:9:1", "drop:7"]);
});

test("prepare 拒绝不撤销旧 authority，prepared 只能消费一次", { timeout: 10000 }, () => {
  const f = fixture();
  const prepared = f.slot.prepareRestore({});
  assert.equal(f.slot.readGeneration(), 1);
  assert.deepEqual(f.trace, []);
  f.slot.restore({}, { wasRunning: false, stop() {}, restart() {} }, prepared);
  assert.throws(() => f.slot.restore({}, { wasRunning: false, stop() {}, restart() {} }, prepared), /已过期或已消费/);
  assert.equal(f.slot.readGeneration(), 2);
});

test("Drive prepare 失败仍保持旧 generation 且不会调用 commit", { timeout: 10000 }, () => {
  const f = fixture();
  f.bindings.prepare_session_replacement = () => { throw new Error("Drive 准备失败"); };
  assert.throws(() => f.slot.prepareRestore({}), /Drive 准备失败/);
  assert.equal(f.slot.readGeneration(), 1);
  assert.equal(f.slot.requireHandle()[0], 7);
  assert.deepEqual(f.trace, ["drop:9"]);
});
