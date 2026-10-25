/**
 * 槽协议共享内存布局与 W 编解码的 TS 镜像（R4a 阶段 2，设计 §12.1/§13.1/§14.1）。
 *
 * **单一事实源 = Rust** `packages/engine/src/facade_slot/{arena,codec}.rs`；
 * 本文件逐字镜像偏移/状态码/位宽，漂移由 `facade-slot-protocol.test.ts`
 * 解析 Rust 源常量做跨语言 tripwire（任一侧改动未同步即红）。
 *
 * 共享字区 = 均一 u32 原子字（`Int32Array` 视图——无符号语义在读取侧
 * `>>> 0` 还原）；主线程监督者只做 CAS/notify，永不 `Atomics.wait`
 * （WebAssembly/threads#106）。
 */

// —— 头部字偏移（arena.rs）——
export const H1_LOCK = 0;
export const H1_WAITERS = 1;
export const BUMP = 2;
export const FREE_HEAD = 3;
export const FREE_HEAD_EMPTY = 0xffffffff;
export const FREE_LEN = 4;
export const RETIRED_FAIL_UNACKED = 5;
export const RETIRED_GEN_EXHAUSTED = 6;
export const TOMBSTONE = 7;
export const TIMEOUT_HIT_ANSWERING = 8;
export const CAPACITY = 9;
export const GEN_BITS = 10;
export const FACADE_RA_TOTAL = 11;
export const FACADE_RB_TOTAL = 12;
export const HEADER_WORDS = 16;

// —— 请求槽字段（步长 7）——
export const SLOT_STRIDE = 7;
export const SLOT_W = 0;
export const SLOT_PAYLOAD_LO = 1;
export const SLOT_PAYLOAD_HI = 2;
export const SLOT_JOIN_INDEX = 3;
export const SLOT_ANSWER_A = 4;
export const SLOT_ANSWER_B = 5;
export const SLOT_FREE_NEXT = 6;

// —— join 槽字段（步长 4）——
export const JOIN_STRIDE = 4;
export const JOIN_W = 0;
export const JOIN_RESULT_LO = 1;
export const JOIN_RESULT_HI = 2;
export const JOIN_DEAD_CODE = 3;

/** 默认 gen 位宽（codec.rs）。 */
export const GEN_BITS_DEFAULT = 29;

/** 请求槽状态（§13.1 重排后的数值；严格递增，I3 无 ABA）。 */
export const SlotState = Object.freeze({
  Idle: 0,
  Req: 1,
  Answering: 2,
  Ok: 3,
  Fail: 4,
  Cancelled: 5,
  FailAcked: 6,
  Retired: 7,
} as const);

/** join 槽状态（§9.2.3 单向状态机）。 */
export const JoinState = Object.freeze({
  Waiting: 0,
  Done: 1,
  DeadHardtrap: 2,
  DeadPanic: 3,
  Consumed: 4,
} as const);

/** tombstone 结构化码（arena.rs::TombstoneCode）。 */
export const TombstoneCode = Object.freeze({
  AllocExpectedMismatch: 1,
  FreeListCorrupt: 2,
  JoinResetMismatch: 3,
  RequesterGenerationAdvanced: 4,
  TrampolinePayloadTripwire: 5,
  UnexpectedWriter: 6,
} as const);

/** 请求槽基址（字索引）。 */
export function slotBase(slotIndex: number, capacity: number): number {
  void capacity; // 请求槽区从头部之后开始，与容量无关（join 区才依赖容量）。
  return HEADER_WORDS + slotIndex * SLOT_STRIDE;
}

/** join 槽基址（字索引；join 区在请求区之后）。 */
export function joinBase(joinIndex: number, capacity: number): number {
  return HEADER_WORDS + capacity * SLOT_STRIDE + joinIndex * JOIN_STRIDE;
}

/** 请求槽 W 字偏移（Int32Array 下标）。 */
export function slotWordOffset(slotIndex: number): number {
  return HEADER_WORDS + slotIndex * SLOT_STRIDE + SLOT_W;
}

/** 请求槽任意字段偏移（field = SLOT_* 常量）。 */
export function slotFieldOffset(slotIndex: number, field: number): number {
  return HEADER_WORDS + slotIndex * SLOT_STRIDE + field;
}

/** join 槽 W 字偏移。 */
export function joinWordOffset(joinIndex: number, capacity: number): number {
  return joinBase(joinIndex, capacity) + JOIN_W;
}

/** join 槽任意字段偏移（field = JOIN_* 常量）。 */
export function joinFieldOffset(joinIndex: number, capacity: number, field: number): number {
  return joinBase(joinIndex, capacity) + field;
}

/** 应答载荷字段偏移（arena.rs::AnswerField）。 */
export const AnswerField = Object.freeze({
  WorkerId: SLOT_ANSWER_A,
  Errcode: SLOT_ANSWER_B,
} as const);

/** join 槽状态解码。 */
export function decodeJoinState(w: number, genBits: number = GEN_BITS_DEFAULT): number {
  return decodeState(w, genBits);
}

/** join 复合字（与请求槽同一布局，§9.2.3）。 */
export function encodeJoin(gen: number, state: number, genBits: number = GEN_BITS_DEFAULT): number {
  return encodeW(gen, state, genBits);
}

/** W = gen << (32 − gen_bits) | state（§12.1.1）。 */
export function encodeW(gen: number, state: number, genBits: number = GEN_BITS_DEFAULT): number {
  assertU32(gen, "gen");
  const shifted = gen * 2 ** (32 - genBits);
  if (!Number.isSafeInteger(shifted)) throw new Error(`encodeW: gen ${gen} 超出 gen_bits=${genBits} 域`);
  return (shifted | state) >>> 0;
}

/** 解码 gen（高位）。 */
export function decodeGen(w: number, genBits: number = GEN_BITS_DEFAULT): number {
  return Math.floor((w >>> 0) / 2 ** (32 - genBits));
}

/** 解码状态（低位）。 */
export function decodeState(w: number, genBits: number = GEN_BITS_DEFAULT): number {
  return (w >>> 0) % 2 ** (32 - genBits);
}

function assertU32(value: number, label: string): void {
  if (!Number.isInteger(value) || value < 0 || value > 0xffffffff) {
    throw new Error(`${label} 必须是 u32 域整数，实际 ${value}`);
  }
}

// 布局常量自检（模块加载期；与 Rust 静态断言同式）。
if (HEADER_WORDS !== 16 || SLOT_STRIDE !== 7 || JOIN_STRIDE !== 4 || GEN_BITS_DEFAULT !== 29) {
  throw new Error("facade-slot-protocol 布局常量与 Rust 侧不符（模块加载期自检失败）");
}
