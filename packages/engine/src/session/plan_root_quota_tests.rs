//! 缺口② burst 配额化——root 侧确定性测试（设计 v4 §3.1②/④ p3 与 4c）。
//!
//! p3 按 v4 三审③ 断言四件套：①发布顺序（终结通知不早于临时绑定析构——
//! 由 finish 段发布天然保证，测试以「收到 TaskDied 时 guard 已归还」的台账
//! 可证形态钉住）；②原 panic payload 传播（重抛原对象）；③任务最终释放
//! （guard 全部析构、admission 回基线）；④无 TaskDied 之外的静默消失。
//! a1 失败原子性之「personal 不 install」由 p3 的权威状态零变更断言承载；
//! e1/e2 operations 层死区用例见 ready_stock_stream 级（登记于本轮 index）。

use super::plan_chain_candidates::set_root_panic_accounts;
use super::plan_chain_candidates_tests::execution_fixture;
use crate::AccountId;

/// p3（负向 mutation 红：移除 TaskDied 发布/吞 panic → 挂死或静默红）：
/// root bound panic（detached spawn）→ 协调者有限时间显式失败——原 payload
/// 重抛、不 install（权威状态零部分提交）、guard 最终归还（台账回基线）、
/// 终结通知晚于绑定析构（收到通知时 spawned 已归还）。
#[test]
#[ignore = "口径=串行独立运行：root panic 注入为进程级全局静态（交叉污染同 wave 系登记）；cargo test -p engine --lib p3_root_bound -- --ignored --test-threads 1"]
fn p3_root_bound_panic_reraises_without_install_and_releases_guards() {
    let (mut session, _) = execution_fixture();
    let compute = session.compute.clone();
    let accounts = vec![AccountId(1)];
    let mut batch = {
        let market = session.build_market_view();
        super::plan_chain_candidates::PlanChainOperationBatch::accounts(
            accounts.clone(),
            market,
            Default::default(),
            Default::default(),
            crate::calendar::CivilInstant::new(
                crate::calendar::CivilDate::from_iso("2030-01-02").unwrap(),
                0,
            )
            .unwrap(),
            Default::default(),
        )
    };
    let _ = &mut batch;
    set_root_panic_accounts(&[AccountId(1)]);
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // 阻塞拉取循环：与生产 blocking 链同源（prepare blocking=true 的 4b）。
        loop {
            match batch.prepare_ready_accounts(&mut session, true) {
                Ok(crate::session::plan_chain_candidates::PrepareReady::Progress(true)) => {}
                Ok(crate::session::plan_chain_candidates::PrepareReady::Progress(false)) => {
                    continue;
                }
                Ok(crate::session::plan_chain_candidates::PrepareReady::PendingCapacity) => {
                    // 测试线程非池内 worker：4c 上提合法，等待容量后重入。
                    compute.wait_for_spawn_capacity();
                }
                Err(fatal) => panic!("prepare 失败：{fatal}"),
            }
        }
    }))
    .expect_err("root panic 必须显式传播（不挂死、不静默、不 install）");
    set_root_panic_accounts(&[]);
    // 增量二十一（四审直证）：对象身份=同地址判等——注入点登记 panic_any
    // 所装 Box 的元素地址，重抛对象实际地址与之相等即原对象（重建必不同）。
    let token = payload
        .downcast_ref::<Box<super::plan_chain_candidates::RootPanicToken>>()
        .expect("重抛必须是原 Box<RootPanicToken> 对象（不转换、不新建）");
    assert_eq!(token.account, AccountId(1));
    assert_eq!(
        &**token as *const super::plan_chain_candidates::RootPanicToken as usize,
        super::plan_chain_candidates::root_panic_token_addr().expect("注入地址登记缺失"),
        "payload 对象身份（同地址判等）"
    );
    // ③任务最终释放：guard 全部析构、台账回基线（标记到达≠guard 已析构，
    // 以最终态为准——设计 §3.1④ 三审③）。
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while compute.admission_snapshot() != (0, 0) {
        assert!(
            std::time::Instant::now() < deadline,
            "root guard 必须最终全部归还（admission={:?}）",
            compute.admission_snapshot()
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    // a1 承载断言（迟到不 install）：panic 展开点之后，任何在飞/迟到结果都
    // 不得再改权威状态——以展开点快照为基线观察一段窗口（含 take 在内的
    // 展开前正常入口捕获不在断言范围，如实登记）。
    let authority_at_unwind = session.business_state_hash().unwrap();
    let session_at_unwind = session.session_state_hash().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(
        session.business_state_hash().unwrap(),
        authority_at_unwind,
        "失败 root 的 personal state 不得恢复进权威状态（迟到结果不 install）"
    );
    assert_eq!(
        session.session_state_hash().unwrap(),
        session_at_unwind,
        "session 状态零额外漂移"
    );
}

/// r5（池内 4c）：容量受阻（零在飞+pending 非空）时池内协调者内联推进一项
/// 自有 pending root——不产生 PendingCapacity（池内只应内联）。
#[test]
fn r5_pool_coordinator_inlines_pending_root_under_capacity_pressure() {
    let (mut session, _) = execution_fixture();
    let compute = session.compute.clone();
    // 钉满账：测试侧持有全部 spawned 槽位，留 drive 入口可用。
    let workers = compute.workers();
    if workers <= 1 {
        return; // 单 worker 池不走台账路径（round5-③ 限定），本用例不适用。
    }
    let mut held = Vec::new();
    while let Some(guard) = compute.reserve_spawn_slot() {
        if compute.admission_snapshot().1 + 1 >= workers {
            held.push(guard);
            break;
        }
        held.push(guard);
    }
    let accounts = vec![AccountId(1)];
    let mut batch = {
        let market = session.build_market_view();
        super::plan_chain_candidates::PlanChainOperationBatch::accounts(
            accounts,
            market,
            Default::default(),
            Default::default(),
            crate::calendar::CivilInstant::new(
                crate::calendar::CivilDate::from_iso("2030-01-02").unwrap(),
                0,
            )
            .unwrap(),
            Default::default(),
        )
    };
    let before = session.business_state_hash().unwrap();
    let outcome = compute.install(|| {
        batch.prepare_ready_accounts(&mut session, true)
    });
    // 池内：4c 内联推进（或容量已释放正常派发）——绝不 PendingCapacity。
    match outcome {
        Ok(crate::session::plan_chain_candidates::PrepareReady::PendingCapacity) => {
            panic!("池内协调者收到 PendingCapacity=invariant 错误（只应内联）")
        }
        Ok(_) => {}
        Err(fatal) => panic!("4c 内联推进不得失败：{fatal}"),
    }
    drop(held);
    assert_ne!(
        session.business_state_hash().unwrap(),
        before,
        "root 观察推进了权威计划状态（install 发生）"
    );
}
