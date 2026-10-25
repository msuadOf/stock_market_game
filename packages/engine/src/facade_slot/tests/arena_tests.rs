//! 槽 arena / H1 分配锁 / H2-H4 分配器 / RB 退休 / 四账测试。
//! 设计锚 §12.1.2（H2-H4 请求全序）、§12.1.3（H1 规格）、§12.1.6+§13.3+§14.1
//! （退休两类分账 + 可用账口径恒等式）、§11.2（SLOT_POOL_EXHAUSTED 失败面）。

use std::sync::atomic::Ordering;

use super::with_h1;
use crate::facade_slot::arena::{Exhausted, SlotAllocation, SlotArena};
use crate::facade_slot::codec::{encode_join, encode_w, GEN_BITS_DEFAULT, JoinState, SlotState};
use crate::facade_slot::platform::NullPlatform;

fn publish(arena: &SlotArena, alloc: SlotAllocation, payload: usize) {
    arena.h3_h4_publish(&NullPlatform, alloc, payload, alloc.index);
}

/// 新 arena 全零态：空闲表空、bump=0、退休计数 0、tombstone 0。
#[test]
fn 初始态四账全零可用满额() {
    let arena = SlotArena::alloc(4, GEN_BITS_DEFAULT);
    let a = arena.accounts();
    assert_eq!(a.active, 0);
    assert_eq!(a.free, 0);
    assert_eq!(a.unallocated, 4);
    assert_eq!(a.retired_fail_unacked, 0);
    assert_eq!(a.retired_gen_exhausted, 0);
    // §14.1 恒等式（可用账口径）：活跃+可用+RA+RB ≡ 容量。
    assert!(a.identity_holds(&arena), "初始态恒等式应成立：{a:?}");
    assert_eq!(a.available(), 4);
}

/// H2-H4：bump 新槽（首用 gen=0）→ CAS→(0,REQ) 唯一 Release 发布点。
#[test]
fn 分配新槽发布req() {
    let arena = SlotArena::alloc(2, GEN_BITS_DEFAULT);
    let alloc = with_h1(&arena, |a| a.h2_take(&NullPlatform).expect("bump 首槽应可得"));
    assert_eq!(alloc.index, 0);
    assert_eq!(alloc.new_gen, 0);
    assert_eq!(alloc.expected_w, 0, "新槽 W 初值必须为 0");
    with_h1(&arena, |a| publish(a, alloc, 0x1000));
    assert_eq!(
        arena.slot_word(0).load(Ordering::SeqCst),
        encode_w(0, SlotState::Req, GEN_BITS_DEFAULT)
    );
    // bump 已推进、槽划入活跃；join 槽同时复位为 (0,WAITING)。
    let a = arena.accounts();
    assert_eq!(a.active, 1);
    assert_eq!(a.unallocated, 1);
    assert_eq!(
        arena.join_word(0).load(Ordering::SeqCst),
        encode_join(0, JoinState::Waiting, GEN_BITS_DEFAULT)
    );
}

/// LIFO 复用：push 后 pop 必得同槽且 gen+1（N1 确定性前提）。
#[test]
fn 空闲表lifo复用gen递增() {
    let arena = SlotArena::alloc(2, GEN_BITS_DEFAULT);
    let alloc = with_h1(&arena, |a| a.h2_take(&NullPlatform).unwrap());
    with_h1(&arena, |a| publish(a, alloc, 1));
    // 模拟终态 (0,CANCELLED) 后自回收 push。
    arena
        .slot_word(0)
        .store(encode_w(0, SlotState::Cancelled, GEN_BITS_DEFAULT), Ordering::SeqCst);
    with_h1(&arena, |a| a.push_free(0));
    let again = with_h1(&arena, |a| a.h2_take(&NullPlatform).unwrap());
    assert_eq!(again.index, 0, "空闲表仅此一槽 ⇒ LIFO 必得同槽");
    assert_eq!(again.new_gen, 1, "复用 gen 严格 +1");
    assert_eq!(again.expected_w, encode_w(0, SlotState::Cancelled, GEN_BITS_DEFAULT));
}

/// RB 退休（§13.3 T-耗-1 内核）：gen_bits=4 ⇒ gen 至 15 后 pop 即退休、
/// 计数 retired_gen_exhausted、槽永不再入空闲表。
#[test]
fn gen耗尽槽退休永不复用() {
    let arena = SlotArena::alloc(2, 4);
    // 槽 0 先经合法分配（bump），再伪造终态 (15,FAIL_ACKED) 入表（gen 至
    // 2^4−1 = 15 次复用后的合法位形，直接置值省略 16 轮全链）。
    let first = with_h1(&arena, |a| a.h2_take(&NullPlatform).unwrap());
    assert_eq!(first.index, 0);
    arena
        .slot_word(0)
        .store(encode_w(15, SlotState::FailAcked, 4), Ordering::SeqCst);
    with_h1(&arena, |a| a.push_free(0));
    let next = with_h1(&arena, |a| a.h2_take(&NullPlatform).expect("槽 0 退休后应得槽 1"));
    assert_eq!(next.index, 1, "退休槽不可再分配，必得他槽");
    assert_eq!(arena.retired_gen_exhausted_word().load(Ordering::SeqCst), 1);
    assert_eq!(
        arena.slot_word(0).load(Ordering::SeqCst),
        encode_w(15, SlotState::Retired, 4),
        "退休槽 W 应钉在 (max_gen,RETIRED)"
    );
    // W=RETIRED 的 push 必须显式拒绝（复活 = 分配器 bug）。
    // 注：max-gen 终态槽入表合法（退休发生在下一次 pop），push 按状态拒绝。
    let hit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_h1(&arena, |a| a.push_free(0));
    }));
    assert!(hit.is_err(), "退休槽入表必须显式失败");
    // 非终态槽 push（如 REQ 在途）同样必须拒绝。
    arena
        .slot_word(1)
        .store(encode_w(0, SlotState::Req, 4), Ordering::SeqCst);
    let hit2 = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_h1(&arena, |a| a.push_free(1));
    }));
    assert!(hit2.is_err(), "非终态槽入表必须显式失败");
}

/// 空闲表空 ∧ bump=容量 ⇒ SLOT_POOL_EXHAUSTED 结构化失败（不 panic），
/// 错误载荷携带 arena 级 RA/RB 两计数（§13.3/§14.1）。
#[test]
fn 池耗尽显式err携带两计数() {
    let arena = SlotArena::alloc(1, 4);
    let alloc = with_h1(&arena, |a| a.h2_take(&NullPlatform).unwrap());
    with_h1(&arena, |a| publish(a, alloc, 1));
    match with_h1(&arena, |a| a.h2_take(&NullPlatform)) {
        Err(Exhausted { retired_fail_unacked, retired_gen_exhausted }) => {
            assert_eq!(retired_fail_unacked, 0);
            assert_eq!(retired_gen_exhausted, 0);
        }
        Ok(_) => panic!("唯一槽在途时再分配必须显式耗尽"),
    }
}

/// H4 CAS 失败 = 迟到写者 = 分配器 bug ⇒ 显式 panic→tombstone（§11.2 验收③）。
#[test]
fn h4期望失配tombstone() {
    let arena = SlotArena::alloc(1, GEN_BITS_DEFAULT);
    let alloc = with_h1(&arena, |a| a.h2_take(&NullPlatform).unwrap());
    // H2 与 H4 之间注入"迟到写者"改写 W（测试直接 store 模拟分配器 bug）。
    arena
        .slot_word(0)
        .store(encode_w(5, SlotState::Req, GEN_BITS_DEFAULT), Ordering::SeqCst);
    let hit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_h1(&arena, |a| publish(a, alloc, 1));
    }));
    assert!(hit.is_err(), "H4 期望失配必须 panic 而非静默复用");
    assert_ne!(
        arena.tombstone_word().load(Ordering::SeqCst),
        0,
        "panic 前必须先落 tombstone 字"
    );
}

/// H1 分配锁：try_lock 快路径语义与互斥。
#[test]
fn h1锁互斥与try_lock() {
    let arena = SlotArena::alloc(2, GEN_BITS_DEFAULT);
    let platform = NullPlatform;
    assert!(arena.h1_try_lock(), "无竞争 try-lock 应成功");
    assert!(!arena.h1_try_lock(), "已持锁再 try 应返回 false");
    arena.h1_unlock(&platform);
    assert!(arena.h1_try_lock());
    arena.h1_unlock(&platform);
}

/// 指针双字承载：usize 全宽往返（native 64 位 hi 非零；wasm32 上 hi 恒 0）。
/// 回归锚 = wasm32 `usize` 仅 32 位时 `ptr >> 32` 编译期溢出（实测红）。
#[test]
fn 指针双字承载全宽往返() {
    let arena = SlotArena::alloc(1, GEN_BITS_DEFAULT);
    let addr = usize::MAX - 0x1234;
    arena.set_join_result(0, addr);
    assert_eq!(arena.join_result(0), addr, "join result 指针须全宽往返");
    let alloc = with_h1(&arena, |a| a.h2_take(&NullPlatform).unwrap());
    with_h1(&arena, |a| publish(a, alloc, addr));
    assert_eq!(arena.payload_ptr(0), addr, "载荷指针须全宽往返");
}

/// 四账五态闭合枚举：任意时点每槽恰居其一（§14.1）。
#[test]
fn 四账恒等式随分配回收演进() {
    let arena = SlotArena::alloc(3, GEN_BITS_DEFAULT);
    let a0 = with_h1(&arena, |a| a.h2_take(&NullPlatform).unwrap());
    with_h1(&arena, |a| publish(a, a0, 1));
    let a1 = with_h1(&arena, |a| a.h2_take(&NullPlatform).unwrap());
    with_h1(&arena, |a| publish(a, a1, 1));
    // a0 达终态 CANCELLED 并自回收。
    arena
        .slot_word(0)
        .store(encode_w(0, SlotState::Cancelled, GEN_BITS_DEFAULT), Ordering::SeqCst);
    with_h1(&arena, |a| a.push_free(0));
    let acc = arena.accounts();
    assert_eq!(acc.active, 1, "槽 1 在途");
    assert_eq!(acc.free, 1, "槽 0 已回收");
    assert_eq!(acc.unallocated, 1);
    assert!(acc.identity_holds(&arena));
    // FAIL 未 ACK ⇒ RA 账（计数器由监督者 FAIL 写者递增，见 lifecycle 测试）。
    arena
        .slot_word(1)
        .store(encode_w(0, SlotState::Fail, GEN_BITS_DEFAULT), Ordering::SeqCst);
    arena
        .retired_fail_unacked_word()
        .fetch_add(1, Ordering::SeqCst);
    let acc = arena.accounts();
    assert_eq!(acc.retired_fail_unacked, 1);
    assert_eq!(acc.active, 0);
    assert!(acc.identity_holds(&arena));
}

/// I6 唤醒纪律（复核补记）：一切 W 转移后必须 wake_all。此前测试平台的
/// wake 是 no-op ⇒ 这条纪律在 native 面完全不可观测；现经 `wake_count`
/// 断言退休转移同样唤醒（旧 trampoline 在 u64::MAX 等待上不得永挂）。
#[test]
fn i6_退休转移同样唤醒() {
    use super::TestPlatform;
    let platform = TestPlatform::default();
    let gb = 4u32;
    // 容量 3：槽 0（伪造终态后被退休）+ 槽 1（退休后首个可取槽，断言用）+
    // 槽 2（H4 发布对照组）。容量 2 时槽 0 退休、槽 1 取走后 bump 已达上限，
    // 对照组第三次 h2_take 直接 Exhausted——该 fixture 在编译修复前从未真正
    // 执行过，故这个尺寸缺陷此前未暴露。
    let arena = SlotArena::alloc(3, gb);
    // 槽 0 先经合法分配（bump gen0），再伪造终态 (15,FAIL_ACKED) 入表。
    let first = with_h1(&arena, |a| a.h2_take(&NullPlatform).unwrap());
    assert_eq!(first.index, 0);
    arena
        .slot_word(0)
        .store(encode_w(15, SlotState::FailAcked, gb), Ordering::SeqCst);
    with_h1(&arena, |a| a.push_free(0));

    // 退休发生在这次 h2_take：槽 0 gen=15 达上界 ⇒ 钉 RETIRED + wake。
    let before = platform.wake_count(arena.slot_word_ptr(0));
    platform.h1_lock_for_test(&arena);
    let next = arena.h2_take(&platform).expect("槽 0 退休后应得槽 1");
    platform.h1_unlock_for_test(&arena);
    assert_eq!(next.index, 1, "退休槽不可再分配");
    assert_eq!(arena.retired_gen_exhausted_word().load(Ordering::SeqCst), 1);
    assert_eq!(
        platform.wake_count(arena.slot_word_ptr(0)),
        before + 1,
        "I6：退休也是一次 W 转移，必须 wake_all"
    );
    // 对照：H4 发布同样 wake（既有纪律，用同一观测面钉住）。
    platform.h1_lock_for_test(&arena);
    let alloc = arena.h2_take(&platform).unwrap();
    arena.h3_h4_publish(&platform, alloc, 1, alloc.index);
    platform.h1_unlock_for_test(&arena);
    assert_eq!(
        platform.wake_count(arena.slot_word_ptr(alloc.index)),
        1,
        "I6：H4 发布转移必须 wake_all"
    );
}
