import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  CAPACITY, FREE_HEAD, FREE_LEN, HEADER_WORDS, H1_LOCK, H1_WAITERS, JOIN_DEAD_CODE,
  JOIN_RESULT_HI, JOIN_RESULT_LO, JOIN_STRIDE, JOIN_W, RETIRED_FAIL_UNACKED,
  RETIRED_GEN_EXHAUSTED, SLOT_ANSWER_A, SLOT_ANSWER_B, SLOT_FREE_NEXT,
  SLOT_JOIN_INDEX, SLOT_PAYLOAD_HI, SLOT_PAYLOAD_LO, SLOT_STRIDE, SLOT_W,
  TIMEOUT_HIT_ANSWERING, TOMBSTONE, GEN_BITS, GEN_BITS_DEFAULT,
  JoinState, SlotState, TombstoneCode, decodeGen, decodeState, encodeW, joinBase, slotBase,
} from "./facade-slot-protocol.ts";

// 跨语言 tripwire（设计 §12.1：Rust 为单一事实源，TS 侧逐字镜像）。
// 解析 Rust 源常量并与本文件导出比对——任一侧漂移未同步即红。

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../../..");
const arenaRs = readFileSync(path.join(repoRoot, "packages/engine/src/facade_slot/arena.rs"), "utf8");
const codecRs = readFileSync(path.join(repoRoot, "packages/engine/src/facade_slot/codec.rs"), "utf8");

function rustConst(source: string, name: string): number {
  const match = new RegExp(`pub const ${name}: usize = (\\d+);`).exec(source);
  if (match === null) throw new Error(`Rust 源缺少 pub const ${name}`);
  return Number(match[1]);
}

function rustEnumValue(source: string, enumName: string, variant: string): number {
  const body = new RegExp(`pub enum ${enumName} \\{([\\s\\S]*?)\\n\\}`).exec(source);
  if (body === null) throw new Error(`Rust 源缺少 enum ${enumName}`);
  const match = new RegExp(`\\b${variant} = (\\d+)`).exec(body[1]);
  if (match === null) throw new Error(`Rust enum ${enumName} 缺少 ${variant}`);
  return Number(match[1]);
}

test("头部/槽字段偏移与 Rust arena.rs 逐字一致", () => {
  const pairs: ReadonlyArray<readonly [string, number]> = [
    ["H1_LOCK", H1_LOCK], ["H1_WAITERS", H1_WAITERS], ["FREE_HEAD", FREE_HEAD],
    ["FREE_LEN", FREE_LEN], ["RETIRED_FAIL_UNACKED", RETIRED_FAIL_UNACKED],
    ["RETIRED_GEN_EXHAUSTED", RETIRED_GEN_EXHAUSTED], ["TOMBSTONE", TOMBSTONE],
    ["TIMEOUT_HIT_ANSWERING", TIMEOUT_HIT_ANSWERING], ["CAPACITY", CAPACITY],
    ["GEN_BITS", GEN_BITS], ["HEADER_WORDS", HEADER_WORDS], ["SLOT_STRIDE", SLOT_STRIDE],
    ["SLOT_W", SLOT_W], ["SLOT_PAYLOAD_LO", SLOT_PAYLOAD_LO], ["SLOT_PAYLOAD_HI", SLOT_PAYLOAD_HI],
    ["SLOT_JOIN_INDEX", SLOT_JOIN_INDEX], ["SLOT_ANSWER_A", SLOT_ANSWER_A],
    ["SLOT_ANSWER_B", SLOT_ANSWER_B], ["SLOT_FREE_NEXT", SLOT_FREE_NEXT],
    ["JOIN_STRIDE", JOIN_STRIDE], ["JOIN_W", JOIN_W], ["JOIN_RESULT_LO", JOIN_RESULT_LO],
    ["JOIN_RESULT_HI", JOIN_RESULT_HI], ["JOIN_DEAD_CODE", JOIN_DEAD_CODE],
  ];
  for (const [name, mirrored] of pairs) {
    assert.equal(rustConst(arenaRs, name), mirrored, `${name} 与 Rust 源不符`);
  }
});

test("状态码与 Rust codec.rs 逐字一致（I3 严格递增）", () => {
  for (const [variant, mirrored] of Object.entries(SlotState)) {
    assert.equal(rustEnumValue(codecRs, "SlotState", variant), mirrored, `SlotState::${variant}`);
  }
  for (const [variant, mirrored] of Object.entries(JoinState)) {
    assert.equal(rustEnumValue(codecRs, "JoinState", variant), mirrored, `JoinState::${variant}`);
  }
  for (const [variant, mirrored] of Object.entries(TombstoneCode)) {
    assert.equal(rustEnumValue(arenaRs, "TombstoneCode", variant), mirrored, `TombstoneCode::${variant}`);
  }
  // I3：请求槽状态转移表严格递增（分配 H4 的 (g→g+1,REQ) 由 gen 位支配）。
  const order = [SlotState.Idle, SlotState.Req, SlotState.Answering, SlotState.Ok,
    SlotState.Cancelled, SlotState.FailAcked];
  for (let i = 1; i < order.length; i += 1) assert.ok(order[i] > order[i - 1]);
  assert.ok(SlotState.Fail > SlotState.Answering);
  assert.ok(SlotState.FailAcked > SlotState.Fail);
});

test("W 编解码往返且位域不重叠", () => {
  assert.equal(GEN_BITS_DEFAULT, 29);
  for (const [gen, state] of [[0, 0], [0, 1], [1, 1], [15, 3], [0x1fffffff, 7]] as const) {
    const w = encodeW(gen, state);
    assert.equal(decodeGen(w), gen);
    assert.equal(decodeState(w), state);
  }
  // state 占低 3 位：gen=1,state=1 ⇒ 9（不是 2）。
  assert.equal(encodeW(1, 1), 9);
  // 缩空间（测试 cfg）：gen_bits=4 ⇒ 低 28 位是 state 域。
  assert.equal(encodeW(3, 5, 4), 3 * 2 ** 28 + 5);
});

test("槽/join 基址与 Rust 布局公式一致", () => {
  assert.equal(slotBase(0, 8), HEADER_WORDS);
  assert.equal(slotBase(2, 8), HEADER_WORDS + 2 * SLOT_STRIDE);
  assert.equal(joinBase(0, 8), HEADER_WORDS + 8 * SLOT_STRIDE);
  assert.equal(joinBase(3, 8), HEADER_WORDS + 8 * SLOT_STRIDE + 3 * JOIN_STRIDE);
});
