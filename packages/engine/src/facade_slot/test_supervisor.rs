//! 监督者角色测试驱动（cfg(test)）——N 测试/生命周期测试的 Rust 侧
//! 监督者扮演面。**生产监督者 = 主线程 TS**（`apps/web/src/host/
//! facade-supervisor.ts`，经 Int32Array+Atomics 直接操作同一 W 字）；
//! 本模块与其断言同一批设计不变量（A1-A3 序、C0/C1 缝、门链、延迟回收、
//! D5），漂移由 TS 侧镜像测试兜底（跨语言常量 tripwire）。

use std::sync::atomic::Ordering;

use super::arena::{AnswerField, SlotArena};
use super::codec::{decode_gen, decode_join_state, encode_join, encode_w, JoinState, SlotState};
use super::platform::FacadePlatform;
use super::roles::SpawnFailure;

/// A1-A3 应答结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerOutcome {
    /// A1-A3 全序完成（OK 或 FAIL 终态已发布）。
    Answered,
    /// A1 期望失配（换代/已取消/迟到）⇒ 整体丢弃、**对槽写次数 = 0**
    /// （§13.1：写权获取在首次载荷写之前——迟到写者的写根本不落槽）；
    /// 对迟到构造子的 terminate 卫生义务照旧（幂等）。
    DroppedLate,
}

/// C0 缝（消息处理入口、A1 前）+ A1-A3 全序（§13.1）。
/// gen 取自**消息**而非槽读（§11.2 H5 纪律）。
pub fn answer_ok<P: FacadePlatform>(
    arena: &SlotArena,
    platform: &P,
    slot: u32,
    gen_from_msg: u32,
    worker_id: u32,
) -> AnswerOutcome {
    platform.seam_supervisor_msg_entry(slot); // SEAM-C0
    let gb = arena.gen_bits();
    // A1：CAS (g,REQ)→(g,ANSWERING)（期望 = 完整 W 字）。
    let req_w = encode_w(gen_from_msg, SlotState::Req, gb);
    let answering_w = encode_w(gen_from_msg, SlotState::Answering, gb);
    if arena
        .slot_word(slot)
        .compare_exchange(req_w, answering_w, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        platform.terminate_child(worker_id);
        return AnswerOutcome::DroppedLate;
    }
    platform.seam_supervisor_after_acquire(slot); // SEAM-C1
                                                  // A2：原子写 worker_id 载荷（槽被 A1 钉死，A4 排他）。
    arena
        .answer_word(slot, AnswerField::WorkerId)
        .store(worker_id, Ordering::SeqCst);
    // A3：CAS →(g,OK) + wake_all。
    let ok_w = encode_w(gen_from_msg, SlotState::Ok, gb);
    if arena
        .slot_word(slot)
        .compare_exchange(answering_w, ok_w, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        // A4：领权即钉死——A1→A3 间唯一写者 = supervisor 自身 ⇒ 不可达。
        arena.tombstone_word().store(
            super::arena::TombstoneCode::UnexpectedWriter as u32,
            Ordering::SeqCst,
        );
        panic!("facade_slot：A3 OK CAS 出现协议外写者（槽 {slot}，gen {gen_from_msg}）");
    }
    platform.wake_all_u32(arena.slot_word_ptr(slot));
    AnswerOutcome::Answered
}

/// 构造失败应答（同 A1-A3；FAIL 无子、无 manifest 条目；RA 计入，
/// §13.3 RA 类账）。
pub fn answer_fail<P: FacadePlatform>(
    arena: &SlotArena,
    platform: &P,
    slot: u32,
    gen_from_msg: u32,
    errcode: u32,
) -> AnswerOutcome {
    platform.seam_supervisor_msg_entry(slot);
    let gb = arena.gen_bits();
    let req_w = encode_w(gen_from_msg, SlotState::Req, gb);
    let answering_w = encode_w(gen_from_msg, SlotState::Answering, gb);
    if arena
        .slot_word(slot)
        .compare_exchange(req_w, answering_w, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        // FAIL = spawn 未成立、无子、无 manifest 条目（§12.1.3 表）⇒ 无
        // terminate 卫生对象；零槽写即终。
        return AnswerOutcome::DroppedLate;
    }
    platform.seam_supervisor_after_acquire(slot);
    // RA 类账：**必须在 FAIL 发布 CAS 之前**入账（复核修正）。
    // 若置于 wake_all 之后，请求者可在递增前完成递减 ⇒ u32 下溢回绕成
    // 0xFFFFFFFF 且恒等式静默通过。放在 CAS 前则 SeqCst 全序保证：任何能
    // 观察到我代 FAIL 的一方，必同时观察到本次递增。
    // FAIL 未 ACK 前计入（engine ACK 自回收时递减）；facade 级 monotone
    // 累计（跨实例，仅诊断）。
    arena
        .retired_fail_unacked_word()
        .fetch_add(1, Ordering::SeqCst);
    arena.facade_ra_total_word().fetch_add(1, Ordering::SeqCst);
    arena
        .answer_word(slot, AnswerField::Errcode)
        .store(errcode, Ordering::SeqCst);
    let fail_w = encode_w(gen_from_msg, SlotState::Fail, gb);
    if arena
        .slot_word(slot)
        .compare_exchange(answering_w, fail_w, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        arena.tombstone_word().store(
            super::arena::TombstoneCode::UnexpectedWriter as u32,
            Ordering::SeqCst,
        );
        panic!("facade_slot：A3 FAIL CAS 出现协议外写者（槽 {slot}，gen {gen_from_msg}）");
    }
    platform.wake_all_u32(arena.slot_word_ptr(slot));
    AnswerOutcome::Answered
}

/// D5 死亡信号种类（§8.2.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeadKind {
    HardTrap,
    Panic,
}

/// D5 写 join 槽结果（§9.2.3 CAS 仲裁输家分支）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeadOutcome {
    Written,
    /// 输给 DONE：join 已按正常出口解析——诊断日志，不触发 R2（§9.2.3）。
    LostToDone,
    /// 另一死亡通知已领写权或完成发布。
    LostToDead,
    /// 已 CONSUMED：迟到写丢弃，join 结果不被改写。
    LostToConsumed,
    /// 输给换代：join 槽已被复用为新一代（CAS 期望 (g,WAITING) 对
    /// (g+1,*) 必败）。这是**合法的迟到 D5**（旧 worker 死亡信号晚于槽复用
    /// 到达），过期即弃——不得 panic（复核修正：此前只看 state 不看 gen，
    /// (g+1,WAITING) 会落 other 分支硬崩）。
    LostToStaleGeneration,
}

/// D5：向 join 槽写死亡信号 + futex_notify（§8.2.2——Worker error 事件在
/// 创建者主线程触发，不依赖引擎 JS 活性）。
pub fn write_dead<P: FacadePlatform>(
    arena: &SlotArena,
    platform: &P,
    join_index: u32,
    gen: u32,
    kind: DeadKind,
    code: u32,
) -> DeadOutcome {
    let gb = arena.gen_bits();
    let waiting_w = encode_join(gen, JoinState::Waiting, gb);
    let dead_w = match kind {
        DeadKind::HardTrap => encode_join(gen, JoinState::DeadHardtrap, gb),
        DeadKind::Panic => encode_join(gen, JoinState::DeadPanic, gb),
    };
    if arena
        .join_word(join_index)
        .compare_exchange(
            waiting_w,
            encode_join(gen, JoinState::DeadWriting, gb),
            Ordering::SeqCst,
            Ordering::SeqCst,
        )
        .is_ok()
    {
        platform.seam_dead_after_acquire(join_index);
        arena.set_join_dead_code(join_index, code);
        arena
            .join_word(join_index)
            .compare_exchange(
                encode_join(gen, JoinState::DeadWriting, gb),
                dead_w,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .expect("D5 载荷写权被协议外写者破坏");
        platform.wake_all_u32(arena.join_word_ptr(join_index));
        return DeadOutcome::Written;
    }
    // 输家分支：**先判代再判状态**（复核修正）。此前只 decode state，迟到
    // 的 g 代 D5 撞上已复用为 (g+1,WAITING) 的 join 槽会落 other 分支硬崩——
    // 那是合法的"迟到死亡信号"，应过期即弃。
    let now = arena.join_word(join_index).load(Ordering::SeqCst);
    if decode_gen(now, gb) != gen {
        return DeadOutcome::LostToStaleGeneration;
    }
    match decode_join_state(now, gb) {
        JoinState::Done => DeadOutcome::LostToDone,
        JoinState::DeadWriting | JoinState::DeadHardtrap | JoinState::DeadPanic => {
            DeadOutcome::LostToDead
        }
        JoinState::Consumed => DeadOutcome::LostToConsumed,
        other => {
            arena.tombstone_word().store(
                super::arena::TombstoneCode::UnexpectedWriter as u32,
                Ordering::SeqCst,
            );
            panic!("facade_slot：D5 CAS 输家位形非法 {other:?}（join {join_index}，gen {gen}）");
        }
    }
}

/// 监督者簿记（延迟回收登记面，§12.1.3：主线程不等待）。
#[derive(Default)]
pub struct SupervisorState {
    /// try-lock 失败转登记的 (slot, gen)；下一检查点重试。
    pub pending_recovery: Vec<(u32, u32)>,
}

/// OK 回收门链评估结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryOutcome {
    /// 门链闭合 ∧ try-lock 成功 → 已 push 空闲表。
    Pushed,
    /// 门未闭合（join 未 CONSUMED / 非 OK 终态）——保活跃，下检查点再评。
    GatesNotMet,
    /// 门链闭合但 try-lock 失败 → 延迟回收登记（不阻塞主线程）。
    Delayed,
}

/// workerExit 处理：manifest 摘除（幂等，本驱动只评估门链）→ OK 回收
/// 三门（§10.2.1-4：(a) 终态 OK ∧ (b) join CONSUMED ∧ (c) 摘除=本函数）。
pub fn on_worker_exit<P: FacadePlatform>(
    arena: &SlotArena,
    platform: &P,
    state: &mut SupervisorState,
    slot: u32,
    gen: u32,
) -> RecoveryOutcome {
    if !recovery_gates_met(arena, slot, gen) {
        return RecoveryOutcome::GatesNotMet;
    }
    // 门 (c)：摘除已发生（本函数即摘除动作；生产 = TS manifest ACTIVE→REMOVED）。
    // try-lock 快路径（主线程不 Atomics.wait）。
    platform.seam_recovery_before_lock();
    if !arena.h1_try_lock() {
        if !state.pending_recovery.contains(&(slot, gen)) {
            state.pending_recovery.push((slot, gen));
        }
        return RecoveryOutcome::Delayed;
    }
    if !recovery_gates_met(arena, slot, gen) || arena.is_free(slot) {
        arena.h1_unlock(platform);
        return RecoveryOutcome::GatesNotMet;
    }
    arena.push_free(slot);
    arena.h1_unlock(platform);
    RecoveryOutcome::Pushed
}

/// 测试驱动的manifest摘除由调用方保证；槽与join完整代在H1内再次核对。
fn recovery_gates_met(arena: &SlotArena, slot: u32, gen: u32) -> bool {
    let gb = arena.gen_bits();
    let w = arena.slot_word(slot).load(Ordering::SeqCst);
    // 门 (a)：(gen,OK) 终态（FAIL/CANCELLED 槽归 engine 自回收/RA 账，
    // 非 supervisor 回收面——§12.1.3 回收权按终态写者归属拆分）。
    if w != encode_w(gen, SlotState::Ok, gb) {
        return false;
    }
    // 门 (b)：join 槽 (gen,CONSUMED)（engine join 消费后不再触碰的证明）。
    let join_index = arena.join_index_of(slot);
    if arena.join_word(join_index).load(Ordering::SeqCst)
        != encode_join(gen, JoinState::Consumed, gb)
    {
        return false;
    }
    true
}

/// 延迟回收检查点重试（下一 facadeSpawn/workerExit 事件驱动）。
/// 返回本次成功 push 数。
pub fn recover_pending<P: FacadePlatform>(
    arena: &SlotArena,
    platform: &P,
    state: &mut SupervisorState,
) -> usize {
    let mut pushed = 0;
    // 先移走有限快照，只有取锁失败的同代登记留给下次检查点。
    for (slot, gen) in std::mem::take(&mut state.pending_recovery) {
        if on_worker_exit(arena, platform, state, slot, gen) == RecoveryOutcome::Pushed {
            pushed += 1;
        }
    }
    pushed
}

/// 兼容面：SpawnFailure 归 io 映射由门面承担（见 thread_facade wasm 段）。
pub fn spawn_failure_message(failure: &SpawnFailure) -> String {
    match failure {
        SpawnFailure::SlotPoolExhausted {
            retired_fail_unacked,
            retired_gen_exhausted,
        } => format!(
            "SLOT_POOL_EXHAUSTED{{retired_fail_unacked={retired_fail_unacked},retired_gen_exhausted={retired_gen_exhausted}}}"
        ),
        SpawnFailure::AckTimeout => "SPAWN_ACK_TIMEOUT".to_owned(),
        SpawnFailure::WorkerFailed { errcode } => format!("SPAWN_WORKER_FAILED{{errcode={errcode}}}"),
    }
}
