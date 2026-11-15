/**
 * 槽协议生产监督者（主线程侧；R4a 阶段 2，设计 §12-14）。
 *
 * 职责（§13.1 A1-A3 / §12.1.3 回收权 / §10.2.2 W3-S / §8.2.2 D5）：
 * - 应答写权获取：**首次载荷 store 之前** CAS `(g,REQ)→(g,ANSWERING)`；
 *   失败 ⇒ 应答整体丢弃、对槽写次数 = 0，对迟到构造子尽 terminate 卫生；
 * - OK 回收三门：(a) 槽 `(g,OK)` ∧ (b) join `(g,CONSUMED)` ∧ (c) manifest 摘除；
 *   try-lock 快路径，失败转延迟登记（**主线程永不 Atomics.wait**）；
 * - workerExit → manifest 幂等摘除（只认 workerExit，与 self.close 无时序耦合）；
 * - D5 死亡信号写 join 槽（CAS 仲裁，输给 DONE/CONSUMED 时丢弃）。
 *
 * Rust 侧镜像 = `facade_slot::test_supervisor`（cfg(test) 测试角色驱动）；
 * 两侧断言同一批设计不变量，漂移由 `facade-slot-protocol.test.ts` 的跨语言
 * 常量 tripwire + 本文件的确定性测试兜底。
 *
 * 主线程约束：所有共享字访问 = `Atomics.load/store/compareExchange` +
 * `Atomics.notify`；禁止 `Atomics.wait`（WebAssembly/threads#106）。
 */
import {
  AnswerField,
  FACADE_RA_TOTAL,
  BUMP,
  FREE_HEAD,
  FREE_HEAD_EMPTY,
  FREE_LEN,
  H1_LOCK,
  H1_WAITERS,
  JOIN_DEAD_CODE,
  RETIRED_FAIL_UNACKED,
  RETIRED_GEN_EXHAUSTED,
  SLOT_FREE_NEXT,
  SLOT_JOIN_INDEX,
  SlotState,
  TOMBSTONE,
  TombstoneCode,
  JoinState,
  decodeGen,
  decodeJoinState,
  decodeState,
  encodeJoin,
  encodeW,
  joinFieldOffset,
  joinWordOffset,
  slotFieldOffset,
  slotWordOffset,
} from "./facade-slot-protocol.ts";

/** 共享字区视图（宿主对 wasm 线性内存建 Int32Array；整块 arena）。 */
export interface FacadeArenaView {
  readonly words: Int32Array;
  readonly capacity: number;
  readonly genBits: number;
}

/** 有符号 Int32 → u32 位型。 */
function u32(value: number): number {
  return value >>> 0;
}

export type AnswerOutcome = "answered" | "dropped-late";

/**
 * OK 应答 A1-A3（§13.1）。`genFromMsg` 取自 facadeSpawn 消息，**不读槽**
 * （§11.2 H5 纪律：读槽会把迟到消息冒充成本代）。
 */
export function answerOk(
  arena: FacadeArenaView,
  slotIndex: number,
  genFromMsg: number,
  workerId: number,
  seams: SupervisorSeams = NO_SEAMS,
): AnswerOutcome {
  seams.onMessageEntry(slotIndex); // SEAM-C0
  const reqW = encodeW(genFromMsg, SlotState.Req, arena.genBits);
  const answeringW = encodeW(genFromMsg, SlotState.Answering, arena.genBits);
  const wOffset = slotWordOffset(slotIndex);
  if (!compareExchange(arena.words, wOffset, reqW, answeringW)) {
    // A1 失败：槽已取消/换代/迟到 ⇒ 零槽写；迟到子 terminate（幂等）。
    seams.onTerminateLateChild(workerId);
    return "dropped-late";
  }
  seams.onAfterAcquire(slotIndex); // SEAM-C1
  // A2：槽被 A1 钉死（A4），写者唯一。
  Atomics.store(arena.words, slotFieldOffset(slotIndex, AnswerField.WorkerId), workerId | 0);
  // A3：CAS →(g,OK) + notify。
  const okW = encodeW(genFromMsg, SlotState.Ok, arena.genBits);
  if (!compareExchange(arena.words, wOffset, answeringW, okW)) {
    // A4：A1→A3 间唯一写者 = 本函数 ⇒ 不可达；显式 fail-stop 不静默。
    tombstone(arena, TombstoneCode.UnexpectedWriter, `A3 OK CAS 出现协议外写者（槽 ${slotIndex}，gen ${genFromMsg}）`);
  }
  Atomics.notify(arena.words, wOffset);
  return "answered";
}

/** FAIL 应答 A1-A3（同序；RA 计入 + facade 级累计，§13.3）。 */
export function answerFail(
  arena: FacadeArenaView,
  slotIndex: number,
  genFromMsg: number,
  errcode: number,
  seams: SupervisorSeams = NO_SEAMS,
): AnswerOutcome {
  seams.onMessageEntry(slotIndex);
  const reqW = encodeW(genFromMsg, SlotState.Req, arena.genBits);
  const answeringW = encodeW(genFromMsg, SlotState.Answering, arena.genBits);
  const wOffset = slotWordOffset(slotIndex);
  if (!compareExchange(arena.words, wOffset, reqW, answeringW)) {
    // spawn 未成立 ⇒ 无子、无 manifest 条目 ⇒ 无 terminate 义务。
    return "dropped-late";
  }
  seams.onAfterAcquire(slotIndex);
  // RA：必须在 A3 FAIL 发布 CAS **之前**入账（复核修正，与 Rust 侧同式）。
  // 若置于 notify 之后，engine 的 ACK 递减可先落 ⇒ u32 回绕成 0xFFFFFFFF，
  // 恒等式/Exhausted 载荷全脏且静默。置于 CAS 前 ⇒ "RA 计数 ≥ 未 ACK FAIL
  // 数"在任何观察点成立（A1 已领写权，递增不会因迟到路径泄漏）。
  Atomics.add(arena.words, RETIRED_FAIL_UNACKED, 1);
  Atomics.add(arena.words, FACADE_RA_TOTAL, 1);
  Atomics.store(arena.words, slotFieldOffset(slotIndex, AnswerField.Errcode), errcode | 0);
  const failW = encodeW(genFromMsg, SlotState.Fail, arena.genBits);
  if (!compareExchange(arena.words, wOffset, answeringW, failW)) {
    tombstone(arena, TombstoneCode.UnexpectedWriter, `A3 FAIL CAS 出现协议外写者（槽 ${slotIndex}，gen ${genFromMsg}）`);
  }
  Atomics.notify(arena.words, wOffset);
  return "answered";
}

/** 监督者簿记：manifest（worker 生命周期）与延迟回收登记。 */
export interface SupervisorState {
  readonly manifest: Map<number, ManifestEntry>;
  readonly pendingRecovery: Array<readonly [slot: number, gen: number]>;
}

export type ManifestStatus = "active" | "removed";

export interface ManifestEntry {
  readonly slot: number;
  readonly gen: number;
  status: ManifestStatus;
}

export function createSupervisorState(): SupervisorState {
  return { manifest: new Map(), pendingRecovery: [] };
}

/** 子 Worker 构造成功后的 manifest 登记（应答可达性前提）。 */
export function registerChild(
  state: SupervisorState,
  workerId: number,
  slot: number,
  gen: number,
): void {
  const existing = state.manifest.get(workerId);
  if (existing !== undefined && existing.status === "active") {
    throw new Error(`facade manifest 重复登记活跃 worker ${workerId}（槽 ${slot}，gen ${gen}）`);
  }
  state.manifest.set(workerId, { slot, gen, status: "active" });
}

export type RecoveryOutcome = "pushed" | "gates-not-met" | "delayed";

/**
 * workerExit（W3）处理：manifest 幂等摘除（ACTIVE→REMOVED，未知 id 只记
 * 诊断——§10.2 迟到消息交错表）→ OK 回收三门评估。
 */
export function onWorkerExit(
  arena: FacadeArenaView,
  state: SupervisorState,
  workerId: number,
  slot: number,
  gen: number,
): RecoveryOutcome {
  // 摘除只认 workerExit；未知 id 的迟到 workerExit 丢弃（不写槽）。
  const entry = state.manifest.get(workerId);
  if (entry === undefined || entry.slot !== slot || entry.gen !== gen) return "gates-not-met";
  if (entry.status === "active") {
    state.manifest.set(workerId, { slot: entry.slot, gen: entry.gen, status: "removed" });
  }
  return evaluateOkRecovery(arena, state, slot, gen);
}

/**
 * OK 回收三门（§10.2.1-4 + §12.1.3）：(a) 槽终态 OK ∧ (b) join CONSUMED
 * ∧ (c) manifest 已摘除。
 *
 * **门 (c) 检查（复核修正）**：此前只查 (a)(b)，子仍 ACTIVE 时槽即可被
 * 复用——设计 §10.2.1-4 的实质不变量"回收者必须确知一切旧写者已不存在"
 * 被破坏。判据 = 该 (slot,gen) 对应的 manifest 条目**不存在或已 removed**。
 * 从未登记该 pair 的情形（spawn 失败臂 / 检查点在登记前跑）按"无旧写者"
 * 通过——否则会永久卡在 gates-not-met。
 */
export function evaluateOkRecovery(
  arena: FacadeArenaView,
  state: SupervisorState,
  slot: number,
  gen: number,
): RecoveryOutcome {
  if (!recoveryGatesMet(arena, state, slot, gen)) return "gates-not-met";
  if (!tryLock(arena.words)) {
    if (!state.pendingRecovery.some(([s, g]) => s === slot && g === gen)) state.pendingRecovery.push([slot, gen]);
    return "delayed";
  }
  try {
    // H1前初检只用于快返；engine可已复用，持锁后必须重验完整门链。
    if (!recoveryGatesMet(arena, state, slot, gen)) return "gates-not-met";
    // 槽保持 OK 到下次分配；H1 内查询空闲表，防重复事件二次 push。
    let cursor = u32(Atomics.load(arena.words, FREE_HEAD));
    for (let walked = 0; cursor !== FREE_HEAD_EMPTY; walked += 1) {
      if (cursor >= arena.capacity || walked >= arena.capacity) {
        tombstone(arena, TombstoneCode.FreeListCorrupt, "回收遇到损坏空闲表");
      }
      if (cursor === slot) return "gates-not-met";
      cursor = u32(Atomics.load(arena.words, slotFieldOffset(cursor, SLOT_FREE_NEXT)));
    }
    pushFree(arena, slot);
    return "pushed";
  } finally { unlock(arena.words); }
}

/** 完整代、join消费与manifest门链；只有H1内二检可授权入free list。 */
function recoveryGatesMet(arena: FacadeArenaView, state: SupervisorState, slot: number, gen: number): boolean {
  const w = u32(Atomics.load(arena.words, slotWordOffset(slot)));
  if (w !== encodeW(gen, SlotState.Ok, arena.genBits)) return false;
  // join 槽与请求槽按 index 配对（H3 发布写 SLOT_JOIN_INDEX = 槽 index）。
  const joinIndex = u32(Atomics.load(arena.words, slotFieldOffset(slot, SLOT_JOIN_INDEX)));
  const joinOffset = joinWordOffset(joinIndex, arena.capacity);
  if (u32(Atomics.load(arena.words, joinOffset)) !== encodeJoin(gen, JoinState.Consumed, arena.genBits)) {
    return false;
  }
  // 门 (c)：仍存在 ACTIVE 的该 (slot,gen) 条目 ⇒ 子还活着，不得回收。
  for (const entry of state.manifest.values()) {
    if (entry.slot === slot && entry.gen === gen && entry.status === "active") {
      return false;
    }
  }
  return true;
}

/** 延迟回收检查点重试（下一 facadeSpawn / workerExit 事件驱动）。返回成功 push 数。 */
export function recoverPending(
  arena: FacadeArenaView,
  state: SupervisorState,
  recovered?: Array<readonly [slot: number, gen: number]>,
): number {
  let pushed = 0;
  // 先移走快照，评估期间只向下一批登记，绝不遍历仍在增长的数组。
  const pending = state.pendingRecovery.splice(0);
  for (const [slot, gen] of pending) {
    const outcome = evaluateOkRecovery(arena, state, slot, gen);
    if (outcome === "pushed") {
      pushed += 1;
      // evaluate 已经离开 H1 finally；这里只交出本次真实 push 的观测事实。
      recovered?.push([slot, gen]);
    }
  }
  return pushed;
}

export type DeadKind = "hardtrap" | "panic";
export type DeadOutcome =
  | "written"
  | "lost-to-dead"
  | "lost-to-done"
  | "lost-to-consumed"
  /** 输给换代：迟到的旧代死亡信号晚于槽复用到达——过期即弃，不崩。 */
  | "lost-to-stale-generation";

/** D5：join 槽死亡信号（§8.2.2）。CAS 仲裁，输家按位形分类。 */
export function writeDead(
  arena: FacadeArenaView,
  joinIndex: number,
  gen: number,
  kind: DeadKind,
  code: number,
): DeadOutcome {
  const offset = joinWordOffset(joinIndex, arena.capacity);
  const waitingW = encodeJoin(gen, JoinState.Waiting, arena.genBits);
  const deadW = encodeJoin(
    gen,
    kind === "hardtrap" ? JoinState.DeadHardtrap : JoinState.DeadPanic,
    arena.genBits,
  );
  const writingW = encodeJoin(gen, JoinState.DeadWriting, arena.genBits);
  if (compareExchange(arena.words, offset, waitingW, writingW)) {
    Atomics.store(arena.words, joinFieldOffset(joinIndex, arena.capacity, JOIN_DEAD_CODE), code | 0);
    if (!compareExchange(arena.words, offset, writingW, deadW)) {
      tombstone(arena, TombstoneCode.UnexpectedWriter, "D5 载荷写权被协议外写者破坏");
    }
    Atomics.notify(arena.words, offset);
    return "written";
  }
  // 先判代再判状态（复核修正，与 Rust 侧同式）：迟到的 g 代死亡信号撞上
  // 已复用为 (g+1,*) 的 join 槽是**合法**情形——过期即弃，不得崩。
  const now = u32(Atomics.load(arena.words, offset));
  if (decodeGen(now, arena.genBits) !== gen) return "lost-to-stale-generation";
  switch (decodeJoinState(now, arena.genBits)) {
    case JoinState.DeadWriting:
    case JoinState.DeadHardtrap:
    case JoinState.DeadPanic:
      return "lost-to-dead";
    case JoinState.Done:
      return "lost-to-done";
    case JoinState.Consumed:
      return "lost-to-consumed";
    default:
      tombstone(arena, TombstoneCode.UnexpectedWriter, `D5 CAS 输家位形非法（join ${joinIndex}，gen ${gen}）`);
  }
}

/** 诊断读取（宿主诊断面；不做协议决策）。 */
export function readSlot(arena: FacadeArenaView, slotIndex: number): { gen: number; state: number; raw: number } {
  const raw = u32(Atomics.load(arena.words, slotWordOffset(slotIndex)));
  return { gen: decodeGen(raw, arena.genBits), state: decodeState(raw, arena.genBits), raw };
}

/** 四账快照（§14.1 可用账口径；镜像 Rust `SlotArena::accounts`）。 */
export interface Accounts {
  readonly active: number;
  readonly free: number;
  readonly unallocated: number;
  readonly retiredFailUnacked: number;
  readonly retiredGenExhausted: number;
  readonly capacity: number;
}

/**
 * 四账快照 + 恒等式（静点核对：走查空闲表 + 逐槽分类 + 计数器对照）。
 * 镜像 Rust `SlotArena::accounts`（arena.rs）；不一致时返回 null 而非抛错
 * ——由调用点决定诊断/上报方式（本函数只作观测，不做协议决策）。
 */
export function readAccounts(arena: FacadeArenaView): Accounts | null {
  const capacity = arena.capacity;
  const inFree = new Uint8Array(capacity);
  let cursor = u32(Atomics.load(arena.words, FREE_HEAD));
  let walked = 0;
  while (cursor !== FREE_HEAD_EMPTY) {
    if (cursor >= capacity || inFree[cursor] !== 0) return null; // 越界或环
    inFree[cursor] = 1;
    walked += 1;
    cursor = u32(Atomics.load(arena.words, slotFieldOffset(cursor, SLOT_FREE_NEXT)));
  }
  if (walked !== u32(Atomics.load(arena.words, FREE_LEN))) return null;
  const bump = Math.min(u32(Atomics.load(arena.words, BUMP)), capacity);
  let active = 0;
  let ra = 0;
  let rb = 0;
  for (let i = 0; i < bump; i += 1) {
    if (inFree[i] !== 0) continue;
    const state = decodeState(u32(Atomics.load(arena.words, slotWordOffset(i))), arena.genBits);
    if (state === SlotState.Fail) ra += 1;
    else if (state === SlotState.Retired) rb += 1;
    else active += 1;
  }
  const accounts: Accounts = {
    active,
    free: walked,
    unallocated: capacity - bump,
    retiredFailUnacked: ra,
    retiredGenExhausted: rb,
    capacity,
  };
  // 恒等式：活跃 + 空闲 + 未划拨 + RA + RB ≡ 容量；计数器与扫描互证。
  if (active + walked + (capacity - bump) + ra + rb !== capacity) return null;
  if (ra !== u32(Atomics.load(arena.words, RETIRED_FAIL_UNACKED))) return null;
  if (rb !== u32(Atomics.load(arena.words, RETIRED_GEN_EXHAUSTED))) return null;
  return accounts;
}

// —— 内部：H1 try-lock（主线程永不等待）——

function tryLock(words: Int32Array): boolean {
  return Atomics.compareExchange(words, H1_LOCK, 0, 1) === 0;
}

function unlock(words: Int32Array): void {
  Atomics.store(words, H1_LOCK, 0);
  if (u32(Atomics.load(words, H1_WAITERS)) > 0) Atomics.notify(words, H1_LOCK);
}

function compareExchange(words: Int32Array, offset: number, expected: number, replacement: number): boolean {
  return u32(Atomics.compareExchange(words, offset, expected | 0, replacement | 0)) === u32(expected);
}

/** 终态 push 回空闲表（须持 H1；调用方保证槽为终态）。 */
function pushFree(arena: FacadeArenaView, slotIndex: number): void {
  const state = decodeState(u32(Atomics.load(arena.words, slotWordOffset(slotIndex))), arena.genBits);
  if (state !== SlotState.Ok && state !== SlotState.FailAcked && state !== SlotState.Cancelled) {
    tombstone(arena, TombstoneCode.FreeListCorrupt, `push_free 槽 ${slotIndex} 非终态（state=${state}）`);
  }
  const oldHead = u32(Atomics.load(arena.words, FREE_HEAD));
  Atomics.store(arena.words, slotFieldOffset(slotIndex, SLOT_FREE_NEXT), oldHead | 0);
  Atomics.store(arena.words, FREE_HEAD, slotIndex | 0);
  Atomics.add(arena.words, FREE_LEN, 1);
}

function tombstone(arena: FacadeArenaView, code: number, detail: string): never {
  Atomics.store(arena.words, TOMBSTONE, code);
  throw new Error(`facade_slot tombstone(${code})：${detail}`);
}

/** 测试缝（生产 = 全 no-op；与 Rust FacadePlatform 的 seam_* 对齐）。 */
export interface SupervisorSeams {
  onMessageEntry(slot: number): void;
  onAfterAcquire(slot: number): void;
  onTerminateLateChild(workerId: number): void;
}

export const NO_SEAMS: SupervisorSeams = {
  onMessageEntry() {},
  onAfterAcquire() {},
  onTerminateLateChild() {},
};
