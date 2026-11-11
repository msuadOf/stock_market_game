//! 槽 arena：共享内存布局 + H1 分配锁 + H2-H4 分配器 + RB 退休 + 四账。
//!
//! **布局**（均一 u32 原子字，JS 侧经 `Int32Array` 同偏移访问；TS 镜像 =
//! `apps/web/src/host/facade-slot-protocol.ts`）：
//!
//! ```text
//! 头部 16 字：
//!   [0] h1_lock       [1] h1_waiters   [2] bump          [3] free_head
//!   [4] free_len      [5] RA(arena)    [6] RB(arena)     [7] tombstone
//!   [8] timeout_hit_answering(诊断)    [9] capacity      [10] gen_bits
//!   [11] facade_RA_total(跨实例累计)   [12] facade_RB_total
//!   [13..15] 预留
//! 请求槽 ×capacity，步长 7：
//!   [0] W [1] payload_lo [2] payload_hi [3] join_index
//!   [4] answer_a(worker_id) [5] answer_b(errcode) [6] free_next
//! join 槽 ×capacity（index 与请求槽配对），步长 4：
//!   [0] JW [1] result_lo [2] result_hi [3] dead_code
//! ```
//!
//! **H1 分配器锁**（§12.1.3）：u32 锁字 + 等待计数握手（unlock 见
//! waiter_count>0 → wake_all——防 test-and-set 型丢唤醒）；持有者 = engine
//! 请求者（H2-H4）/ 终态写者自回收（push）/ supervisor OK 回收（try-lock
//! 快路径，失败转延迟回收登记——主线程不 `Atomics.wait`）。临界区 = 纯
//! 内存有限指令（A5 同型）：**不含** memory.grow 与共享堆分配（arena 于
//! facade init 单线程期预划拨，容量 2×(G+N+2)，§12.1.6）⇒ 叶锁、无出边。
//!
//! **回收权归属**（§12.1.3 修订 §10.2.1-4）：OK = supervisor（门 (b) join
//! CONSUMED ∧ (c) manifest 摘除）；FAIL_ACKED / CANCELLED = engine 终态
//! 写者即刻自回收（写后即确知无旧写者，I7）。
//!
//! **四账**（§14.1 可用账口径）：五态闭合枚举 UNALLOCATED/FREE/ACTIVE/
//! RETIRED-RA/RETIRED-RB 每槽恰居其一；恒等式
//! `活跃 + 可用(空闲表+bump未及) + RA + RB ≡ 容量 = 2×(G+N+2)`。

use std::sync::atomic::{AtomicU32, Ordering};

use super::codec::{
    decode_gen, decode_join_state, decode_state, encode_join, encode_w, gen_bits_valid, gen_max,
    JoinState, SlotState, GEN_BITS_DEFAULT,
};
use super::platform::FacadePlatform;

// —— 头部字偏移 ——
pub const H1_LOCK: usize = 0;
pub const H1_WAITERS: usize = 1;
pub const BUMP: usize = 2;
pub const FREE_HEAD: usize = 3;
/// 空闲表空哨兵（TS 侧镜像值为 -1）。
pub const FREE_HEAD_EMPTY: u32 = u32::MAX;
pub const FREE_LEN: usize = 4;
pub const RETIRED_FAIL_UNACKED: usize = 5;
pub const RETIRED_GEN_EXHAUSTED: usize = 6;
pub const TOMBSTONE: usize = 7;
pub const TIMEOUT_HIT_ANSWERING: usize = 8;
pub const CAPACITY: usize = 9;
pub const GEN_BITS: usize = 10;
pub const FACADE_RA_TOTAL: usize = 11;
pub const FACADE_RB_TOTAL: usize = 12;
pub const HEADER_WORDS: usize = 16;

// —— 请求槽字段偏移（步长 7）——
pub const SLOT_STRIDE: usize = 7;
pub const SLOT_W: usize = 0;
pub const SLOT_PAYLOAD_LO: usize = 1;
pub const SLOT_PAYLOAD_HI: usize = 2;
pub const SLOT_JOIN_INDEX: usize = 3;
pub const SLOT_ANSWER_A: usize = 4;
pub const SLOT_ANSWER_B: usize = 5;
pub const SLOT_FREE_NEXT: usize = 6;

// —— join 槽字段偏移（步长 4）——
pub const JOIN_STRIDE: usize = 4;
pub const JOIN_W: usize = 0;
pub const JOIN_RESULT_LO: usize = 1;
pub const JOIN_RESULT_HI: usize = 2;
pub const JOIN_DEAD_CODE: usize = 3;

/// tombstone 结构化码（§8.6-② 合成面的 arena 落点；供 R2-3 协议消费）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum TombstoneCode {
    /// H4 CAS 期望失配（迟到写者 = 分配器 bug，§11.2 H2 纪律）。
    AllocExpectedMismatch = 1,
    /// 空闲表/bump 槽位形违约（push 非终态、退休槽在表、bump 槽非零）。
    FreeListCorrupt = 2,
    /// join 槽复用时旧代未 CONSUMED（门链破坏）。
    JoinResetMismatch = 3,
    /// 请求者在途见换代（I5′ 门链破坏绊线）。
    RequesterGenerationAdvanced = 4,
    /// trampoline 载荷双检失配（I5′ 绊线，§13.2）。
    TrampolinePayloadTripwire = 5,
    /// 终态/消费 CAS 出现协议外写者。
    UnexpectedWriter = 6,
}

/// 应答载荷字段（A2 写者 = 监督者）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerField {
    /// OK：worker_id。
    WorkerId,
    /// FAIL：结构化错误码。
    Errcode,
}

/// 分配结果（H2 产出；expected_w = H4 CAS 的期望完整旧字）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotAllocation {
    pub index: u32,
    pub new_gen: u32,
    pub expected_w: u32,
}

/// 池耗尽结构化失败（§11.2/§13.3：显式 Err 不 panic；携带 arena 级
/// RA/RB 两计数区分"垂死累积"与"健康耗尽"两种成因，§14.1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exhausted {
    pub retired_fail_unacked: u32,
    pub retired_gen_exhausted: u32,
}

/// 四账快照（§14.1；静点核对——计数器与扫描双源互证）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Accounts {
    pub active: u32,
    pub free: u32,
    pub unallocated: u32,
    pub retired_fail_unacked: u32,
    pub retired_gen_exhausted: u32,
    pub capacity: u32,
}

impl Accounts {
    /// 可用账 = 空闲表 + bump 未及（§14.1 升格口径）。
    pub fn available(&self) -> u32 {
        self.free + self.unallocated
    }
    /// 恒等式与账目一致性核对（空闲表走查完整性 + 计数器对照 + 五态恰居其一）。
    pub fn identity_holds(&self, arena: &SlotArena) -> bool {
        let capacity = self.capacity;
        // 五闭合：每槽恰居其一（走查空闲表成员集合）。
        let mut in_free = vec![false; capacity as usize];
        let mut cursor = arena.word(FREE_HEAD).load(Ordering::SeqCst);
        let mut walked: u32 = 0;
        while cursor != FREE_HEAD_EMPTY {
            let idx = cursor as usize;
            if idx >= capacity as usize || in_free[idx] {
                return false; // 越界或环 = 空闲表腐坏
            }
            in_free[idx] = true;
            walked += 1;
            cursor = arena
                .slot_field_word(idx, SLOT_FREE_NEXT)
                .load(Ordering::SeqCst);
        }
        if walked != arena.word(FREE_LEN).load(Ordering::SeqCst) {
            return false; // 表长与链长不符
        }
        // 逐槽分类（UNALLOCATED 由 bump 派生）。
        let bump = arena.word(BUMP).load(Ordering::SeqCst);
        let gen_bits = arena.gen_bits;
        let (mut active, mut ra, mut rb) = (0u32, 0u32, 0u32);
        for i in 0..capacity.min(bump) {
            let state = decode_state(
                arena
                    .slot_field_word(i as usize, SLOT_W)
                    .load(Ordering::SeqCst),
                gen_bits,
            );
            if in_free[i as usize] {
                // 空闲表内必为终态（Ok/FailAcked/Cancelled）——FAIL（RA）与
                // RETIRED 不得在表。
                match state {
                    SlotState::Ok | SlotState::FailAcked | SlotState::Cancelled => {}
                    _ => return false,
                }
                continue;
            }
            match state {
                SlotState::Fail => ra += 1,
                SlotState::Retired => rb += 1,
                _ => active += 1,
            }
        }
        // 恒等式（可用账口径）。
        if active + self.free + self.unallocated + ra + rb != capacity {
            return false;
        }
        // 计数器对照（arena 字 vs 扫描）。
        ra == arena.word(RETIRED_FAIL_UNACKED).load(Ordering::SeqCst)
            && rb == arena.word(RETIRED_GEN_EXHAUSTED).load(Ordering::SeqCst)
            && active == self.active
            && self.free == walked
    }
}

use crate::libos::thread::FACADE_SLOT_MAX_CAPACITY as MAX_CAPACITY;

/// 槽 arena（共享内存区；`Box<[AtomicU32]>` 连续，地址可导出给宿主视图）。
pub struct SlotArena {
    capacity: u32,
    gen_bits: u32,
    raw: Box<[AtomicU32]>,
}

// 裸字面共享：跨线程（engine worker / 测试角色线程）经 Arc 共享；
// wasm 面随共享线性内存跨实例可见。原子字全 SeqCst（§12.1.2 H3 载荷
// seqcst 纪律），无非原子字段。
unsafe impl Send for SlotArena {}
unsafe impl Sync for SlotArena {}

impl SlotArena {
    /// 单线程期划拨（facade init；容量 2×(G+N+2)，gen_bits 默认 29）。
    /// 容量/位宽在划拨后冻结（头部字供宿主与诊断读取）。
    pub fn alloc(capacity: u32, gen_bits: u32) -> SlotArena {
        if capacity == 0 || capacity > MAX_CAPACITY {
            panic!("arena 容量非法：{capacity}（须 1..=2^20）");
        }
        if !gen_bits_valid(gen_bits) {
            panic!("gen_bits 非法：{gen_bits}（须 1..=29）");
        }
        let words =
            HEADER_WORDS + capacity as usize * SLOT_STRIDE + capacity as usize * JOIN_STRIDE;
        let mut raw = Vec::with_capacity(words);
        for _ in 0..words {
            raw.push(AtomicU32::new(0));
        }
        let arena = SlotArena {
            capacity,
            gen_bits,
            raw: raw.into_boxed_slice(),
        };
        arena
            .word(FREE_HEAD)
            .store(FREE_HEAD_EMPTY, Ordering::SeqCst);
        arena.word(CAPACITY).store(capacity, Ordering::SeqCst);
        arena.word(GEN_BITS).store(gen_bits, Ordering::SeqCst);
        arena
    }

    #[cfg(target_family = "wasm")]
    pub fn capacity(&self) -> u32 {
        self.capacity
    }
    /// 共享字区首地址（宿主建 Int32Array 视图的锚；wasm32 上为 u32 域）。
    #[cfg(target_family = "wasm")]
    pub fn shared_words_addr(&self) -> usize {
        self.raw.as_ptr() as usize
    }
    pub fn gen_bits(&self) -> u32 {
        self.gen_bits
    }

    #[inline]
    fn word(&self, index: usize) -> &AtomicU32 {
        // 索引域由内部常量保证；越界 = 内部 bug ⇒ 显式 panic。
        self.raw
            .get(index)
            .unwrap_or_else(|| panic!("arena 字越界：{index}"))
    }
    #[inline]
    fn word_ptr(&self, index: usize) -> *const AtomicU32 {
        &self.raw[index] as *const AtomicU32
    }
    #[inline]
    fn slot_base(&self, index: u32) -> usize {
        HEADER_WORDS + index as usize * SLOT_STRIDE
    }
    #[inline]
    fn join_base(&self, index: u32) -> usize {
        HEADER_WORDS + self.capacity as usize * SLOT_STRIDE + index as usize * JOIN_STRIDE
    }

    // —— 测试/宿主桥接访问面（诊断与断言用；生产写路径走下方协议函数）——
    pub fn slot_word(&self, index: u32) -> &AtomicU32 {
        self.word(self.slot_base(index) + SLOT_W)
    }
    pub fn slot_word_ptr(&self, index: u32) -> *const AtomicU32 {
        self.word_ptr(self.slot_base(index) + SLOT_W)
    }
    pub fn slot_field_word(&self, index: usize, field: usize) -> &AtomicU32 {
        self.word(self.slot_base(index as u32) + field)
    }
    pub fn join_word(&self, index: u32) -> &AtomicU32 {
        self.word(self.join_base(index) + JOIN_W)
    }
    pub fn join_word_ptr(&self, index: u32) -> *const AtomicU32 {
        self.word_ptr(self.join_base(index) + JOIN_W)
    }
    pub fn answer_word(&self, index: u32, field: AnswerField) -> &AtomicU32 {
        let offset = match field {
            AnswerField::WorkerId => SLOT_ANSWER_A,
            AnswerField::Errcode => SLOT_ANSWER_B,
        };
        self.word(self.slot_base(index) + offset)
    }
    /// join result 双字写入（W2 trampoline 侧；测试亦可作驱动桩）。
    ///
    /// 指针经 u64 承载（`usize` 在 wasm32 仅 32 位，直接 `ptr >> 32` 是
    /// 编译期溢出）：wasm32 上 hi 恒 0，native 上保留完整 64 位地址。
    pub fn set_join_result(&self, index: u32, ptr: usize) {
        let wide = ptr as u64;
        self.word(self.join_base(index) + JOIN_RESULT_LO)
            .store(wide as u32, Ordering::SeqCst);
        self.word(self.join_base(index) + JOIN_RESULT_HI)
            .store((wide >> 32) as u32, Ordering::SeqCst);
    }
    /// join result 双字读取（wasm32 上 hi=0，`as usize` 无损）。
    pub fn join_result(&self, index: u32) -> usize {
        let lo = self
            .word(self.join_base(index) + JOIN_RESULT_LO)
            .load(Ordering::SeqCst) as u64;
        let hi = self
            .word(self.join_base(index) + JOIN_RESULT_HI)
            .load(Ordering::SeqCst) as u64;
        (lo | (hi << 32)) as usize
    }
    /// join 槽死亡信号细节码（D5 写者存、join 读面取）。
    pub fn join_dead_code(&self, index: u32) -> u32 {
        self.word(self.join_base(index) + JOIN_DEAD_CODE)
            .load(Ordering::SeqCst)
    }
    /// join 槽死亡信号细节码写入（D5 写者）。
    #[cfg(target_arch = "wasm32")]
    pub fn join_dead_code_word(&self, index: u32) -> &AtomicU32 {
        self.word(self.join_base(index) + JOIN_DEAD_CODE)
    }
    pub fn set_join_dead_code(&self, index: u32, code: u32) {
        self.word(self.join_base(index) + JOIN_DEAD_CODE)
            .store(code, Ordering::SeqCst);
    }
    pub fn tombstone_word(&self) -> &AtomicU32 {
        self.word(TOMBSTONE)
    }
    pub fn retired_fail_unacked_word(&self) -> &AtomicU32 {
        self.word(RETIRED_FAIL_UNACKED)
    }
    pub fn retired_gen_exhausted_word(&self) -> &AtomicU32 {
        self.word(RETIRED_GEN_EXHAUSTED)
    }
    pub fn facade_ra_total_word(&self) -> &AtomicU32 {
        self.word(FACADE_RA_TOTAL)
    }
    pub fn timeout_hit_answering_word(&self) -> &AtomicU32 {
        self.word(TIMEOUT_HIT_ANSWERING)
    }
    /// 载荷指针（H3 写、trampoline 读；u64 承载同 set_join_result）。
    pub fn payload_ptr(&self, index: u32) -> usize {
        let lo = self
            .word(self.slot_base(index) + SLOT_PAYLOAD_LO)
            .load(Ordering::SeqCst) as u64;
        let hi = self
            .word(self.slot_base(index) + SLOT_PAYLOAD_HI)
            .load(Ordering::SeqCst) as u64;
        (lo | (hi << 32)) as usize
    }
    /// 配对 join index（H3 写）。
    pub fn join_index_of(&self, index: u32) -> u32 {
        self.word(self.slot_base(index) + SLOT_JOIN_INDEX)
            .load(Ordering::SeqCst)
    }

    /// tombstone 落字 + 显式 panic（铁律二：先留证再崩）。
    fn tombstone_panic(&self, code: TombstoneCode, detail: &str) -> ! {
        self.word(TOMBSTONE).store(code as u32, Ordering::SeqCst);
        panic!("facade_slot tombstone({code:?})：{detail}");
    }

    /// H1 持锁下检查回收是否已完成，避免重复退出事件使空闲表成环。
    pub fn is_free(&self, index: u32) -> bool {
        let mut cursor = self.word(FREE_HEAD).load(Ordering::SeqCst);
        let mut walked = 0;
        while cursor != FREE_HEAD_EMPTY {
            if cursor >= self.capacity || walked >= self.capacity {
                self.tombstone_panic(TombstoneCode::FreeListCorrupt, "重复回收检查遇到损坏空闲表");
            }
            if cursor == index {
                return true;
            }
            cursor = self
                .slot_field_word(cursor as usize, SLOT_FREE_NEXT)
                .load(Ordering::SeqCst);
            walked += 1;
        }
        false
    }

    /// 四账快照（静点核对用：走查空闲表 + 逐槽分类）。
    pub fn accounts(&self) -> Accounts {
        let bump = self.word(BUMP).load(Ordering::SeqCst).min(self.capacity);
        let free_len = self.word(FREE_LEN).load(Ordering::SeqCst);
        let mut in_free = vec![false; self.capacity as usize];
        let mut cursor = self.word(FREE_HEAD).load(Ordering::SeqCst);
        while cursor != FREE_HEAD_EMPTY && (cursor as usize) < self.capacity as usize {
            if in_free[cursor as usize] {
                self.tombstone_panic(
                    TombstoneCode::FreeListCorrupt,
                    &format!("空闲表成环于槽 {cursor}（accounts 走查）"),
                );
            }
            in_free[cursor as usize] = true;
            cursor = self
                .slot_field_word(cursor as usize, SLOT_FREE_NEXT)
                .load(Ordering::SeqCst);
        }
        let mut active = 0u32;
        let mut ra = 0u32;
        let mut rb = 0u32;
        for (i, is_free) in in_free.iter().enumerate().take(bump as usize) {
            if *is_free {
                continue;
            }
            match decode_state(
                self.slot_field_word(i, SLOT_W).load(Ordering::SeqCst),
                self.gen_bits,
            ) {
                SlotState::Fail => ra += 1,
                SlotState::Retired => rb += 1,
                _ => active += 1,
            }
        }
        Accounts {
            active,
            free: free_len,
            unallocated: self.capacity - bump,
            retired_fail_unacked: ra,
            retired_gen_exhausted: rb,
            capacity: self.capacity,
        }
    }

    // -----------------------------------------------------------------
    // H1 分配器锁（§12.1.3 规格）
    // -----------------------------------------------------------------
    fn assert_h1_held(&self, op: &str) {
        if self.word(H1_LOCK).load(Ordering::SeqCst) != 1 {
            self.tombstone_panic(
                TombstoneCode::FreeListCorrupt,
                &format!("{op} 须在 H1 持有下调用（锁纪律违约）"),
            );
        }
    }

    /// try-lock 快路径（supervisor 侧；失败转延迟回收登记，不阻塞主线程）。
    pub fn h1_try_lock(&self) -> bool {
        self.word(H1_LOCK)
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }

    /// engine 侧阻塞获取（futex Mutex；等待计数握手防丢唤醒/活锁）。
    pub fn h1_lock_blocking<P: FacadePlatform>(&self, platform: &P) {
        loop {
            if self
                .word(H1_LOCK)
                .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                return;
            }
            self.word(H1_WAITERS).fetch_add(1, Ordering::SeqCst);
            // 期望=1（持有）：解锁 store 0 后由 wake_all 冲刷。
            platform.wait_u32(self.word_ptr(H1_LOCK), 1, u64::MAX);
            self.word(H1_WAITERS).fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// 解锁（ waiter_count>0 → wake_all）。
    pub fn h1_unlock<P: FacadePlatform>(&self, platform: &P) {
        self.word(H1_LOCK).store(0, Ordering::Release);
        if self.word(H1_WAITERS).load(Ordering::SeqCst) > 0 {
            platform.wake_all_u32(self.word_ptr(H1_LOCK));
        }
    }

    // -----------------------------------------------------------------
    // H2 分配（须持 H1）：pop LIFO / bump；RB 退休发生于此（唯一入账点）
    // -----------------------------------------------------------------
    pub fn h2_take<P: FacadePlatform>(&self, platform: &P) -> Result<SlotAllocation, Exhausted> {
        self.assert_h1_held("H2");
        loop {
            let head = self.word(FREE_HEAD).load(Ordering::SeqCst);
            if head != FREE_HEAD_EMPTY {
                let idx = head as usize;
                let w_old = self.slot_field_word(idx, SLOT_W).load(Ordering::SeqCst);
                let gen = decode_gen(w_old, self.gen_bits);
                match decode_state(w_old, self.gen_bits) {
                    SlotState::Retired => {
                        self.tombstone_panic(
                            TombstoneCode::FreeListCorrupt,
                            &format!("退休槽 {idx} 出现在空闲表（复活 = 分配器 bug）"),
                        );
                    }
                    SlotState::Ok | SlotState::FailAcked | SlotState::Cancelled => {}
                    other => {
                        self.tombstone_panic(
                            TombstoneCode::FreeListCorrupt,
                            &format!("空闲表槽 {idx} 状态非法：{other:?}（未终态即回收）"),
                        );
                    }
                }
                // pop（LIFO）。
                let next = self
                    .slot_field_word(idx, SLOT_FREE_NEXT)
                    .load(Ordering::SeqCst);
                self.word(FREE_HEAD).store(next, Ordering::SeqCst);
                self.word(FREE_LEN).fetch_sub(1, Ordering::SeqCst);
                // RB 退休：gen 达上界 ⇒ 钉 (max_gen,RETIRED) 永不复用（§13.3）。
                if gen >= gen_max(self.gen_bits) {
                    self.slot_field_word(idx, SLOT_W).store(
                        encode_w(gen, SlotState::Retired, self.gen_bits),
                        Ordering::SeqCst,
                    );
                    self.word(RETIRED_GEN_EXHAUSTED)
                        .fetch_add(1, Ordering::SeqCst);
                    self.word(FACADE_RB_TOTAL).fetch_add(1, Ordering::SeqCst);
                    // I6：这也是一次 W 转移，退出的等待者必须被冲刷。
                    // 该槽此后永不复用 ⇒ 不会有后续的 H4 wake 来兜底，
                    // 阻塞在旧期望值上的旧 trampoline 将永挂（u64::MAX 等待）。
                    platform.wake_all_u32(self.slot_word_ptr(idx as u32));
                    continue; // 取下一槽
                }
                return Ok(SlotAllocation {
                    index: idx as u32,
                    new_gen: gen + 1,
                    expected_w: w_old,
                });
            }
            // bump：新槽首用 gen=0（W 初值 0 = (0,Idle)，§12.1.1）。
            let b = self.word(BUMP).load(Ordering::SeqCst);
            if b >= self.capacity {
                return Err(Exhausted {
                    retired_fail_unacked: self.word(RETIRED_FAIL_UNACKED).load(Ordering::SeqCst),
                    retired_gen_exhausted: self.word(RETIRED_GEN_EXHAUSTED).load(Ordering::SeqCst),
                });
            }
            let w_old = self
                .slot_field_word(b as usize, SLOT_W)
                .load(Ordering::SeqCst);
            if w_old != 0 {
                self.tombstone_panic(
                    TombstoneCode::FreeListCorrupt,
                    &format!("bump 槽 {b} W 非零（{w_old}）：未划拨槽被外部写入"),
                );
            }
            self.word(BUMP).store(b + 1, Ordering::SeqCst);
            return Ok(SlotAllocation {
                index: b,
                new_gen: 0,
                expected_w: 0,
            });
        }
    }

    // -----------------------------------------------------------------
    // H3+H4 发布（须持 H1）：载荷先行、join 复位、CAS→(g+1,REQ)、wake_all
    // -----------------------------------------------------------------
    /// H3 原子（seqcst）写载荷 + join 槽复位 + H4 CAS（唯一 Release 发布点）
    /// + futex_wake_all（换代转移同样唤醒，I6）。
    pub fn h3_h4_publish<P: FacadePlatform>(
        &self,
        platform: &P,
        alloc: SlotAllocation,
        payload: usize,
        join_index: u32,
    ) {
        self.assert_h1_held("H3/H4");
        // H3：载荷先行（§12.1.2——闭包 Box 在锁外 (l) 内先行入共享堆，
        // 此处只落槽字；两锁无嵌套）。u64 承载（wasm32 上 hi 恒 0）。
        let payload_wide = payload as u64;
        self.slot_field_word(alloc.index as usize, SLOT_PAYLOAD_LO)
            .store(payload_wide as u32, Ordering::SeqCst);
        self.slot_field_word(alloc.index as usize, SLOT_PAYLOAD_HI)
            .store((payload_wide >> 32) as u32, Ordering::SeqCst);
        self.slot_field_word(alloc.index as usize, SLOT_JOIN_INDEX)
            .store(join_index, Ordering::SeqCst);
        // join 槽复位：分配者独占配对。合法旧值三形——新槽 0；CONSUMED
        // （OK 门 (b) 闭合后回收的对）；WAITING（CANCELLED/FAIL_ACKED 回收
        // 的对——spawn 失败无句柄 ⇒ 无 joiner，WAITING 即无人再触的死值；
        // Done/Dead 残留 = 协议外 ⇒ tombstone）。
        let jw_old = self.join_word(join_index).load(Ordering::SeqCst);
        let jw_old_ok = jw_old == 0
            || matches!(
                decode_join_state(jw_old, self.gen_bits),
                JoinState::Consumed | JoinState::Waiting
            );
        if !jw_old_ok {
            self.tombstone_panic(
                TombstoneCode::JoinResetMismatch,
                &format!("join 槽 {join_index} 旧代状态 {jw_old:#x} 非 CONSUMED（门链破坏）"),
            );
        }
        self.join_word(join_index).store(
            encode_join(alloc.new_gen, JoinState::Waiting, self.gen_bits),
            Ordering::SeqCst,
        );
        // H4：CAS 期望=完整旧字；失败 = 迟到写者 = 分配器 bug ⇒ tombstone。
        let new_w = encode_w(alloc.new_gen, SlotState::Req, self.gen_bits);
        if self
            .slot_field_word(alloc.index as usize, SLOT_W)
            .compare_exchange(alloc.expected_w, new_w, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            self.tombstone_panic(
                TombstoneCode::AllocExpectedMismatch,
                &format!(
                    "H4 CAS 期望失配：槽 {} 期望 {:#x} 实为 {:#x}（迟到写者 = 分配器 bug）",
                    alloc.index,
                    alloc.expected_w,
                    self.slot_field_word(alloc.index as usize, SLOT_W)
                        .load(Ordering::SeqCst)
                ),
            );
        }
        // 换代转移同样 wake_all（I6：阻塞在旧期望值上的旧 trampoline 由
        // 分配路径冲刷）。
        platform.wake_all_u32(self.slot_word_ptr(alloc.index));
    }

    // -----------------------------------------------------------------
    // 自回收 push（须持 H1；终态写者 = engine CANCELLED/FAIL_ACKED 与
    // supervisor OK 门链闭合后）
    // -----------------------------------------------------------------
    pub fn push_free(&self, index: u32) {
        self.assert_h1_held("push_free");
        let w = self
            .slot_field_word(index as usize, SLOT_W)
            .load(Ordering::SeqCst);
        match decode_state(w, self.gen_bits) {
            SlotState::Ok | SlotState::FailAcked | SlotState::Cancelled => {}
            SlotState::Retired => {
                self.tombstone_panic(
                    TombstoneCode::FreeListCorrupt,
                    &format!("退休槽 {index} 复活入表（push_free 拒绝）"),
                );
            }
            other => {
                self.tombstone_panic(
                    TombstoneCode::FreeListCorrupt,
                    &format!("push_free 槽 {index} 状态非法：{other:?}（未终态）"),
                );
            }
        }
        // LIFO 头插。
        let old_head = self.word(FREE_HEAD).load(Ordering::SeqCst);
        self.slot_field_word(index as usize, SLOT_FREE_NEXT)
            .store(old_head, Ordering::SeqCst);
        self.word(FREE_HEAD).store(index, Ordering::SeqCst);
        self.word(FREE_LEN).fetch_add(1, Ordering::SeqCst);
    }
}

const _: () = {
    // 布局静态断言：头部 + 双区不随平台漂移（TS 镜像依赖）。
    assert!(HEADER_WORDS == 16);
    assert!(SLOT_STRIDE == 7);
    assert!(JOIN_STRIDE == 4);
    assert!(GEN_BITS_DEFAULT == 29);
    // 指针宽度 tripwire（编译期、按目标分别钉）：载荷/join result 以 u64 双字
    // 承载——native 指针 64 位恰填满两字；wasm32 指针 32 位、hi 恒 0。
    // 若未来出现 128 位指针目标，两字装不下 ⇒ 此断言在编译期失败，
    // 而不是在运行期静默截断（2026-10-21 复核者要求）。
    if cfg!(target_pointer_width = "64") {
        assert!(std::mem::size_of::<usize>() == 8);
    } else if cfg!(target_pointer_width = "32") {
        assert!(std::mem::size_of::<usize>() == 4);
    } else {
        panic!("未登记的目标指针宽度：槽载荷双字承载仅支持 32/64 位指针");
    }
};
