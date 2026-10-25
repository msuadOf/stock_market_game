//! 角色操作正测（请求者/trampoline/监督者测试驱动/join）。
//! 设计锚 §12.1.2（发布序）、§12.1.3（自回收）、§13.1（A1-A3/谓词等待）、
//! §9.2.3（join 单向状态机）、§9.2.1（Box 所有权表）。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

use super::TestPlatform;
use crate::facade_slot::arena::AnswerField;
use crate::facade_slot::arena::SlotArena;
use crate::facade_slot::codec::*;
use crate::facade_slot::platform::NullPlatform;
use crate::facade_slot::roles::*;
use crate::facade_slot::test_supervisor;

/// 闭包计数器（Box 所有权表断言：run/drop 各恰一次的载体；每代闭包各持
/// 一份——N1 需分代断言）。
#[derive(Default)]
pub(crate) struct JobCounters {
    pub run: AtomicUsize,
    pub drop: AtomicUsize,
}

/// Drop 计数守卫（闭包环境析构 ⇒ drop+1：OK 臂随 invoke 消费析构、
/// FAIL/CANCELLED 臂由 engine 就地 drop——§9.2.1 "drop 恰一次"两态统一计）。
struct DropGuard(Arc<JobCounters>);

impl Drop for DropGuard {
    fn drop(&mut self) {
        self.0.drop.fetch_add(1, Ordering::SeqCst);
    }
}

/// 生成带 run/drop 计数的擦除闭包（T=()）。
pub(crate) fn counted_job(tag: &'static str, counters: &Arc<JobCounters>) -> Box<SpawnJob> {
    let guard = DropGuard(Arc::clone(counters));
    let run_counters = Arc::clone(counters);
    SpawnJob::from_closure(
        move || {
            drop(guard); // 闭包环境析构（等价闭包本体 drop）
            run_counters.run.fetch_add(1, Ordering::SeqCst);
        },
        format!("lifecycle-{tag}"),
    )
}

/// 全链正测：spawn→构造→OK 应答→trampoline 消费→join DONE→CONSUMED。
/// Box 所有权：run=1、drop=0（闭包随 invoke 消费）、T 恰一次回传。
#[test]
fn 全链ok_闭包恰跑一次join取回结果() {
    let arena = Arc::new(SlotArena::alloc(2, GEN_BITS_DEFAULT));
    let platform = Arc::new(TestPlatform::default());
    let counters = Arc::new(JobCounters::default());
    let job = counted_job("g0", &counters);
    let (a2, p2) = (Arc::clone(&arena), Arc::clone(&platform));
    let requester = thread::spawn(move || request_spawn::<(), _>(&a2, &p2, job, 10_000));
    let (slot, gen, _name) = platform.wait_notify_spawn(0);
    assert_eq!((slot, gen), (0, 0));
    // 子构造（trampoline 线程进入等待）。
    let (a3, p3) = (Arc::clone(&arena), Arc::clone(&platform));
    let child = thread::spawn(move || trampoline_run(&a3, &p3, 0, 0, 7));
    let outcome = test_supervisor::answer_ok(&arena, &platform, 0, 0, 7);
    assert!(matches!(outcome, test_supervisor::AnswerOutcome::Answered));
    let handle = requester
        .join()
        .expect("请求者线程不应 panic")
        .expect("全链 OK 应返回句柄");
    let child_out = child.join().expect("trampoline 不应 panic");
    assert!(matches!(child_out, TrampolineExit::Ran { .. }));
    let (): () =
        join_typed(&arena, &platform, handle.join_index, handle.gen).expect("join DONE 应取回结果");
    // join 槽 → CONSUMED。
    assert_eq!(
        arena.join_word(0).load(Ordering::SeqCst),
        encode_join(0, JoinState::Consumed, GEN_BITS_DEFAULT)
    );
    assert_eq!(
        platform.wake_count(arena.slot_word_ptr(0)),
        2,
        "I6：H4与OK发布各唤醒一次"
    );
    assert_eq!(
        platform.wake_count(arena.join_word_ptr(0)),
        2,
        "I6：DONE与CONSUMED各唤醒一次"
    );
    assert_eq!(counters.run.load(Ordering::SeqCst), 1);
    assert_eq!(
        counters.drop.load(Ordering::SeqCst),
        1,
        "OK 臂闭包随 invoke 消费析构（§9.2.1 drop 恰一次）"
    );
    assert_eq!(arena.tombstone_word().load(Ordering::SeqCst), 0);
}

/// FAIL 链：构造失败应答 → engine 读 canary 错误码 → FAIL_ACKED 自回收。
/// RA 计数：监督者写 FAIL 时 +1、engine ACK 回收时 −1 ⇒ 终态 0；闭包 drop=1。
#[test]
fn fail链_错误码移交与自回收() {
    let arena = Arc::new(SlotArena::alloc(2, GEN_BITS_DEFAULT));
    let platform = Arc::new(TestPlatform::default());
    let counters = Arc::new(JobCounters::default());
    let job = counted_job("f0", &counters);
    let (a2, p2) = (Arc::clone(&arena), Arc::clone(&platform));
    let requester = thread::spawn(move || request_spawn::<(), _>(&a2, &p2, job, 10_000));
    platform.wait_notify_spawn(0);
    platform.seams.b.arm();
    let outcome = test_supervisor::answer_fail(&arena, &platform, 0, 0, 0xC0DE);
    assert!(matches!(outcome, test_supervisor::AnswerOutcome::Answered));
    assert_eq!(
        arena.retired_fail_unacked_word().load(Ordering::SeqCst),
        1,
        "FAIL 写者应把槽计入 RA 账"
    );
    platform.seams.b.release_once();
    let result = requester.join().expect("请求者不应 panic");
    match result {
        Err(SpawnFailure::WorkerFailed { errcode }) => assert_eq!(errcode, 0xC0DE),
        other => panic!("FAIL 链应返回 WorkerFailed：{other:?}"),
    }
    assert_eq!(
        counters.drop.load(Ordering::SeqCst),
        1,
        "FAIL 臂闭包由 engine drop 恰一次"
    );
    assert_eq!(
        arena.retired_fail_unacked_word().load(Ordering::SeqCst),
        0,
        "ACK 后 RA 归零"
    );
    let acc = arena.accounts();
    assert_eq!(acc.free, 1, "FAIL_ACKED 后自回收入空闲表");
    assert!(acc.identity_holds(&arena));
    assert_eq!(
        platform.wake_count(arena.slot_word_ptr(0)),
        3,
        "I6：H4、FAIL、FAIL_ACKED各唤醒一次"
    );
}

/// CANCELLED 链：虚拟截止注入 → 取消 CAS → 自回收 → Err(AckTimeout)；
/// trampoline 醒后 Superseded 不解引用（run=0、drop 不再增长）。
#[test]
fn cancelled链_超时取消自回收() {
    let arena = Arc::new(SlotArena::alloc(2, GEN_BITS_DEFAULT));
    let platform = Arc::new(TestPlatform::default());
    let counters = Arc::new(JobCounters::default());
    let job = counted_job("c0", &counters);
    let (a2, p2) = (Arc::clone(&arena), Arc::clone(&platform));
    let requester = thread::spawn(move || request_spawn::<(), _>(&a2, &p2, job, 10_000));
    platform.wait_notify_spawn(0);
    let (a3, p3) = (Arc::clone(&arena), Arc::clone(&platform));
    let child = thread::spawn(move || trampoline_run(&a3, &p3, 0, 0, 3));
    // T-缩 到点：注入虚拟截止。
    platform.fire_deadline(arena.slot_word_ptr(0));
    let result = requester.join().expect("请求者不应 panic");
    assert!(matches!(result, Err(SpawnFailure::AckTimeout)));
    assert_eq!(
        counters.drop.load(Ordering::SeqCst),
        1,
        "CANCELLED 臂闭包由 engine drop 恰一次"
    );
    assert_eq!(
        arena.slot_word(0).load(Ordering::SeqCst),
        encode_w(0, SlotState::Cancelled, GEN_BITS_DEFAULT)
    );
    let acc = arena.accounts();
    assert_eq!(acc.free, 1, "CANCELLED 即刻自回收");
    let child_out = child.join().expect("trampoline 不应 panic");
    assert!(matches!(child_out, TrampolineExit::Superseded));
    assert_eq!(counters.run.load(Ordering::SeqCst), 0);
    assert_eq!(
        counters.drop.load(Ordering::SeqCst),
        1,
        "Superseded 不得触碰闭包（无二次 drop）"
    );
    let _ = platform.wait_worker_exit();
    assert_eq!(
        platform.wake_count(arena.slot_word_ptr(0)),
        2,
        "I6：H4与超时CANCEL各唤醒一次"
    );
}

/// join DEAD_HARDTRAP（D5）：监督者写死亡信号 → join Err；CONSUMED 后迟到写丢弃。
#[test]
fn join死亡信号与consumed输家() {
    let arena = Arc::new(SlotArena::alloc(2, GEN_BITS_DEFAULT));
    let platform = Arc::new(TestPlatform::default());
    super::with_h1(&arena, |a| {
        let alloc = a.h2_take(&NullPlatform).unwrap();
        a.h3_h4_publish(&platform, alloc, 0, 0);
    });
    // D5：WAITING → DEAD_HARDTRAP。
    let out = test_supervisor::write_dead(
        &arena,
        &platform,
        0,
        0,
        test_supervisor::DeadKind::HardTrap,
        0xD5,
    );
    assert!(matches!(out, test_supervisor::DeadOutcome::Written));
    let joined: Result<(), _> = join_typed(&arena, &platform, 0, 0);
    let err = joined.expect_err("DEAD 应使 join 失败");
    assert!(
        format!("{err}").contains("0xD5"),
        "死亡信号细节应随错误面：{err}"
    );
    assert_eq!(
        arena.join_word(0).load(Ordering::SeqCst),
        encode_join(0, JoinState::Consumed, GEN_BITS_DEFAULT),
        "DEAD 消费后 CONSUMED，迟到写者 CAS 必败"
    );
    // 迟到 DEAD 写（已 CONSUMED）：CAS 输家丢弃、不改写结果。
    let late =
        test_supervisor::write_dead(&arena, &platform, 0, 0, test_supervisor::DeadKind::Panic, 1);
    assert!(matches!(late, test_supervisor::DeadOutcome::LostToConsumed));
    assert_eq!(
        arena.join_word(0).load(Ordering::SeqCst),
        encode_join(0, JoinState::Consumed, GEN_BITS_DEFAULT)
    );
}

/// 迟到 D5（复核补记）：旧代死亡信号撞上已复用的 join 槽 ⇒ 过期即弃，
/// **不得 panic**（此前输家分支只判 state 不判 gen，(g+1,WAITING) 会硬崩）。
#[test]
fn 迟到d5撞换代过期即弃() {
    let arena = Arc::new(SlotArena::alloc(2, GEN_BITS_DEFAULT));
    let platform = Arc::new(TestPlatform::default());
    super::with_h1(&arena, |a| {
        let alloc = a.h2_take(&NullPlatform).unwrap();
        a.h3_h4_publish(&platform, alloc, 0, 0);
    });
    // join 槽已复用为新一代 (1,WAITING)（等价 H3 复位后的位形）。
    arena.join_word(0).store(
        encode_join(1, JoinState::Waiting, GEN_BITS_DEFAULT),
        Ordering::SeqCst,
    );
    let out = test_supervisor::write_dead(
        &arena,
        &platform,
        0,
        0, // 旧代 g=0 的死亡信号
        test_supervisor::DeadKind::HardTrap,
        0xD5,
    );
    assert!(matches!(
        out,
        test_supervisor::DeadOutcome::LostToStaleGeneration
    ));
    assert_eq!(
        arena.tombstone_word().load(Ordering::SeqCst),
        0,
        "迟到 D5 不得落 tombstone"
    );
    assert_eq!(
        arena.join_word(0).load(Ordering::SeqCst),
        encode_join(1, JoinState::Waiting, GEN_BITS_DEFAULT),
        "迟到的旧代写不得改写新代 join 字"
    );
}

/// OK 回收三门（§10.2.1-4 + §12.1.3）：门 (b) join CONSUMED 未闭合 ⇒ 不回收；
/// 闭合后 push；H1 try-lock 失败 ⇒ 延迟回收登记，检查点重试成功。
#[test]
fn ok回收三门与延迟登记() {
    let arena = Arc::new(SlotArena::alloc(2, GEN_BITS_DEFAULT));
    let platform = Arc::new(TestPlatform::default());
    super::with_h1(&arena, |a| {
        let alloc = a.h2_take(&NullPlatform).unwrap();
        a.h3_h4_publish(&platform, alloc, 0, 0);
    });
    test_supervisor::answer_ok(&arena, &platform, 0, 0, 1);
    let mut state = test_supervisor::SupervisorState::default();
    // 门 (b) 未闭合（join 未消费）。
    match test_supervisor::on_worker_exit(&arena, &platform, &mut state, 0, 0) {
        test_supervisor::RecoveryOutcome::GatesNotMet => {}
        other => panic!("join 未 CONSUMED 时不得回收：{other:?}"),
    }
    assert_eq!(arena.accounts().free, 0);
    // 门 (b) 闭合：写真实 result 并 DONE → join 消费 → CONSUMED。
    let result_box = Box::into_raw(Box::new(())) as usize;
    arena.set_join_result(0, result_box);
    arena.join_word(0).store(
        encode_join(0, JoinState::Done, GEN_BITS_DEFAULT),
        Ordering::SeqCst,
    );
    let (a2, p2) = (Arc::clone(&arena), Arc::clone(&platform));
    let joiner = thread::spawn(move || join_typed::<(), _>(&a2, &p2, 0, 0));
    joiner
        .join()
        .expect("join 线程不应 panic")
        .expect("join DONE 应取回 result");
    assert_eq!(
        arena.join_word(0).load(Ordering::SeqCst),
        encode_join(0, JoinState::Consumed, GEN_BITS_DEFAULT)
    );
    match test_supervisor::on_worker_exit(&arena, &platform, &mut state, 0, 0) {
        test_supervisor::RecoveryOutcome::Pushed => {}
        other => panic!("三门闭合应回收：{other:?}"),
    }
    assert_eq!(arena.accounts().free, 1);
    // 延迟登记：再走一对全链到 CONSUMED（LIFO 复得槽 0、gen=1），但预持
    // H1 锁使 supervisor try-lock 失败。
    let (slot2, gen2) = super::with_h1(&arena, |a| {
        let alloc2 = a.h2_take(&NullPlatform).unwrap();
        let (s, g) = (alloc2.index, alloc2.new_gen);
        a.h3_h4_publish(&platform, alloc2, 0, s);
        (s, g)
    });
    test_supervisor::answer_ok(&arena, &platform, slot2, gen2, 2);
    let result2 = Box::into_raw(Box::new(())) as usize;
    arena.set_join_result(slot2, result2);
    arena.join_word(slot2).store(
        encode_join(gen2, JoinState::Done, GEN_BITS_DEFAULT),
        Ordering::SeqCst,
    );
    let (a3, p3) = (Arc::clone(&arena), Arc::clone(&platform));
    let joiner2 = thread::spawn(move || join_typed::<(), _>(&a3, &p3, slot2, gen2));
    joiner2
        .join()
        .expect("join 线程不应 panic")
        .expect("join DONE 应取回 result");
    assert!(arena.h1_try_lock(), "预持 H1");
    match test_supervisor::on_worker_exit(&arena, &platform, &mut state, slot2, gen2) {
        test_supervisor::RecoveryOutcome::Delayed => {}
        other => panic!("try-lock 失败应转延迟登记：{other:?}"),
    }
    assert_eq!(
        arena.accounts().free,
        0,
        "延迟期间不入表（周期 2 槽已被 LIFO 取走在途）"
    );
    arena.h1_unlock(&platform);
    assert_eq!(
        test_supervisor::recover_pending(&arena, &platform, &mut state),
        1
    );
    assert_eq!(arena.accounts().free, 1, "检查点重试成功");
    assert!(arena.accounts().identity_holds(&arena));
}

/// 迟到应答（N3 核心，操作级）：换代后 A1 期望失配 ⇒ 整体丢弃、零载荷写 +
/// 对迟到构造子的 terminate 卫生义务。
#[test]
fn 迟到应答零槽写与terminate卫生() {
    let arena = Arc::new(SlotArena::alloc(2, GEN_BITS_DEFAULT));
    let platform = Arc::new(TestPlatform::default());
    let counters = Arc::new(JobCounters::default());
    let job = counted_job("late", &counters);
    let published = publish_spawn(&arena, &platform, job).expect("发布应成功");
    // engine 超时写者路径：CANCELLED + 自回收（闭包 drop 由该路径承担）。
    let cancel = engine_cancel(&arena, &platform, published);
    assert!(matches!(cancel, CancelOutcome::Cancelled));
    assert_eq!(
        platform.wake_count(arena.slot_word_ptr(0)),
        2,
        "I6：H4与显式CANCEL各唤醒一次"
    );
    assert_eq!(counters.drop.load(Ordering::SeqCst), 1);
    // 同槽 g+1 发布（LIFO 必得同槽）。
    let job1 = counted_job("g1", &counters);
    let published1 = publish_spawn(&arena, &platform, job1).expect("复用发布应成功");
    assert_eq!(published1.slot, 0);
    assert_eq!(published1.gen, 1);
    // canary：g+1 载荷/应答字注入哨值。
    arena
        .answer_word(0, AnswerField::WorkerId)
        .store(0xBEEF, Ordering::SeqCst);
    let outcome = test_supervisor::answer_ok(&arena, &platform, 0, 0, 9);
    assert!(matches!(
        outcome,
        test_supervisor::AnswerOutcome::DroppedLate
    ));
    assert_eq!(
        arena
            .answer_word(0, AnswerField::WorkerId)
            .load(Ordering::SeqCst),
        0xBEEF,
        "迟到应答零载荷写：canary 必须原样"
    );
    assert_eq!(
        arena.slot_word(0).load(Ordering::SeqCst),
        encode_w(1, SlotState::Req, GEN_BITS_DEFAULT),
        "槽字不变（仍 (g+1,REQ)）"
    );
    assert_eq!(
        *platform.terminate_log.lock().unwrap(),
        vec![9],
        "迟到构造子 terminate 卫生"
    );
    // g+1 正常走完。
    let outcome1 = test_supervisor::answer_ok(&arena, &platform, 0, 1, 10);
    assert!(matches!(outcome1, test_supervisor::AnswerOutcome::Answered));
    let ran = trampoline_run(&arena, &platform, 0, 1, 10);
    assert!(matches!(ran, TrampolineExit::Ran { .. }));
    let (): () = join_typed(&arena, &platform, 0, 1).expect("g+1 join");
    assert_eq!(counters.run.load(Ordering::SeqCst), 1);
}

/// 超时撞 ANSWERING（§13.1 修订）：取消 CAS 失败 ⇒ 不再尝试、计诊断一次、
/// 转等终态——迟到成功也是成功（§9.2.1 OK 行）。
#[test]
fn 超时撞answering转等终态() {
    let arena = Arc::new(SlotArena::alloc(2, GEN_BITS_DEFAULT));
    let platform = Arc::new(TestPlatform::default());
    let counters = Arc::new(JobCounters::default());
    let job = counted_job("answ", &counters);
    let (a2, p2) = (Arc::clone(&arena), Arc::clone(&platform));
    let returned = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let r_flag = Arc::clone(&returned);
    let requester = thread::spawn(move || {
        let r = request_spawn::<(), _>(&a2, &p2, job, 10_000);
        r_flag.store(true, Ordering::SeqCst);
        r
    });
    platform.wait_notify_spawn(0);
    let (a3, p3) = (Arc::clone(&arena), Arc::clone(&platform));
    let child = thread::spawn(move || trampoline_run(&a3, &p3, 0, 0, 5));
    // 应答线程：冻于 C1（A1 已成功 ⇒ W=(0,ANSWERING)，A2/A3 未落）。
    platform.seams.c1.arm();
    let (a4, p4) = (Arc::clone(&arena), Arc::clone(&platform));
    let answerer = thread::spawn(move || test_supervisor::answer_ok(&a4, &p4, 0, 0, 5));
    platform.seams.c1.wait_hit();
    // 虚拟截止到点：请求者取消 CAS 必败（ANSWERING）。
    platform.fire_deadline(arena.slot_word_ptr(0));
    std::thread::sleep(std::time::Duration::from_millis(30));
    assert!(
        !returned.load(Ordering::SeqCst),
        "撞 ANSWERING 不得以超时失败返回"
    );
    assert_eq!(
        arena.slot_word(0).load(Ordering::SeqCst),
        encode_w(0, SlotState::Answering, GEN_BITS_DEFAULT),
        "取消不得打断应答（A4① 取消窗关闭）"
    );
    assert_eq!(
        arena.timeout_hit_answering_word().load(Ordering::SeqCst),
        1,
        "诊断计数恰一次"
    );
    // 放行：A2/A3 → OK → 迟到成功。
    platform.seams.c1.release_once();
    let outcome = answerer.join().expect("应答线程不应 panic");
    assert!(matches!(outcome, test_supervisor::AnswerOutcome::Answered));
    let handle = requester
        .join()
        .expect("请求者不应 panic")
        .expect("迟到成功也是成功");
    let _child_out = child.join().expect("trampoline 不应 panic");
    let (): () = join_typed(&arena, &platform, handle.join_index, handle.gen).expect("join");
    assert_eq!(counters.run.load(Ordering::SeqCst), 1);
    assert_eq!(arena.tombstone_word().load(Ordering::SeqCst), 0);
}

/// D5 两阶段发布：暂停载荷写权持有者，join 必须等最终 DEAD，不能消费旧 code。
#[test]
fn d5写权暂停不提前消费且重复通知零写() {
    let arena = Arc::new(SlotArena::alloc(1, GEN_BITS_DEFAULT));
    let platform = Arc::new(TestPlatform::default());
    super::with_h1(&arena, |a| {
        let alloc = a.h2_take(&NullPlatform).unwrap();
        a.h3_h4_publish(&platform, alloc, 0, 0);
    });
    platform.seams.d.arm();
    let (a, p) = (arena.clone(), platform.clone());
    let writer = thread::spawn(move || {
        test_supervisor::write_dead(&a, &p, 0, 0, test_supervisor::DeadKind::HardTrap, 0xD5)
    });
    platform.seams.d.wait_hit();
    assert_eq!(
        arena.join_word(0).load(Ordering::SeqCst),
        encode_join(0, JoinState::DeadWriting, GEN_BITS_DEFAULT)
    );
    assert_eq!(
        test_supervisor::write_dead(
            &arena,
            &platform,
            0,
            0,
            test_supervisor::DeadKind::Panic,
            0xBAD
        ),
        test_supervisor::DeadOutcome::LostToDead
    );
    assert_eq!(arena.join_dead_code(0), 0);
    let (a, p) = (arena.clone(), platform.clone());
    let joiner = thread::spawn(move || join_typed::<(), _>(&a, &p, 0, 0));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !platform.wait_log.lock().unwrap().contains(&encode_join(
        0,
        JoinState::DeadWriting,
        GEN_BITS_DEFAULT,
    )) {
        assert!(std::time::Instant::now() < deadline, "join 未进入写权等待");
        thread::yield_now();
    }
    assert!(!joiner.is_finished(), "载荷未发布不得返回旧 code");
    platform.seams.d.release_once();
    assert_eq!(
        writer.join().unwrap(),
        test_supervisor::DeadOutcome::Written
    );
    assert!(joiner
        .join()
        .unwrap()
        .unwrap_err()
        .to_string()
        .contains("0xD5"));
    assert_eq!(
        platform.wake_count(arena.join_word_ptr(0)),
        2,
        "I6：DEAD发布与CONSUMED各唤醒一次"
    );
}

/// 误报 D5 先于 W2 时结果仍归 trampoline；退出不能 panic 或泄漏结果。
#[test]
fn trampoline输给死亡信号结果恰一释放() {
    for writing in [false, true] {
        let arena = Arc::new(SlotArena::alloc(1, GEN_BITS_DEFAULT));
        let platform = Arc::new(TestPlatform::default());
        let drops = Arc::new(JobCounters::default());
        let result = DropGuard(drops.clone());
        let a = arena.clone();
        let job = SpawnJob::from_closure(
            move || {
                if writing {
                    a.join_word(0).store(
                        encode_join(0, JoinState::DeadWriting, GEN_BITS_DEFAULT),
                        Ordering::SeqCst,
                    );
                } else {
                    test_supervisor::write_dead(
                        &a,
                        &NullPlatform,
                        0,
                        0,
                        test_supervisor::DeadKind::Panic,
                        7,
                    );
                }
                result
            },
            "D5-result".to_owned(),
        );
        let published = publish_spawn(&arena, &platform, job).unwrap();
        test_supervisor::answer_ok(&arena, &platform, published.slot, published.gen, 1);
        assert!(matches!(
            trampoline_run(&arena, &platform, 0, 0, 1),
            TrampolineExit::Ran { .. }
        ));
        assert_eq!(drops.drop.load(Ordering::SeqCst), 1, "D5输家结果恰一次释放");
        assert_eq!(arena.tombstone_word().load(Ordering::SeqCst), 0);
    }
}

#[test]
fn 重复退出不重复回收且延迟登记去重() {
    let arena = SlotArena::alloc(1, GEN_BITS_DEFAULT);
    let platform = TestPlatform::default();
    super::with_h1(&arena, |a| {
        let alloc = a.h2_take(&NullPlatform).unwrap();
        a.h3_h4_publish(&platform, alloc, 0, 0);
    });
    test_supervisor::answer_ok(&arena, &platform, 0, 0, 1);
    arena.join_word(0).store(
        encode_join(0, JoinState::Consumed, GEN_BITS_DEFAULT),
        Ordering::SeqCst,
    );
    let mut state = test_supervisor::SupervisorState::default();
    let guard = super::H1Guard::new(&arena, &platform);
    for _ in 0..3 {
        assert_eq!(
            test_supervisor::on_worker_exit(&arena, &platform, &mut state, 0, 0),
            test_supervisor::RecoveryOutcome::Delayed
        );
    }
    assert_eq!(state.pending_recovery.len(), 1);
    assert_eq!(
        test_supervisor::recover_pending(&arena, &platform, &mut state),
        0
    );
    assert_eq!(state.pending_recovery.len(), 1);
    drop(guard);
    assert_eq!(
        test_supervisor::recover_pending(&arena, &platform, &mut state),
        1
    );
    assert_eq!(
        test_supervisor::on_worker_exit(&arena, &platform, &mut state, 0, 0),
        test_supervisor::RecoveryOutcome::GatesNotMet
    );
    assert_eq!(arena.accounts().free, 1);
}


#[test]
fn 旧回收锁前复用后须锁内重验完整代与join() {
    for pending in [false, true] {
        let arena = Arc::new(SlotArena::alloc(1, GEN_BITS_DEFAULT));
        let platform = Arc::new(TestPlatform::default());
        super::with_h1(&arena, |a| { let alloc = a.h2_take(&NullPlatform).unwrap(); a.h3_h4_publish(&platform, alloc, 0, 0); });
        test_supervisor::answer_ok(&arena, &platform, 0, 0, 1);
        arena.join_word(0).store(encode_join(0, JoinState::Consumed, GEN_BITS_DEFAULT), Ordering::SeqCst);
        let mut state = test_supervisor::SupervisorState::default();
        assert_eq!(test_supervisor::on_worker_exit(&arena, &platform, &mut state, 0, 0), test_supervisor::RecoveryOutcome::Pushed);
        if pending { state.pending_recovery.push((0, 0)); }
        platform.seams.recovery.arm();
        let (a, p) = (arena.clone(), platform.clone());
        let old = thread::spawn(move || {
            if pending { assert_eq!(test_supervisor::recover_pending(&a, &p, &mut state), 0); }
            else { assert_eq!(test_supervisor::on_worker_exit(&a, &p, &mut state, 0, 0), test_supervisor::RecoveryOutcome::GatesNotMet); }
        });
        platform.seams.recovery.wait_hit();
        let counters = Arc::new(JobCounters::default());
        let new = publish_spawn(&arena, &platform, counted_job("new", &counters)).unwrap();
        assert_eq!(new.gen, 1);
        platform.seams.recovery.release_once();
        old.join().expect("正常复用不能触发迟到回收panic");
        assert_eq!(arena.slot_word(0).load(Ordering::SeqCst), encode_w(1, SlotState::Req, GEN_BITS_DEFAULT));
        assert_eq!(arena.join_word(0).load(Ordering::SeqCst), encode_join(1, JoinState::Waiting, GEN_BITS_DEFAULT));
        assert_eq!(arena.accounts().free, 0);
        assert_eq!(arena.tombstone_word().load(Ordering::SeqCst), 0);
        engine_cancel(&arena, &platform, new);
    }
}
