import assert from "node:assert/strict";
import test from "node:test";

import {
  BUMP, CAPACITY, FREE_HEAD, FREE_HEAD_EMPTY, FREE_LEN, HEADER_WORDS, JOIN_STRIDE, SLOT_JOIN_INDEX, SLOT_STRIDE,
  SlotState, JoinState, TombstoneCode, TOMBSTONE, RETIRED_FAIL_UNACKED,
  encodeJoin, encodeW, joinWordOffset, slotFieldOffset, slotWordOffset,
} from "./facade-slot-protocol.ts";
import {
  answerFail, answerOk, createSupervisorState, evaluateOkRecovery, onWorkerExit, readAccounts, readSlot,
  recoverPending, registerChild, writeDead, type FacadeArenaView,
} from "./facade-supervisor.ts";

// 与 Rust `facade_slot::test_supervisor` 同批不变量的 TS 镜像测（§13.1 A1-A3、
// §12.1.3 三门回收/延迟登记、§8.2.2 D5、§13.3 RA 账）。

const CAP = 4;
const GEN_BITS = 29;

function freshArena(): FacadeArenaView {
  const words = new Int32Array(HEADER_WORDS + CAP * SLOT_STRIDE + CAP * JOIN_STRIDE);
  // 头部字（arena.rs::alloc：空表哨兵 + 容量 + 位宽）。
  Atomics.store(words, FREE_HEAD, FREE_HEAD_EMPTY | 0);
  Atomics.store(words, CAPACITY, CAP);
  Atomics.store(words, GEN_BITS, GEN_BITS);
  return { words, capacity: CAP, genBits: GEN_BITS };
}

/** 模拟 H2-H4 发布（engine 侧；测试直接落字，按 arena.rs 写序）。 */
function publish(arena: FacadeArenaView, slot: number, gen: number): void {
  // 镜像 H2 bump 推进（真实分配器在 H1 锁内完成；测试直接落字）。
  Atomics.store(arena.words, BUMP, slot + 1);
  Atomics.store(arena.words, slotFieldOffset(slot, SLOT_JOIN_INDEX), slot);
  Atomics.store(arena.words, joinWordOffset(slot, arena.capacity), encodeJoin(gen, JoinState.Waiting, GEN_BITS) | 0);
  Atomics.store(arena.words, slotWordOffset(slot), encodeW(gen, SlotState.Req, GEN_BITS) | 0);
}

test("A1-A3 OK 全序：REQ→ANSWERING→OK，载荷 worker_id 落槽", () => {
  const arena = freshArena();
  publish(arena, 0, 0);
  assert.equal(answerOk(arena, 0, 0, 7), "answered");
  assert.equal(readSlot(arena, 0).state, SlotState.Ok);
  assert.equal(readSlot(arena, 0).gen, 0);
});

test("迟到应答：A1 期望失配 ⇒ 零槽写 + terminate 卫生", () => {
  const arena = freshArena();
  publish(arena, 0, 1);
  const late: number[] = [];
  const seams = { onMessageEntry() {}, onAfterAcquire() {}, onTerminateLateChild(id: number) { late.push(id); } };
  Atomics.store(arena.words, slotFieldOffset(0, 4), 0xBEEF | 0); // worker_id canary
  assert.equal(answerOk(arena, 0, 0, 9, seams), "dropped-late");
  assert.deepEqual(late, [9]);
  assert.equal(Atomics.load(arena.words, slotFieldOffset(0, 4)) >>> 0, 0xBEEF);
  assert.equal(readSlot(arena, 0).raw, encodeW(1, SlotState.Req, GEN_BITS));
});

test("FAIL 应答计入 RA 账", () => {
  const arena = freshArena();
  publish(arena, 0, 0);
  assert.equal(answerFail(arena, 0, 0, 0xC0DE), "answered");
  assert.equal(Atomics.load(arena.words, RETIRED_FAIL_UNACKED), 1);
  assert.equal(readSlot(arena, 0).state, SlotState.Fail);
});

test("OK 回收三门：join 未 CONSUMED 不回收，闭合后 push，H1 占用转延迟", () => {
  const arena = freshArena();
  const state = createSupervisorState();
  publish(arena, 0, 0);
  answerOk(arena, 0, 0, 1);
  assert.equal(onWorkerExit(arena, state, 1, 0, 0), "gates-not-met");
  assert.equal(Atomics.load(arena.words, FREE_LEN), 0);
  // join → CONSUMED。
  Atomics.store(arena.words, joinWordOffset(0, arena.capacity), encodeJoin(0, JoinState.Consumed, GEN_BITS) | 0);
  assert.equal(onWorkerExit(arena, state, 1, 0, 0), "pushed");
  assert.equal(Atomics.load(arena.words, FREE_LEN), 1);
  assert.equal(Atomics.load(arena.words, FREE_HEAD) >>> 0, 0);
  // 第二个槽：预持 H1 锁 ⇒ 延迟登记；释放后检查点重试。
  publish(arena, 1, 0);
  answerOk(arena, 1, 0, 2);
  registerChild(state, 2, 1, 0);
  Atomics.store(arena.words, joinWordOffset(1, arena.capacity), encodeJoin(0, JoinState.Consumed, GEN_BITS) | 0);
  Atomics.store(arena.words, 0, 1); // H1_LOCK
  assert.equal(onWorkerExit(arena, state, 2, 1, 0), "delayed");
  assert.equal(state.pendingRecovery.length, 1);
  Atomics.store(arena.words, 0, 0);
  assert.equal(recoverPending(arena, state), 1);
  assert.equal(Atomics.load(arena.words, FREE_LEN), 2);
});

test("未知 workerExit 只丢弃，不写槽", () => {
  const arena = freshArena();
  const state = createSupervisorState();
  publish(arena, 0, 0);
  answerOk(arena, 0, 0, 3);
  assert.equal(onWorkerExit(arena, state, 999, 0, 0), "gates-not-met");
  assert.equal(state.manifest.has(999), false);
});

test("D5 死亡信号：WAITING→DEAD；已 CONSUMED 的迟到写丢弃", () => {
  const arena = freshArena();
  publish(arena, 0, 0);
  assert.equal(writeDead(arena, 0, 0, "hardtrap", 0xD5), "written");
  Atomics.store(arena.words, joinWordOffset(0, arena.capacity), encodeJoin(0, JoinState.Consumed, GEN_BITS) | 0);
  assert.equal(writeDead(arena, 0, 0, "panic", 1), "lost-to-consumed");
});

test("A1→A3 间出现协议外写者 ⇒ 先落 tombstone 再抛", () => {
  const arena = freshArena();
  publish(arena, 0, 0);
  // SEAM-C1 位于 A1 成功后、A2/A3 前——在此注入协议外写者。
  const seams = {
    onMessageEntry() {},
    onAfterAcquire(slot: number) {
      Atomics.store(arena.words, slotWordOffset(slot), encodeW(0, SlotState.Cancelled, GEN_BITS) | 0);
    },
    onTerminateLateChild() {},
  };
  assert.throws(() => answerOk(arena, 0, 0, 1, seams), /tombstone/);
  assert.equal(Atomics.load(arena.words, TOMBSTONE) >>> 0, TombstoneCode.UnexpectedWriter);
});

test("四账快照镜像 Rust accounts：健康回收后恒等式成立", () => {
  const arena = freshArena();
  const state = createSupervisorState();
  const a0 = readAccounts(arena);
  assert.ok(a0 !== null);
  assert.equal(a0.unallocated, CAP);
  assert.equal(a0.active, 0);
  // 发布→OK→join CONSUMED→回收。
  publish(arena, 0, 0);
  answerOk(arena, 0, 0, 1);
  registerChild(state, 1, 0, 0);
  Atomics.store(arena.words, joinWordOffset(0, arena.capacity), encodeJoin(0, JoinState.Consumed, GEN_BITS) | 0);
  assert.equal(onWorkerExit(arena, state, 1, 0, 0), "pushed");
  const a1 = readAccounts(arena);
  assert.ok(a1 !== null, "回收后四账恒等式必须成立");
  assert.equal(a1.free, 1);
  assert.equal(a1.unallocated, CAP - 1);
  assert.equal(a1.active, 0);
  // FAIL 未 ACK ⇒ RA=1，恒等式仍平。
  publish(arena, 1, 0);
  answerFail(arena, 1, 0, 7);
  const a2 = readAccounts(arena);
  assert.ok(a2 !== null);
  assert.equal(a2.retiredFailUnacked, 1);
  assert.equal(a2.active, 0);
});

test("门 (c)：子仍 ACTIVE 时不得回收（复核修正项）", () => {
  const arena = freshArena();
  const state = createSupervisorState();
  publish(arena, 0, 0);
  answerOk(arena, 0, 0, 1);
  registerChild(state, 1, 0, 0);
  // join 已 CONSUMED（门 b 闭合），但 manifest 仍是 ACTIVE。
  Atomics.store(arena.words, joinWordOffset(0, arena.capacity), encodeJoin(0, JoinState.Consumed, GEN_BITS) | 0);
  assert.equal(evaluateOkRecovery(arena, state, 0, 0), "gates-not-met",
    "子仍活跃时槽不得回收（门 c 未闭合）");
  assert.equal(Atomics.load(arena.words, FREE_LEN), 0, "不得入空闲表");
  // workerExit 摘除后 ⇒ 三门齐全 ⇒ 可回收。
  assert.equal(onWorkerExit(arena, state, 1, 0, 0), "pushed");
  assert.equal(Atomics.load(arena.words, FREE_LEN), 1);
});

test("迟到 D5 撞换代：过期即弃，不崩不落 tombstone", () => {
  const arena = freshArena();
  publish(arena, 0, 0);
  // join 槽已复用为新一代 (1,WAITING)。
  Atomics.store(arena.words, joinWordOffset(0, arena.capacity), encodeJoin(1, JoinState.Waiting, GEN_BITS) | 0);
  assert.equal(writeDead(arena, 0, 0, "hardtrap", 0xD5), "lost-to-stale-generation");
  assert.equal(Atomics.load(arena.words, TOMBSTONE), 0, "迟到 D5 不得落 tombstone");
  assert.equal(Atomics.load(arena.words, joinWordOffset(0, arena.capacity)) >>> 0, encodeJoin(1, JoinState.Waiting, GEN_BITS));
});
