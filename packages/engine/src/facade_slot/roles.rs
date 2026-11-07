//! 角色操作：engine 请求者（H1-H5 + 谓词等待/取消/自回收）、子 trampoline
//! （决策读/双检绊线/W1-W3）、JoinHandle join（§9.2.3 join 单向状态机）。
//!
//! 生产监督者 = 主线程 TS（`apps/web/src/host/facade-supervisor.ts`）；
//! Rust 侧 `test_supervisor`（cfg(test)）为 N 测试的监督者角色驱动，
//! 与 TS 断言同一批设计不变量（A1-A3 序、门链、延迟回收）。

use std::io;
use std::marker::PhantomData;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::arena::{SlotArena, TombstoneCode};
use super::codec::{
    decode_gen, decode_join_state, decode_state, encode_join, encode_w, JoinState, SlotState,
};
use super::platform::FacadePlatform;

/// 擦除后的 spawn 闭包盒（H3 载荷字指向的本体；随 REQ 发布所有权入槽）。
pub struct SpawnJob {
    /// Box<F> 擦除指针。
    pub closure: *mut u8,
    /// 消费闭包并返回 Box<T>::into_raw（W1，子侧）。
    pub invoke: unsafe fn(*mut u8) -> *mut u8,
    /// FAIL/CANCELLED 臂由 engine 就地 drop 闭包（§9.2.1 所有权表）。
    pub drop_fn: unsafe fn(*mut u8),
    /// W2 输给死亡信号时释放未交付的结果，避免擦除类型后泄漏。
    pub drop_result: unsafe fn(*mut u8),
    /// 线程名（H5 消息携带；arena 不设 name 字——无槽上读者，台账登记偏差）。
    pub name: String,
    #[cfg(target_arch = "wasm32")]
    pub tombstone: &'static crate::wasm_tombstone::TombstoneRecord,
}

// F: Send + 'static ⇒ 跨线程（engine worker → 子 Worker）转移安全。
unsafe impl Send for SpawnJob {}

impl SpawnJob {
    /// 泛型闭包擦除（invoke/drop 单态化 fn 指针，无 vtable）。
    pub fn from_closure<F, T>(f: F, name: String) -> Box<SpawnJob>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        unsafe fn invoke_erased<F, T>(raw: *mut u8) -> *mut u8
        where
            F: FnOnce() -> T,
            T: Sized,
        {
            let f = Box::from_raw(raw as *mut F);
            let t: T = f();
            Box::into_raw(Box::new(t)) as *mut u8
        }
        unsafe fn drop_erased<F>(raw: *mut u8) {
            drop(Box::from_raw(raw as *mut F));
        }
        Box::new(SpawnJob {
            closure: Box::into_raw(Box::new(f)) as *mut u8,
            invoke: invoke_erased::<F, T>,
            drop_fn: drop_erased::<F>,
            drop_result: drop_erased::<T>,
            name,
            #[cfg(target_arch = "wasm32")]
            tombstone: Box::leak(Box::new(crate::wasm_tombstone::TombstoneRecord::new())),
        })
    }
}

/// engine 侧就地 drop 闭包盒（FAIL/CANCELLED 臂；恰一次）。
fn drop_job(job: *mut SpawnJob) {
    unsafe {
        let job = Box::from_raw(job);
        (job.drop_fn)(job.closure);
        // SpawnJob 盒（含 name）随之析构。
    }
}

/// spawn 结构化失败面（显式 Err，不 panic；§11.2/§13.3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnFailure {
    /// 池耗尽（携带 arena 级 RA/RB 两计数，区分成因，§14.1）。
    SlotPoolExhausted {
        retired_fail_unacked: u32,
        retired_gen_exhausted: u32,
    },
    /// ACK 超时且取消成立（§8.2.1-3 SPAWN_ACK_TIMEOUT）。
    AckTimeout,
    /// 构造失败（errcode=WORKER_CONSTRUCT/SCRIPT_LOAD/INSTANTIATE/…）。
    WorkerFailed { errcode: u32 },
}

/// H1-H5 发布结果（job 指针供 engine 失败臂就地 drop）。
pub struct PublishedSpawn {
    pub slot: u32,
    pub gen: u32,
    pub(crate) job: *mut SpawnJob,
}

/// H1-H4（分配+发布）——请求全序 §12.1.2；失败面 = 池耗尽（未发布 ⇒ 闭包
/// 就地 drop，§10.2.1-5/§11.2）。调用方随后自行 H5（§13.2：H4 之后）。
pub fn publish_spawn<P: FacadePlatform>(
    arena: &SlotArena,
    platform: &P,
    job: Box<SpawnJob>,
) -> Result<PublishedSpawn, SpawnFailure> {
    let payload = Box::into_raw(job) as usize;
    arena.h1_lock_blocking(platform);
    // 临界区内只取分配结果，**不在锁内 drop_job**（复核修正，§12.1.3：H1 为
    // 叶锁、临界区不含共享堆 dealloc）。失败时先放锁，再在锁外就地 drop 闭包。
    let alloc_result = arena.h2_take(platform);
    match alloc_result {
        Ok(alloc) => {
            arena.h3_h4_publish(platform, alloc, payload, alloc.index);
            arena.h1_unlock(platform);
            Ok(PublishedSpawn {
                slot: alloc.index,
                gen: alloc.new_gen,
                job: payload as *mut SpawnJob,
            })
        }
        Err(e) => {
            arena.h1_unlock(platform);
            // 未发布 ⇒ 闭包所有权未入槽，锁外 drop 后显式失败。
            drop_job(payload as *mut SpawnJob);
            Err(SpawnFailure::SlotPoolExhausted {
                retired_fail_unacked: e.retired_fail_unacked,
                retired_gen_exhausted: e.retired_gen_exhausted,
            })
        }
    }
}

/// join 句柄（泛型 T 经 PhantomData 记忆；一次性——join 消费后槽 CONSUMED）。
pub struct FacadeJoinHandle<T> {
    pub(crate) arena: Arc<SlotArena>,
    pub(crate) slot: u32,
    pub(crate) gen: u32,
    pub(crate) join_index: u32,
    pub(crate) _pd: PhantomData<fn() -> T>,
}

impl<T> std::fmt::Debug for FacadeJoinHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FacadeJoinHandle")
            .field("slot", &self.slot)
            .field("gen", &self.gen)
            .field("join_index", &self.join_index)
            .finish()
    }
}

// Arc<SlotArena> Send+Sync + fn 指针 PhantomData ⇒ 自动 Send；显式声明供
// world.rs 句柄面（Send 约束来自 std::thread 语义）。
// **T: Send 约束（复核修正）**：此前 `unsafe impl<T> Send` 无任何 T 约束，
// 配合无约束的 join_typed<T> 可把非 Send 类型搬过线程边界。join 的语义
// 契约正是"取回子线程产出"，故 T 必与 std::thread::JoinHandle 同约束。
unsafe impl<T: Send> Send for FacadeJoinHandle<T> {}

/// 请求者全链：H1-H5 + 谓词等待（§13.1：REQ→ANSWERING→终态）+ 三终态分支
/// （§9.2.1 Box 所有权表 + §12.1.3 自回收）。
pub fn request_spawn<T, P>(
    arena: &Arc<SlotArena>,
    platform: &P,
    job: Box<SpawnJob>,
    ack_timeout_ms: u64,
) -> Result<FacadeJoinHandle<T>, SpawnFailure>
where
    P: FacadePlatform,
    T: Send + 'static,
{
    // name 先行克隆（H5 时 job 所有权已入槽，避免与 trampoline 并发读）。
    let name = job.name.clone();
    #[cfg(target_arch = "wasm32")]
    let tombstone_addr = job.tombstone as *const _ as u32;
    let published = publish_spawn(arena, platform, job)?;
    // H5：REQ 发布之后（§13.2 时序前提——消息先到会使 A1 假性失败）。
    #[cfg(not(target_arch = "wasm32"))]
    platform.notify_spawn(published.slot, published.gen, &name);
    #[cfg(target_arch = "wasm32")]
    platform.notify_spawn_v2(published.slot, published.gen, &name, tombstone_addr);

    let slot = published.slot;
    let gen = published.gen;
    let gb = arena.gen_bits();
    let w_ptr = arena.slot_word_ptr(slot);
    let mut cancel_attempted = false;
    loop {
        let w = arena.slot_word(slot).load(Ordering::SeqCst);
        if decode_gen(w, gb) != gen {
            arena.tombstone_word().store(
                TombstoneCode::RequesterGenerationAdvanced as u32,
                Ordering::SeqCst,
            );
            panic!(
                "facade_slot 绊线：请求者在途见换代（槽 {slot} 期望 gen {gen} 实读 {w:#x}）——I5′ 门链破坏"
            );
        }
        match decode_state(w, gb) {
            SlotState::Ok => {
                // 读 worker_id 载荷 + 双检绊线（§13.2：防御性，不参与证明）。
                let worker_id = arena
                    .answer_word(slot, super::arena::AnswerField::WorkerId)
                    .load(Ordering::SeqCst);
                let recheck = arena.slot_word(slot).load(Ordering::SeqCst);
                if recheck != w {
                    arena.tombstone_word().store(
                        TombstoneCode::RequesterGenerationAdvanced as u32,
                        Ordering::SeqCst,
                    );
                    panic!("facade_slot 绊线：OK 载荷读窗口 W 变更（槽 {slot}）");
                }
                let _ = worker_id; // 诊断面（句柄 join 对账经 manifest）
                let join_index = arena.join_index_of(slot);
                return Ok(FacadeJoinHandle {
                    arena: Arc::clone(arena),
                    slot,
                    gen,
                    join_index,
                    _pd: PhantomData,
                });
            }
            SlotState::Fail => {
                // SEAM-B：醒见 FAIL 后、错误码载荷读前（N2）。
                platform.seam_engine_fail_seen(slot);
                let errcode = arena
                    .answer_word(slot, super::arena::AnswerField::Errcode)
                    .load(Ordering::SeqCst);
                let recheck = arena.slot_word(slot).load(Ordering::SeqCst);
                if recheck != w {
                    arena.tombstone_word().store(
                        TombstoneCode::RequesterGenerationAdvanced as u32,
                        Ordering::SeqCst,
                    );
                    panic!("facade_slot 绊线：FAIL 载荷读窗口 W 变更（槽 {slot}）");
                }
                // CAS (g,FAIL)→(g,FAIL_ACKED)（第四门回执）。
                let fail_w = encode_w(gen, SlotState::Fail, gb);
                let acked_w = encode_w(gen, SlotState::FailAcked, gb);
                if arena
                    .slot_word(slot)
                    .compare_exchange(fail_w, acked_w, Ordering::SeqCst, Ordering::SeqCst)
                    .is_err()
                {
                    arena
                        .tombstone_word()
                        .store(TombstoneCode::UnexpectedWriter as u32, Ordering::SeqCst);
                    panic!("facade_slot：FAIL_ACKED CAS 出现协议外写者（槽 {slot}）");
                }
                platform.wake_all_u32(w_ptr);
                drop_job(published.job);
                // RA 账递减（监督者写 FAIL 时计入）+ 即刻自回收（H1）。
                arena.h1_lock_blocking(platform);
                arena
                    .retired_fail_unacked_word()
                    .fetch_sub(1, Ordering::SeqCst);
                arena.push_free(slot);
                arena.h1_unlock(platform);
                return Err(SpawnFailure::WorkerFailed { errcode });
            }
            SlotState::Req | SlotState::Answering => {
                match platform.wait_u32(w_ptr, w, ack_timeout_ms) {
                    super::platform::FutexWait::ValueChanged => continue,
                    super::platform::FutexWait::TimedOut => {
                        if !cancel_attempted {
                            // 超时醒来仍先尝试取消 CAS（§13.1）；单次尝试。
                            cancel_attempted = true;
                            let req_w = encode_w(gen, SlotState::Req, gb);
                            let cancelled_w = encode_w(gen, SlotState::Cancelled, gb);
                            if arena
                                .slot_word(slot)
                                .compare_exchange(
                                    req_w,
                                    cancelled_w,
                                    Ordering::SeqCst,
                                    Ordering::SeqCst,
                                )
                                .is_ok()
                            {
                                platform.wake_all_u32(w_ptr);
                                drop_job(published.job);
                                // CANCELLED 即刻自回收（无 RA 账）。
                                arena.h1_lock_blocking(platform);
                                arena.push_free(slot);
                                arena.h1_unlock(platform);
                                return Err(SpawnFailure::AckTimeout);
                            }
                            // CAS 败：重读——ANSWERING ⇒ 取消窗关闭（A4①），
                            // 计诊断一次、不再尝试；终态 ⇒ 循环处理。
                            let now = arena.slot_word(slot).load(Ordering::SeqCst);
                            if decode_state(now, gb) == SlotState::Answering {
                                arena
                                    .timeout_hit_answering_word()
                                    .fetch_add(1, Ordering::SeqCst);
                            }
                            continue;
                        }
                        // 已试过取消（撞 ANSWERING）⇒ 转等终态（§13.1：
                        // 迟到成功也是成功）。
                        continue;
                    }
                }
            }
            // 请求者在途不可达位形（CANCELLED 唯一写者=本函数超时臂且写后
            // 即返；IDLE/FAIL_ACKED/RETIRED 无写路径）⇒ 协议外 ⇒ 绊线。
            other => {
                arena
                    .tombstone_word()
                    .store(TombstoneCode::UnexpectedWriter as u32, Ordering::SeqCst);
                panic!("facade_slot：请求者在途非法状态 {other:?}（槽 {slot}，W={w:#x}）");
            }
        }
    }
}

/// engine 超时写者独立驱动面（N3 操作级；request_spawn 超时臂同式）。
pub enum CancelOutcome {
    /// 取消成立（已 wake + drop 闭包 + 自回收）。
    Cancelled,
    /// CAS 输给应答（迟到成功/失败——调用方重读决策；闭包所有权随终态）。
    LostAnswered,
}

pub fn engine_cancel<P: FacadePlatform>(
    arena: &SlotArena,
    platform: &P,
    published: PublishedSpawn,
) -> CancelOutcome {
    let gb = arena.gen_bits();
    let req_w = encode_w(published.gen, SlotState::Req, gb);
    let cancelled_w = encode_w(published.gen, SlotState::Cancelled, gb);
    if arena
        .slot_word(published.slot)
        .compare_exchange(req_w, cancelled_w, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        platform.wake_all_u32(arena.slot_word_ptr(published.slot));
        drop_job(published.job);
        arena.h1_lock_blocking(platform);
        arena.push_free(published.slot);
        arena.h1_unlock(platform);
        CancelOutcome::Cancelled
    } else {
        CancelOutcome::LostAnswered
    }
}

/// trampoline 出口形态。
#[derive(Debug)]
pub enum TrampolineExit {
    /// W1 闭包已消费、W2 join DONE 已写、退出意图已记录；真实 W3 由 JS 收尾发出。
    Ran,
    /// 决策读非 (g,OK)（CANCELLED@旧代或换代）——不解引用载荷、退出意图照常记录。
    Superseded,
}

/// 子 trampoline（§12.1.5 决策读 + §13.2 双检绊线 + §9.2.3 W1-W3）。
///
/// worker_id 由监督者构造期经握手通道下发（非槽载荷——N1 尾巴的幂等
/// terminate/workerExit 依赖它）。
pub fn trampoline_run<P: FacadePlatform>(
    arena: &SlotArena,
    platform: &P,
    slot: u32,
    gen: u32,
    worker_id: u32,
) -> TrampolineExit {
    let gb = arena.gen_bits();
    // 等待应答（谓词循环：(g,REQ)/(g,ANSWERING) 再等；终态/换代出）。
    loop {
        let w = arena.slot_word(slot).load(Ordering::SeqCst);
        if decode_gen(w, gb) != gen {
            break; // 换代 → 决策读（不解引用）
        }
        match decode_state(w, gb) {
            SlotState::Req | SlotState::Answering => {
                platform.wait_u32(arena.slot_word_ptr(slot), w, u64::MAX);
            }
            _ => break, // (g,OK)/(g,FAIL)/(g,CANCELLED)
        }
    }
    // SEAM-A：wait 醒后、决策读前（N1 冻结点）。
    platform.seam_trampoline_wake(slot);
    // 决策读（冻结后新读——窗口期可已换代，N1 断言面）。
    let expected_ok = encode_w(gen, SlotState::Ok, gb);
    let w2 = arena.slot_word(slot).load(Ordering::SeqCst);
    if w2 != expected_ok {
        // 非 (g,OK) ⇒ 不解引用载荷（§9.2.1 "state≠OK 不解引用"守卫）；
        // 旧子照常记录退出意图；返回 JS 并清理后才上报真实 W3（§9.2.5-2）。
        platform.worker_exit(worker_id, slot, gen);
        return TrampolineExit::Superseded;
    }
    // SEAM-A2：读 W=(g,OK) 后、载荷读前（N1-b 冻结点）。
    platform.seam_trampoline_ok_read(slot);
    let job_ptr = arena.payload_ptr(slot) as *mut SpawnJob;
    // 双检绊线（§13.2：门链被未来改动破坏时的显式失败，不参与正确性证明）。
    let recheck = arena.slot_word(slot).load(Ordering::SeqCst);
    if recheck != expected_ok {
        arena.tombstone_word().store(
            TombstoneCode::TrampolinePayloadTripwire as u32,
            Ordering::SeqCst,
        );
        panic!("facade_slot 绊线：trampoline 载荷读窗口 W 变更（槽 {slot}，gen {gen}）");
    }
    // W1：恰一闭包恰一跑。
    let job = unsafe { Box::from_raw(job_ptr) };
    #[cfg(target_arch = "wasm32")]
    {
        use crate::wasm_tombstone::{DeathKind, PanicTarget, TombstoneHeader};
        let join_index = arena.join_index_of(slot);
        // wasm RUNTIME 是整个实例内不可重置的静态 owner；记录同样保持到实例废弃。
        let join = unsafe { &*(arena.join_word(join_index) as *const _) };
        let dead_code = unsafe { &*(arena.join_dead_code_word(join_index) as *const _) };
        crate::wasm_tombstone::bind(PanicTarget {
            record: job.tombstone,
            join,
            dead_code,
            waiting: encode_join(gen, JoinState::Waiting, gb),
            writing: encode_join(gen, JoinState::DeadWriting, gb),
            dead: encode_join(gen, JoinState::DeadPanic, gb),
            header: TombstoneHeader {
                worker_id,
                // child 执行相位尚无已批准编码；0 明确表示未知。
                phase: 0,
                kind: DeathKind::Panic,
            },
        });
    }
    let result = unsafe { (job.invoke)(job.closure) };
    let drop_result = job.drop_result;
    drop(job);
    // W2：join 槽写 result + CAS WAITING→DONE + wake（§9.2.3）。
    let join_index = arena.join_index_of(slot);
    arena.set_join_result(join_index, result as usize);
    let waiting_w = encode_join(gen, JoinState::Waiting, gb);
    let done_w = encode_join(gen, JoinState::Done, gb);
    match arena.join_word(join_index).compare_exchange(
        waiting_w,
        done_w,
        Ordering::SeqCst,
        Ordering::SeqCst,
    ) {
        Ok(_) => platform.wake_all_u32(arena.join_word_ptr(join_index)),
        Err(actual) => {
            // D5 获胜：join 只消费死亡载荷，结果仍归本线程，恰一次释放。
            unsafe {
                drop_result(result);
            }
            if decode_gen(actual, gb) != gen
                || !matches!(
                    decode_join_state(actual, gb),
                    JoinState::DeadWriting
                        | JoinState::DeadHardtrap
                        | JoinState::DeadPanic
                        | JoinState::Consumed
                )
            {
                arena
                    .tombstone_word()
                    .store(TombstoneCode::UnexpectedWriter as u32, Ordering::SeqCst);
                panic!("facade_slot：W2 DONE CAS 出现协议外写者（join {join_index}，gen {gen}）");
            }
        }
    }
    // 仅记录退出意图；须返回 JS 后 raw destroy，才能发真实 W3 并 close。
    platform.worker_exit(worker_id, slot, gen);
    TrampolineExit::Ran
}

/// JoinHandle::join（§9.2.3 消费面）：终态读取 → CONSUMED；死亡信号映射
/// 结构化 io::Error。
///
/// **T: Send + 'static（复核修正）**：函数体会把 join_result 当 `*mut T`
/// 解引用（`Box::from_raw`）。此前 T 无任何约束，join_index/gen 又是裸 u32
/// （wasm 侧直接来自 `facade_child_trampoline` 的 JS 入参），调用方错配类型
/// 即静默 reinterpret。约束到与 `std::thread::JoinHandle::join` 一致。
pub fn join_typed<T, P>(arena: &SlotArena, platform: &P, join_index: u32, gen: u32) -> io::Result<T>
where
    P: FacadePlatform,
    T: Send + 'static,
{
    let gb = arena.gen_bits();
    loop {
        let jw = arena.join_word(join_index).load(Ordering::SeqCst);
        if decode_gen(jw, gb) != gen {
            arena
                .tombstone_word()
                .store(TombstoneCode::JoinResetMismatch as u32, Ordering::SeqCst);
            panic!("facade_slot：join 在途换代（join {join_index} 期望 gen {gen}）——门链破坏");
        }
        match decode_join_state(jw, gb) {
            JoinState::Waiting | JoinState::DeadWriting => {
                platform.wait_u32(arena.join_word_ptr(join_index), jw, u64::MAX);
            }
            JoinState::Done => {
                let ptr = arena.join_result(join_index);
                let recheck = arena.join_word(join_index).load(Ordering::SeqCst);
                if recheck != jw {
                    arena
                        .tombstone_word()
                        .store(TombstoneCode::JoinResetMismatch as u32, Ordering::SeqCst);
                    panic!("facade_slot 绊线：join result 读窗口 JW 变更（join {join_index}）");
                }
                let consumed_w = encode_join(gen, JoinState::Consumed, gb);
                if arena
                    .join_word(join_index)
                    .compare_exchange(jw, consumed_w, Ordering::SeqCst, Ordering::SeqCst)
                    .is_err()
                {
                    arena
                        .tombstone_word()
                        .store(TombstoneCode::UnexpectedWriter as u32, Ordering::SeqCst);
                    panic!("facade_slot：CONSUMED CAS 出现协议外写者（join {join_index}）");
                }
                platform.wake_all_u32(arena.join_word_ptr(join_index));
                return Ok(*unsafe { Box::from_raw(ptr as *mut T) });
            }
            JoinState::DeadHardtrap | JoinState::DeadPanic => {
                let code = arena.join_dead_code(join_index);
                let consumed_w = encode_join(gen, JoinState::Consumed, gb);
                if arena
                    .join_word(join_index)
                    .compare_exchange(jw, consumed_w, Ordering::SeqCst, Ordering::SeqCst)
                    .is_err()
                {
                    arena
                        .tombstone_word()
                        .store(TombstoneCode::UnexpectedWriter as u32, Ordering::SeqCst);
                    panic!("facade_slot：DEAD 消费 CAS 出现协议外写者（join {join_index}）");
                }
                platform.wake_all_u32(arena.join_word_ptr(join_index));
                let kind = if matches!(decode_join_state(jw, gb), JoinState::DeadHardtrap) {
                    "HARDTRAP"
                } else {
                    "PANIC"
                };
                return Err(io::Error::other(format!(
                    "FACADE_DEAD_{kind}:0x{code:X}（worker 死亡信号，§8.2.2）"
                )));
            }
            JoinState::Consumed => {
                arena
                    .tombstone_word()
                    .store(TombstoneCode::UnexpectedWriter as u32, Ordering::SeqCst);
                panic!("facade_slot：join 句柄重复消费（join {join_index}）");
            }
        }
    }
}
