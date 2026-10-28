import assert from "node:assert/strict";
import test from "node:test";
import { BUMP, CAPACITY, GEN_BITS, FREE_HEAD, FREE_HEAD_EMPTY, HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE, SLOT_JOIN_INDEX, H1_LOCK, SlotState, JoinState, encodeW, encodeJoin, slotWordOffset, slotFieldOffset, joinWordOffset } from "./facade-slot-protocol.ts";
import { answerOk, createSupervisorState, onWorkerExit, recoverPending, registerChild } from "./facade-supervisor.ts";

function fixture() {
  const words = new Int32Array(new SharedArrayBuffer(4 * (HEADER_WORDS + 2 * (SLOT_STRIDE + JOIN_STRIDE))));
  Atomics.store(words, CAPACITY, 2); Atomics.store(words, GEN_BITS, 29); Atomics.store(words, FREE_HEAD, FREE_HEAD_EMPTY | 0);
  Atomics.store(words, BUMP, 1); Atomics.store(words, slotFieldOffset(0, SLOT_JOIN_INDEX), 0);
  Atomics.store(words, slotWordOffset(0), encodeW(0, SlotState.Req, 29) | 0);
  const arena = { words, capacity: 2, genBits: 29 }, state = createSupervisorState();
  registerChild(state, 1, 0, 0); answerOk(arena, 0, 0, 1);
  Atomics.store(words, joinWordOffset(0, 2), encodeJoin(0, JoinState.Consumed, 29) | 0);
  Atomics.store(words, H1_LOCK, 1); onWorkerExit(arena, state, 1, 0, 0); Atomics.store(words, H1_LOCK, 0);
  return { arena, state, words };
}

test("pending 输出仅追加真实 pushed pair，返回 count/旧调用兼容且 H1 已释放", { timeout: 10000 }, () => {
  const f = fixture(), recovered: Array<readonly [number, number]> = [];
  assert.equal(recoverPending(f.arena, f.state, recovered), 1);
  assert.deepEqual(recovered, [[0, 0]]); assert.equal(Atomics.load(f.words, H1_LOCK), 0);
  assert.equal(recoverPending(f.arena, f.state), 0);
  assert.equal(recoverPending(f.arena, f.state, recovered), 0); assert.deepEqual(recovered, [[0, 0]]);
});

test("pending 旧代不得输出新代 pair，也不改新代 manifest", { timeout: 10000 }, () => {
  const f = fixture(), recovered: Array<readonly [number, number]> = [];
  f.state.manifest.delete(1); registerChild(f.state, 2, 0, 1);
  Atomics.store(f.words, slotWordOffset(0), encodeW(1, SlotState.Req, 29) | 0);
  Atomics.store(f.words, joinWordOffset(0, 2), encodeJoin(1, JoinState.Waiting, 29) | 0);
  assert.equal(recoverPending(f.arena, f.state, recovered), 0); assert.deepEqual(recovered, []);
  assert.deepEqual(f.state.manifest.get(2), { slot: 0, gen: 1, status: "active" });
});
