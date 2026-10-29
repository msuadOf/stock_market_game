//! 确定性交错负测 N1/N1-b/N2/N3/N3-b（§12.1.5 + §13.1 改写扩充）。
//!
//! 测试缝 = SEAM-A/A2/B/C0/C1（harness 定序）；虚拟截止 = T-缩 等价。
//! 三角色 = engine 请求者线程 + 子 trampoline 线程 + harness 扮演的监督者
//! （Rust `test_supervisor` 为 N 测试驱动面；生产监督者 = TS，见
//! apps/web/src/host/facade-supervisor.ts 的镜像断言）。

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;

use super::lifecycle_tests::{counted_job, JobCounters};
use super::TestPlatform;
use crate::facade_slot::arena::AnswerField;
use crate::facade_slot::arena::SlotArena;
use crate::facade_slot::codec::*;
use crate::facade_slot::platform::NullPlatform;
use crate::facade_slot::roles::*;
use crate::facade_slot::test_supervisor;

/// N1 跨代载荷负测（Sol 反例原样复刻，§12.1.5-N1）：
/// g 代 spawn → C0 冻住应答 → T-缩 超时取消+自回收（LIFO 同槽）→
/// g+1 同槽发布+应答 OK → 放 SEAM-A。
/// 断言：(i) 旧子决策读 (g+1,OK)≠(g,OK) ⇒ 不解引用——g 代闭包 Drop 恰 1
/// （engine 计）、run 0；g+1 代 run 恰 1；(ii) 旧子 workerExit 收到；
/// (iii) 无 tombstone；尾巴：放 C0 后迟到应答 A1 必败 → 零槽写 + terminate。
#[test]
fn n1_跨代载荷负测() {
    let arena = Arc::new(SlotArena::alloc(2, GEN_BITS_DEFAULT));
    let platform = Arc::new(TestPlatform::default());
    let counters0 = Arc::new(JobCounters::default());
    let counters1 = Arc::new(JobCounters::default());

    // —— g 代 spawn（请求者线程阻塞等应答）。
    let job0 = counted_job("g0", &counters0);
    let (a0, p0) = (Arc::clone(&arena), Arc::clone(&platform));
    let requester0 = thread::spawn(move || request_spawn::<(), _>(&a0, &p0, job0, 10_000));
    let (slot, gen, name) = platform.wait_notify_spawn(0);
    assert_eq!((slot, gen), (0, 0), "首槽首代");
    assert_eq!(name, "lifecycle-g0");

    // —— 子构造（旧 trampoline 线程），C0 冻住 = harness 不应答。
    let (a1, p1) = (Arc::clone(&arena), Arc::clone(&platform));
    let old_child = thread::spawn(move || trampoline_run(&a1, &p1, 0, 0, 1));
    // SEAM-A 先 arm（旧子醒后即冻）。
    platform.seams.a.arm();

    // —— T-缩：engine 超时 → CANCELLED + 自回收。
    platform.fire_deadline(arena.slot_word_ptr(0));
    let result0 = requester0.join().expect("请求者不应 panic");
    assert!(
        matches!(result0, Err(SpawnFailure::AckTimeout)),
        "{result0:?}"
    );
    assert_eq!(
        counters0.drop.load(Ordering::SeqCst),
        1,
        "g 代闭包 Drop 恰 1（engine 计）"
    );
    // 撤销虚拟截止（粘性标志；g+1 请求者复用同槽同字，不撤则立即被误判超时）。
    platform.clear_deadline(arena.slot_word_ptr(0));

    // —— 旧子被 CANCELLED 转移唤醒 → SEAM-A 冻结并 post PAUSED。
    platform.seams.a.wait_hit();

    // —— harness：同槽 gen g+1 分配（H2-H4 全序）并走完 OK 全链。
    let job1 = counted_job("g1", &counters1);
    let (a2, p2) = (Arc::clone(&arena), Arc::clone(&platform));
    let requester1 = thread::spawn(move || request_spawn::<(), _>(&a2, &p2, job1, 10_000));
    let (slot1, gen1, _) = platform.wait_notify_spawn(1);
    assert_eq!((slot1, gen1), (0, 1), "LIFO 必得同槽、gen+1（确定性）");
    // 新子构造 + A1-A3 应答 OK。
    let (a3, p3) = (Arc::clone(&arena), Arc::clone(&platform));
    let new_child = thread::spawn(move || trampoline_run(&a3, &p3, 0, 1, 2));
    let answer = test_supervisor::answer_ok(&arena, &platform, 0, 1, 2);
    assert!(matches!(answer, test_supervisor::AnswerOutcome::Answered));
    let handle = requester1
        .join()
        .expect("请求者 1 不应 panic")
        .expect("g+1 正常成功");
    // —— 放 SEAM-A：此刻旧子（CANCELLED 醒）与新子（(1,OK) 醒）均已冻于
    // A——两枚令牌分别放行（先后不影响断言：W 恒 (1,OK)、各子决策读独立），
    // 撤防后不再新增冻结。
    platform.seams.a.release_once();
    platform.seams.a.release_once();
    let new_out = new_child.join().expect("新子不应 panic");
    assert!(matches!(new_out, TrampolineExit::Ran));
    let (): () = join_typed(&arena, &platform, handle.join_index, handle.gen).expect("g+1 join");
    assert_eq!(
        counters1.run.load(Ordering::SeqCst),
        1,
        "g+1 代闭包 run 恰 1（新子计）"
    );
    let old_out = old_child.join().expect("旧子不应 panic");
    assert!(matches!(old_out, TrampolineExit::Superseded), "{old_out:?}");
    assert_eq!(
        counters0.run.load(Ordering::SeqCst),
        0,
        "旧子不得执行 g 代闭包"
    );
    assert_eq!(
        counters0.drop.load(Ordering::SeqCst),
        1,
        "旧子不得触碰 g 代闭包（无二次 drop）"
    );

    // —— (ii) 旧子 worker 退出（workerExit 收到）。
    let (wid, wslot, wgen) = platform.wait_worker_exit();
    assert_eq!((wid, wslot, wgen), (1, 0, 0));

    // —— (iii) 无 tombstone。
    assert_eq!(arena.tombstone_word().load(Ordering::SeqCst), 0);

    // —— 尾巴：放 C0 → 迟到应答 CAS 期望 (g,REQ) 必败 → 丢弃 + terminate 幂等。
    let worker_id_canary = arena
        .answer_word(0, AnswerField::WorkerId)
        .load(Ordering::SeqCst);
    assert_eq!(worker_id_canary, 2, "g+1 应答载荷在位（canary 基线）");
    let late = test_supervisor::answer_ok(&arena, &platform, 0, 0, 1);
    assert!(matches!(late, test_supervisor::AnswerOutcome::DroppedLate));
    assert_eq!(
        arena
            .answer_word(0, AnswerField::WorkerId)
            .load(Ordering::SeqCst),
        2,
        "迟到应答零载荷写"
    );
    assert_eq!(
        *platform.terminate_log.lock().unwrap(),
        vec![1],
        "迟到构造子 terminate（幂等卫生）"
    );
}

/// N1-b 合法 OK 载荷窗口（§12.1.5）：SEAM-A2 冻住子于"读 W=(g,OK) 后、
/// 载荷读前"；回收压力 ⇒ 门 (b) 未闭合 ⇒ 槽不可分配；放行后全链+回收。
#[test]
fn n1b_合法ok载荷窗口() {
    let arena = Arc::new(SlotArena::alloc(1, GEN_BITS_DEFAULT));
    let platform = Arc::new(TestPlatform::default());
    let counters = Arc::new(JobCounters::default());
    let job = counted_job("n1b", &counters);
    let (a0, p0) = (Arc::clone(&arena), Arc::clone(&platform));
    let requester = thread::spawn(move || request_spawn::<(), _>(&a0, &p0, job, 10_000));
    platform.wait_notify_spawn(0);
    platform.seams.a2.arm();
    let (a1, p1) = (Arc::clone(&arena), Arc::clone(&platform));
    let child = thread::spawn(move || trampoline_run(&a1, &p1, 0, 0, 4));
    test_supervisor::answer_ok(&arena, &platform, 0, 0, 4);
    let handle = requester
        .join()
        .expect("请求者不应 panic")
        .expect("OK 应答");
    // 子冻于"OK 已读、载荷未读"。
    platform.seams.a2.wait_hit();
    // 回收压力：监督者 workerExit 评估 ⇒ 门 (b)（join CONSUMED）未闭合。
    let mut sup = test_supervisor::SupervisorState::default();
    match test_supervisor::on_worker_exit(&arena, &platform, &mut sup, 0, 0) {
        test_supervisor::RecoveryOutcome::GatesNotMet => {}
        other => panic!("闭包未跑、join 未 CONSUMED ⇒ 不得回收：{other:?}"),
    }
    // 强制造槽分配：cap=1 ⇒ 必得 SLOT_POOL_EXHAUSTED（槽不在空闲表）。
    match super::with_h1(&arena, |a| a.h2_take(&NullPlatform)) {
        Err(crate::facade_slot::arena::Exhausted { .. }) => {}
        Ok(_) => panic!("门 (b) 未闭合时槽不得可分配"),
    }
    // 放行：子消费 → DONE → join CONSUMED → 门链闭合 → 回收成功。
    platform.seams.a2.release_once();
    let out = child.join().expect("子不应 panic");
    assert!(matches!(out, TrampolineExit::Ran));
    let (): () = join_typed(&arena, &platform, handle.join_index, handle.gen).expect("join");
    match test_supervisor::on_worker_exit(&arena, &platform, &mut sup, 0, 0) {
        test_supervisor::RecoveryOutcome::Pushed => {}
        other => panic!("门链闭合应回收：{other:?}"),
    }
    assert_eq!(arena.accounts().free, 1);
    assert_eq!(counters.run.load(Ordering::SeqCst), 1);
}

/// N2 FAIL 载荷移交窗口（§12.1.5）：SEAM-B 冻住 engine 于"醒见 (g,FAIL) 后、
/// 错误码载荷读前"；分配压力 ⇒ 第四门未成立 ⇒ 槽不可分配；放行 ⇒ engine
/// 读得注入 canary 错误码（g 代载荷未被覆写）→ FAIL_ACKED → 自回收。
#[test]
fn n2_fail载荷移交窗口() {
    let arena = Arc::new(SlotArena::alloc(1, GEN_BITS_DEFAULT));
    let platform = Arc::new(TestPlatform::default());
    let counters = Arc::new(JobCounters::default());
    let job = counted_job("n2", &counters);
    platform.seams.b.arm();
    let (a0, p0) = (Arc::clone(&arena), Arc::clone(&platform));
    let requester = thread::spawn(move || request_spawn::<(), _>(&a0, &p0, job, 10_000));
    platform.wait_notify_spawn(0);
    // 构造失败（FAIL 无子、无条目）——errcode canary。
    test_supervisor::answer_fail(&arena, &platform, 0, 0, 0xFA);
    // engine 醒见 FAIL → SEAM-B 冻结。
    platform.seams.b.wait_hit();
    assert_eq!(
        arena.retired_fail_unacked_word().load(Ordering::SeqCst),
        1,
        "FAIL 已计 RA"
    );
    // 分配压力：第四门（ACK 在载荷读之后）未成立 ⇒ 槽不可分配。
    match super::with_h1(&arena, |a| a.h2_take(&NullPlatform)) {
        Err(crate::facade_slot::arena::Exhausted {
            retired_fail_unacked: ra,
            ..
        }) => assert_eq!(ra, 1, "RA 账随 FAIL 计入"),
        Ok(_) => panic!("FAIL 未 ACK 时槽不得可分配"),
    }
    // 放行：engine 读 canary → FAIL_ACKED → 自回收。
    platform.seams.b.release_once();
    let result = requester.join().expect("请求者不应 panic");
    match result {
        Err(SpawnFailure::WorkerFailed { errcode }) => {
            assert_eq!(errcode, 0xFA, "g 代错误码未被覆写")
        }
        other => panic!("FAIL 链：{other:?}"),
    }
    assert_eq!(arena.retired_fail_unacked_word().load(Ordering::SeqCst), 0);
    assert_eq!(arena.accounts().free, 1, "自回收后空闲表含该槽");
    assert_eq!(counters.drop.load(Ordering::SeqCst), 1);
}

/// N3 迟到应答跨代 CAS 负测（§13.1 改写）：C0 冻住应答于 A1 前 → cancel →
/// 自回收 → 同槽 gen g+1 REQ 在途 → 放 C0。断言：A1 期望 (g,REQ) 对
/// (g+1,REQ) 必败 ⇒ 应答整体丢弃、槽字不变、g+1 载荷未触碰（canary）且
/// g+1 正常走到 OK。
#[test]
fn n3_迟到应答跨代cas负测() {
    let arena = Arc::new(SlotArena::alloc(2, GEN_BITS_DEFAULT));
    let platform = Arc::new(TestPlatform::default());
    let counters0 = Arc::new(JobCounters::default());
    let counters1 = Arc::new(JobCounters::default());

    // g 代：发布后由 harness 扮演 engine 超时写者（操作级：取消+自回收）。
    let job0 = counted_job("g0", &counters0);
    let published0 = publish_spawn(&arena, &platform, job0).expect("g 发布");
    assert_eq!((published0.slot, published0.gen), (0, 0));
    let cancel = engine_cancel(&arena, &platform, published0);
    assert!(matches!(cancel, CancelOutcome::Cancelled));
    assert_eq!(counters0.drop.load(Ordering::SeqCst), 1);

    // g+1 同槽 REQ 在途（请求者线程等待）。
    let job1 = counted_job("g1", &counters1);
    let (a1, p1) = (Arc::clone(&arena), Arc::clone(&platform));
    let requester1 = thread::spawn(move || request_spawn::<(), _>(&a1, &p1, job1, 10_000));
    // g0 为操作级发布（无 H5 日志）⇒ 本条即日志第 0 条。
    let (slot, gen, _) = platform.wait_notify_spawn(0);
    assert_eq!((slot, gen), (0, 1));
    let (a2, p2) = (Arc::clone(&arena), Arc::clone(&platform));
    let child = thread::spawn(move || trampoline_run(&a2, &p2, 0, 1, 6));

    // 放 C0：迟到应答（g 代消息，gen 取自消息）——A1 对 (g+1,REQ) 必败。
    let late = test_supervisor::answer_ok(&arena, &platform, 0, 0, 9);
    assert!(matches!(late, test_supervisor::AnswerOutcome::DroppedLate));
    assert_eq!(
        arena.slot_word(0).load(Ordering::SeqCst),
        encode_w(1, SlotState::Req, GEN_BITS_DEFAULT),
        "槽字不变（仍 (g+1,REQ)）"
    );
    assert_eq!(*platform.terminate_log.lock().unwrap(), vec![9]);
    // g+1 正常走到 OK。
    let answer = test_supervisor::answer_ok(&arena, &platform, 0, 1, 6);
    assert!(matches!(answer, test_supervisor::AnswerOutcome::Answered));
    let handle = requester1
        .join()
        .expect("请求者不应 panic")
        .expect("g+1 成功");
    let out = child.join().expect("子不应 panic");
    assert!(matches!(out, TrampolineExit::Ran));
    let (): () = join_typed(&arena, &platform, handle.join_index, handle.gen).expect("join");
    assert_eq!(counters1.run.load(Ordering::SeqCst), 1);
    assert_eq!(counters0.run.load(Ordering::SeqCst), 0);
    assert_eq!(arena.tombstone_word().load(Ordering::SeqCst), 0);
}

/// N3-b 写权获取窗口（§13.1 新增）：A1 成功后冻于 C1 → (i) engine 取消 CAS
/// 必败（取消窗关闭）；(ii) 槽不在空闲表、强制造槽分配必得他槽；(iii) 放行后
/// 载荷写落 g 代槽（canary 验证写入对象正确）→ (g,OK) 链路正常、wake 无丢失。
#[test]
fn n3b_写权获取窗口() {
    let arena = Arc::new(SlotArena::alloc(2, GEN_BITS_DEFAULT));
    let platform = Arc::new(TestPlatform::default());
    let counters = Arc::new(JobCounters::default());
    let job = counted_job("n3b", &counters);
    let (a0, p0) = (Arc::clone(&arena), Arc::clone(&platform));
    let returned = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let r_flag = Arc::clone(&returned);
    let requester = thread::spawn(move || {
        let r = request_spawn::<(), _>(&a0, &p0, job, 10_000);
        r_flag.store(true, Ordering::SeqCst);
        r
    });
    platform.wait_notify_spawn(0);
    let (a1, p1) = (Arc::clone(&arena), Arc::clone(&platform));
    let child = thread::spawn(move || trampoline_run(&a1, &p1, 0, 0, 8));
    // 应答冻于 C1（A1 已成功）。
    platform.seams.c1.arm();
    let (a2, p2) = (Arc::clone(&arena), Arc::clone(&platform));
    let answerer = thread::spawn(move || test_supervisor::answer_ok(&a2, &p2, 0, 0, 8));
    platform.seams.c1.wait_hit();
    assert_eq!(
        arena.slot_word(0).load(Ordering::SeqCst),
        encode_w(0, SlotState::Answering, GEN_BITS_DEFAULT),
        "A1 已领写权"
    );
    // (i) engine 取消 CAS 必败：注入虚拟截止（模拟 engine 超时写者）。
    platform.fire_deadline(arena.slot_word_ptr(0));
    thread::sleep(std::time::Duration::from_millis(30));
    assert!(
        !returned.load(Ordering::SeqCst),
        "取消窗关闭 ⇒ 请求者不得超时失败返回"
    );
    assert_eq!(
        arena.slot_word(0).load(Ordering::SeqCst),
        encode_w(0, SlotState::Answering, GEN_BITS_DEFAULT),
        "取消 CAS 不得落槽（A4①）"
    );
    // (ii) 槽不在空闲表、强制造槽分配必得他槽。
    let other = super::with_h1(&arena, |a| a.h2_take(&NullPlatform)).expect("应得他槽");
    assert_eq!(other.index, 1, "cap=2 ⇒ 他槽必为槽 1");
    // (iii) 放行：A2 载荷写落 g 代槽（canary=worker_id 8）→ A3 OK → 全链。
    platform.seams.c1.release_once();
    let outcome = answerer.join().expect("应答线程不应 panic");
    assert!(matches!(outcome, test_supervisor::AnswerOutcome::Answered));
    assert_eq!(
        arena
            .answer_word(0, AnswerField::WorkerId)
            .load(Ordering::SeqCst),
        8,
        "载荷写落 g 代槽（写入对象正确）"
    );
    let handle = requester
        .join()
        .expect("请求者不应 panic")
        .expect("迟到成功也是成功");
    let out = child.join().expect("子不应 panic");
    assert!(matches!(out, TrampolineExit::Ran));
    let (): () = join_typed(&arena, &platform, handle.join_index, handle.gen).expect("join");
    assert_eq!(counters.run.load(Ordering::SeqCst), 1);
    assert_eq!(arena.tombstone_word().load(Ordering::SeqCst), 0);
}
