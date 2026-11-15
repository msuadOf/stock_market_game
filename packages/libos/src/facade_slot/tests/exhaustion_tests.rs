//! 连续耗尽测试 T-耗-1~4（§13.3 + §14.1 可用账口径四账恒等式）。
//!
//! 操作级驱动（无阻塞等待角色）：发布→应答→trampoline 内联→join 内联→
//! workerExit 门链回收，循环推进到显式失败面。

use std::sync::Arc;

use super::lifecycle_tests::{counted_job, JobCounters};
use super::TestPlatform;
use crate::facade_slot::arena::{Exhausted, SlotArena};
use crate::facade_slot::codec::*;
use crate::facade_slot::roles::*;
use crate::facade_slot::test_supervisor;

/// 完整健康周期（操作级）：发布→OK→trampoline→join→回收。返回 (slot, gen)。
fn healthy_cycle(
    arena: &SlotArena,
    platform: &TestPlatform,
    sup: &mut test_supervisor::SupervisorState,
    counters: &Arc<JobCounters>,
    worker_id: u32,
) -> (u32, u32) {
    let job = counted_job("cycle", counters);
    let published = publish_spawn(arena, platform, job).expect("健康路径发布应成功");
    test_supervisor::answer_ok(arena, platform, published.slot, published.gen, worker_id);
    let out = trampoline_run(arena, platform, published.slot, published.gen, worker_id);
    assert!(matches!(out, TrampolineExit::Ran));
    let (): () = join_typed(arena, platform, published.slot, published.gen).expect("join");
    match test_supervisor::on_worker_exit(arena, platform, sup, published.slot, published.gen) {
        test_supervisor::RecoveryOutcome::Pushed => {}
        other => panic!("健康路径应回收：{other:?}"),
    }
    (published.slot, published.gen)
}

/// T-耗-1 单槽连续耗尽：gen_bits=4 ⇒ 每槽 15 次复用即耗尽；恰在第 15 次
/// 复用后该槽转 RETIRED、retired_gen_exhausted=1、RETIRED 槽此后绝不出现在
/// 空闲表（canary：后续分配必得他槽）。
#[test]
fn t耗1_单槽连续耗尽() {
    let arena = SlotArena::alloc(2, 4);
    let platform = TestPlatform::default();
    let counters = Arc::new(JobCounters::default());
    let mut sup = test_supervisor::SupervisorState::default();
    // LIFO：同一槽反复复用——首用 gen0 + 15 次复用（gen1..15）= 16 次全链。
    for round in 0..16u32 {
        let last = healthy_cycle(&arena, &platform, &mut sup, &counters, round);
        assert_eq!(last.0, 0, "LIFO 下全部落同槽");
        assert_eq!(last.1, round, "gen 逐轮 +1");
        assert_eq!(
            arena
                .retired_gen_exhausted_word()
                .load(std::sync::atomic::Ordering::SeqCst),
            0
        );
    }
    // 第 17 次分配：pop 出 gen=15（=2^4−1）⇒ 退休，必得他槽。
    let job = counted_job("t1", &counters);
    let published = publish_spawn(&arena, &platform, job).expect("退休后应得他槽");
    assert_eq!(published.slot, 1, "canary：后续分配必得他槽");
    assert_eq!(published.gen, 0);
    assert_eq!(
        arena
            .retired_gen_exhausted_word()
            .load(std::sync::atomic::Ordering::SeqCst),
        1,
        "恰在第 15 次复用后转 RETIRED"
    );
    assert_eq!(
        arena.slot_word(0).load(std::sync::atomic::Ordering::SeqCst),
        encode_w(15, SlotState::Retired, 4)
    );
    // 恒等式仍平。
    assert!(arena.accounts().identity_holds(&arena));
    assert_eq!(counters.run.load(std::sync::atomic::Ordering::SeqCst), 16);
}

/// T-耗-2 全池连续耗尽到显式失败：健康路径循环 spawn/join 直至
/// SLOT_POOL_EXHAUSTED ⇒ 结构化 Err（非 panic、非挂死）∧ 恒等式吻合
/// （可用=0 即空闲表空 ∧ bump=容量）∧ RA=0（健康耗尽不产生垂死退休）。
#[test]
fn t耗2_全池耗尽显式失败恒等式() {
    let arena = SlotArena::alloc(2, 4);
    let platform = TestPlatform::default();
    let counters = Arc::new(JobCounters::default());
    let mut sup = test_supervisor::SupervisorState::default();
    let mut spawned = 0u32;
    loop {
        let job = counted_job("t2", &counters);
        match publish_spawn(&arena, &platform, job) {
            Ok(published) => {
                test_supervisor::answer_ok(
                    &arena,
                    &platform,
                    published.slot,
                    published.gen,
                    spawned,
                );
                let out = trampoline_run(&arena, &platform, published.slot, published.gen, spawned);
                assert!(matches!(out, TrampolineExit::Ran));
                let (): () = join_typed(&arena, &platform, published.slot, published.gen).unwrap();
                match test_supervisor::on_worker_exit(
                    &arena,
                    &platform,
                    &mut sup,
                    published.slot,
                    published.gen,
                ) {
                    test_supervisor::RecoveryOutcome::Pushed => {}
                    other => panic!("健康路径应回收：{other:?}"),
                }
                spawned += 1;
            }
            Err(SpawnFailure::SlotPoolExhausted {
                retired_fail_unacked: ra,
                retired_gen_exhausted: rb,
            }) => {
                assert_eq!(ra, 0, "两类分账直接断言：健康耗尽 RA=0");
                assert_eq!(rb, 2, "两槽全退休");
                break;
            }
            Err(other) => panic!("耗尽前不应有其他失败：{other:?}"),
        }
    }
    // 16 次/槽 × 2 槽 = 32 次全链。
    assert_eq!(spawned, 32);
    let acc = arena.accounts();
    assert_eq!(acc.available(), 0, "可用=0（空闲表空 ∧ bump=容量）");
    assert_eq!(acc.unallocated, 0);
    assert_eq!(acc.free, 0);
    assert_eq!(acc.active, 0);
    assert_eq!(acc.retired_fail_unacked, 0);
    assert_eq!(acc.retired_gen_exhausted, 2);
    assert!(acc.identity_holds(&arena), "四账恒等式吻合（可用账口径）");
}

/// T-耗-3 换实例重置（恢复式断言）：失败面触发 → 新 arena 断言
/// RA=0 ∧ RB=0 ∧ 活跃=0 ∧ 可用=容量（bump=0 ∧ 空闲表空）——恒等式以
/// "可用=容量"满足；游戏链路继续（新 arena 上一轮全链成功）。
#[test]
fn t耗3_换实例重置恢复式断言() {
    let arena = SlotArena::alloc(1, 4);
    let platform = TestPlatform::default();
    let counters = Arc::new(JobCounters::default());
    let mut sup = test_supervisor::SupervisorState::default();
    // 推进到耗尽。
    loop {
        let job = counted_job("t3", &counters);
        match publish_spawn(&arena, &platform, job) {
            Ok(p) => {
                test_supervisor::answer_ok(&arena, &platform, p.slot, p.gen, 1);
                trampoline_run(&arena, &platform, p.slot, p.gen, 1);
                let (): () = join_typed(&arena, &platform, p.slot, p.gen).unwrap();
                test_supervisor::on_worker_exit(&arena, &platform, &mut sup, p.slot, p.gen);
            }
            Err(SpawnFailure::SlotPoolExhausted { .. }) => break,
            Err(o) => panic!("{o:?}"),
        }
    }
    // 换实例（R2-3/换局① 的新 memory ⇒ 新 arena）。
    let arena2 = SlotArena::alloc(1, 4);
    let acc = arena2.accounts();
    assert_eq!(acc.retired_fail_unacked, 0);
    assert_eq!(acc.retired_gen_exhausted, 0);
    assert_eq!(acc.active, 0);
    assert_eq!(acc.available(), 1, "恢复式：可用=容量（非全零）");
    assert!(acc.identity_holds(&arena2));
    // 游戏链路继续：新 arena 上一轮全链成功。
    let mut sup2 = test_supervisor::SupervisorState::default();
    let (slot, gen) = healthy_cycle(&arena2, &platform, &mut sup2, &counters, 42);
    assert_eq!((slot, gen), (0, 0));
}

/// T-耗-4 混合分账：注入 RA（FAIL 后 engine 冻结不 ACK）×k 与 T-缩 gen
/// 耗尽并行推进 ⇒ 两类计数器互不串账、恒等式仍平、错误载荷计数与注入数
/// 一致、canary：RA 注入槽退役后绝不出现在可用账。
#[test]
fn t耗4_混合分账互不串账() {
    let arena = SlotArena::alloc(3, 4);
    let platform = TestPlatform::default();
    let counters = Arc::new(JobCounters::default());
    let mut sup = test_supervisor::SupervisorState::default();
    let k = 2u32;
    // 阶段 1：注入 k 个 RA（FAIL 后无人 ACK——engine 冻结）。
    let mut ra_slots = Vec::new();
    for i in 0..k {
        let job = counted_job("ra", &counters);
        let p = publish_spawn(&arena, &platform, job).expect("RA 注入发布");
        test_supervisor::answer_fail(&arena, &platform, p.slot, p.gen, 100 + i);
        ra_slots.push(p.slot);
        // 注入语义：engine 楔死 ⇒ 不 ACK、不自回收（闭包滞留为冻结语义的一部分，
        // 测试计数不计 drop——本断言面只看账目不串）。
    }
    assert_eq!(
        arena
            .retired_fail_unacked_word()
            .load(std::sync::atomic::Ordering::SeqCst),
        k
    );
    // 阶段 2：剩余槽健康循环至 RB 耗尽。
    let exhausted = loop {
        let job = counted_job("mix", &counters);
        match publish_spawn(&arena, &platform, job) {
            Ok(p) => {
                assert!(
                    !ra_slots.contains(&p.slot),
                    "canary：RA 注入槽绝不出现在可用账（分配必得他槽）"
                );
                test_supervisor::answer_ok(&arena, &platform, p.slot, p.gen, 200);
                trampoline_run(&arena, &platform, p.slot, p.gen, 200);
                let (): () = join_typed(&arena, &platform, p.slot, p.gen).unwrap();
                test_supervisor::on_worker_exit(&arena, &platform, &mut sup, p.slot, p.gen);
            }
            Err(SpawnFailure::SlotPoolExhausted {
                retired_fail_unacked: ra,
                retired_gen_exhausted: rb,
            }) => {
                break (ra, rb);
            }
            Err(o) => panic!("{o:?}"),
        }
    };
    // 两类计数互不串账：错误载荷计数 = 注入数；RB = 全部剩余槽。
    let (ra, rb) = exhausted;
    assert_eq!(ra, k, "错误载荷 RA 计数与注入数一致");
    assert_eq!(rb, 3 - k);
    let acc = arena.accounts();
    assert_eq!(acc.retired_fail_unacked, k);
    assert_eq!(acc.retired_gen_exhausted, 3 - k);
    assert_eq!(acc.available(), 0);
    assert_eq!(acc.active, 0);
    assert!(acc.identity_holds(&arena), "恒等式（可用账口径）仍平");
    // RA 槽账面状态仍为 FAIL（冻结、未 ACK）。
    for slot in ra_slots {
        assert_eq!(
            decode_state(
                arena
                    .slot_word(slot)
                    .load(std::sync::atomic::Ordering::SeqCst),
                4
            ),
            SlotState::Fail
        );
    }
}

/// 恒等式在 RA 注入中点也成立（非耗尽终态的静点核对）。
#[test]
fn 恒等式ra注入中点仍平() {
    let arena = SlotArena::alloc(2, GEN_BITS_DEFAULT);
    let platform = TestPlatform::default();
    let counters = Arc::new(JobCounters::default());
    let job = counted_job("mid", &counters);
    let p = publish_spawn(&arena, &platform, job).expect("发布");
    test_supervisor::answer_fail(&arena, &platform, p.slot, p.gen, 1);
    let acc = arena.accounts();
    assert_eq!(acc.active, 0);
    assert_eq!(acc.retired_fail_unacked, 1);
    assert!(acc.identity_holds(&arena));
    let _: Option<Exhausted> = None;
}
