use super::*;
use crate::session::pipeline::local_admission::{admit_ready_batch, AccountReceipt};
use crate::session::pipeline::IntentCandidateKey;
use crate::{AccountId, Intent, Money, Side, StockId};

/// T5（Sol 必修 1·root 闭环）完成见证：capture_roots 返回即「root 结果+
/// personal patch 完整归并后」——root 在 capture 期同步闭环（单 worker 池
/// 上以 4b 协作等待自足完成），首批拉取**同步**取得全部 root 产物，
/// 无执行期唤醒/等待（旧 PlanRoot 通知断言随生产形状删除退役；空/非空
/// root 两态由 observed_accounts 有/无计划账户覆盖——单 worker 协作执行
/// 即活性复核）。
#[test]
fn capture_roots_closes_all_roots_before_stream_starts_with_one_or_several_workers() {
    for workers in [1, 3] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .unwrap();
        // 空根形态：无计划账户（quote_setup 无 plan）——闭环平凡完成。
        let mut session =
            GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
        pool.install(|| {
            let sources = ReadyIngressSources {
                initial: Vec::new(),
                observed_accounts: vec![AccountId(1)],
            };
            let mut ingress = sources.capture_roots(&mut session, None).unwrap();
            // 同步取尽（无等待路径）：全部产物已并入链。
            let _ = ingress.first_ready_batch(&mut session).unwrap();
            let (mut chain, _notifications, _) = ingress.into_parts();
            PlanContinuationDomain::scoped(&mut session, |domain| chain.next_ready_batch(domain))
                .unwrap();
        });
        // 非空根形态：为账户 1 现建计划（execution_fixture 同构）后重跑——
        // capture 期真实计算+归并，返回后同步可取。
        let fixture_pool = crate::compute::ComputeContext::from_pool(std::sync::Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(workers)
                .build()
                .unwrap(),
        ));
        let (mut session, request) =
            crate::session::plan_chain_candidates_tests::execution_fixture_with_compute(
                fixture_pool,
            );
        // 非空根批（对齐 dead-zone 装配）：accounts root（账户 2）+ 种子执行
        // 请求（账户 1 计划）——drain 期真实 root 计算+归并。
        let market = session.build_market_view();
        let mut roots = crate::session::plan_chain_candidates::PlanChainOperationBatch::accounts(
            vec![AccountId(2)],
            market,
            Default::default(),
            Default::default(),
            crate::calendar::CivilInstant::new(
                crate::calendar::CivilDate::from_iso("2030-01-02").unwrap(),
                0,
            )
            .unwrap(),
            Default::default(),
        );
        roots.push_execution(request);
        pool.install(|| {
            let sources = ReadyIngressSources {
                initial: Vec::new(),
                observed_accounts: vec![AccountId(2)],
            };
            let mut ingress = sources.capture_roots(&mut session, Some(roots)).unwrap();
            let mut ready = ingress.first_ready_batch(&mut session).unwrap();
            let (mut chain, _notifications, _) = ingress.into_parts();
            // 同步取尽（闭环后无等待路径——纯队列消费）。
            loop {
                let batch = PlanContinuationDomain::scoped(&mut session, |domain| {
                    chain.next_ready_batch(domain)
                })
                .unwrap();
                if batch.is_empty() {
                    break;
                }
                ready.extend(batch);
            }
            // 非空根：闭环后全部候选同步就绪（至少一个计划链候选）。
            assert!(
                ready.iter().any(|candidate| matches!(
                    candidate.key(),
                    IntentCandidateKey::PlanChain { .. }
                )),
                "非空 root 闭环后计划链候选同步可取（workers={workers}）"
            );
        });
    }
}

#[test]
fn account_receipts_follow_request_time_and_plan_readiness_not_key_order() {
    let owner = AccountId(1);
    let place = |stock: StockId| Intent::PlaceLimit {
        code: stock,
        side: Side::Buy,
        price: crate::LimitPrice::Fixed(Money::from_cents(100)),
        qty: 100,
    };
    let ready = vec![
        IntentCandidate::new(IntentCandidateKey::plan_chain(99), owner, place(StockId(1))),
        IntentCandidate::new(IntentCandidateKey::npc(owner, 0), owner, place(StockId(0)))
            .with_ingress_order(0, 0),
        IntentCandidate::new(IntentCandidateKey::plan_chain(2), owner, place(StockId(2))),
    ];
    let mut receipts = AccountReceipts::default();
    let admitted = admit_ready_batch(ready, &mut receipts).unwrap();
    assert_eq!(
        admitted
            .iter()
            .map(|candidate| candidate.key().clone())
            .collect::<Vec<_>>(),
        vec![
            IntentCandidateKey::npc(owner, 0),
            IntentCandidateKey::plan_chain(99),
            IntentCandidateKey::plan_chain(2),
        ]
    );
    let following = receipts
        .observe(&IntentCandidate::new(
            IntentCandidateKey::plan_chain(1),
            owner,
            place(StockId(3)),
        ))
        .unwrap();
    assert_eq!(following, AccountReceipt::ReadyThisTick(2));
}

#[test]
fn same_stock_fifo_uses_receipt_time_across_player_and_npc_queues() {
    for workers in [1, 3] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .unwrap();
        let admitted = pool.install(|| {
            let mut session =
                GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42)
                    .unwrap();
            let code = session
                .stock_id(&session.state.setup.stocks[0].code.0)
                .unwrap();
            session
                .enqueue_player_intent(
                    AccountId(0),
                    Intent::PlaceLimit {
                        code: code.clone(),
                        side: Side::Buy,
                        price: crate::LimitPrice::Fixed(Money::from_cents(990)),
                        qty: 100,
                    },
                )
                .unwrap();
            let received_npc = session
                .state
                .ingress_receipt_cursors
                .receive(
                    AccountId(1),
                    Intent::PlaceLimit {
                        code: code.clone(),
                        side: Side::Buy,
                        price: crate::LimitPrice::Fixed(Money::from_cents(990)),
                        qty: 100,
                    },
                )
                .unwrap();
            // T3 形状适配（登记 t3reg 行 3 同族）：NPC 批改为 tick 内
            // NpcDecisionHandover 移交槽位（生产=决策阶段入口）。
            let output = super::super::npc_tick_preparation::NpcDecisionHandover::new(
                session.state.tick,
                vec![AccountId(1)],
                vec![received_npc],
                Vec::new(),
            );
            let sources = ReadyIngress::capture_sources(&mut session, &mut Some(output)).unwrap();
            admit_ready_batch(sources.initial, &mut AccountReceipts::default()).unwrap()
        });
        assert!(matches!(
            admitted.as_slice(),
            [first, second]
                if matches!(first.key(), IntentCandidateKey::Player { .. })
                    && matches!(second.key(), IntentCandidateKey::Npc { .. })
        ));
    }
}

#[test]
fn same_account_cash_lane_uses_cross_source_receive_time() {
    // known-6 #6 处置（夹具缺陷，出生即红）：本用例前提=同账户两只**不同**
    // 股票共享现金 lane（跨来源 receive 时点排序），原夹具误用单股
    // quote_setup(0)（stocks[1] 越界 panic，97e4953a 引入时即红）——改用
    // 双股 two_stock_quote_setup；断言面（玩家先/NPC 后按 receive 时点）不变。
    let mut session = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        45,
    )
    .unwrap();
    let first_stock = session
        .stock_id(&session.state.setup.stocks[0].code.0)
        .unwrap();
    let second_stock = session
        .stock_id(&session.state.setup.stocks[1].code.0)
        .unwrap();
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: first_stock,
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(990)),
                qty: 100,
            },
        )
        .unwrap();
    let npc = session
        .state
        .ingress_receipt_cursors
        .receive(
            AccountId(0),
            Intent::PlaceLimit {
                code: second_stock,
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(990)),
                qty: 100,
            },
        )
        .unwrap();
    let output = super::super::npc_tick_preparation::NpcDecisionHandover::new(
        session.state.tick,
        vec![AccountId(0)],
        vec![npc],
        Vec::new(),
    );
    let sources = ReadyIngress::capture_sources(&mut session, &mut Some(output)).unwrap();
    let admitted = admit_ready_batch(sources.initial, &mut AccountReceipts::default()).unwrap();

    assert!(matches!(
        admitted.as_slice(),
        [first, second]
            if matches!(first.key(), IntentCandidateKey::Player { .. })
                && matches!(second.key(), IntentCandidateKey::Npc { .. })
    ));
}

#[test]
fn enqueue_after_tick_capture_is_deferred_to_the_next_capture() {
    let mut session =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let code = session
        .stock_id(&session.state.setup.stocks[0].code.0)
        .unwrap();
    let intent = || Intent::PlaceLimit {
        code: code.clone(),
        side: Side::Buy,
        price: crate::LimitPrice::Fixed(Money::from_cents(990)),
        qty: 100,
    };

    session
        .enqueue_player_intent(AccountId(0), intent())
        .unwrap();
    // T3 形状适配（登记 t3reg 行 4）：空 NPC 决策产出（无 due）+玩家 capture。
    let empty_output =
        super::super::npc_tick_preparation::NpcDecisionHandover::empty(session.state.tick);
    let frozen = ReadyIngress::capture_sources(&mut session, &mut Some(empty_output)).unwrap();
    assert!(matches!(
        frozen.initial.as_slice(),
        [candidate] if matches!(candidate.key(), IntentCandidateKey::Player { .. })
    ));
    session
        .enqueue_player_intent(AccountId(0), intent())
        .unwrap();
    assert_eq!(session.state.pending_player.len(), 1);
}

#[test]
fn failed_tick_keeps_external_queue_for_the_next_committed_attempt() {
    let mut session =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 43).unwrap();
    let code = session
        .stock_id(&session.state.setup.stocks[0].code.0)
        .unwrap();
    let intent = Intent::PlaceLimit {
        code,
        side: Side::Buy,
        price: crate::LimitPrice::Fixed(Money::from_cents(990)),
        qty: 100,
    };
    session
        .enqueue_player_intent(AccountId(0), intent.clone())
        .unwrap();
    let before = session.business_state_hash().unwrap();
    let fatal = crate::session::StepFatal::InvariantViolation {
        description: "受理后、提交前注入失败".to_owned(),
        location: "ready_ingress_tests".to_owned(),
    };
    session.inject_post_shadow_failure(fatal.clone());

    assert_eq!(session.step().unwrap_err(), fatal);

    assert_eq!(session.state.pending_player.len(), 1);
    let queued = &session.state.pending_player[0];
    assert_eq!(queued.owner, AccountId(0));
    assert_eq!(
        serde_json::to_value(&queued.intent).unwrap(),
        serde_json::to_value(intent).unwrap()
    );
    assert_eq!(session.business_state_hash().unwrap(), before);
}

/// Sol T5 终裁·完成边界见证：同一 session.compute 上，capture_roots
/// **返回当场**断言——①root 源排空（roots_source_drained）；②personal
/// patch 完整归并（root_personal_installs 计数在返回时点已达成）；③真实
/// root 产物来源（非空形态下 Prepared 交付计数>0 且产物可同步取）。
/// 覆盖 空/非空 × 单/多 worker + 容量受阻形态（3 账户/2 worker 池外排空，
/// 仅多 worker 池承载——单 worker 池容量台账恒准入，如实登记）。
/// 红证（增量八十六链）：删除 drain 调用后本边界断言当场失败——后续
/// blocking 拉取不能补救（断言先于任何拉取执行）。
#[test]
fn capture_boundary_asserts_drain_and_personal_merge_at_return() {
    for workers in [1, 3] {
        // 空根形态：无计划账户——源排空平凡成立、无 personal install。
        // Sol 终裁二轮·2：构造即绑专用池（GameSession::new_with_compute）
        // ——session.compute 与排空同资源域（pool.install 不换绑定的盲区）。
        let pool = crate::compute::ComputeContext::from_pool(std::sync::Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(workers)
                .build()
                .unwrap(),
        ));
        let mut session = GameSession::new_with_compute(
            crate::session::npc_working_quote_tests::quote_setup(0),
            42,
            pool,
        )
        .unwrap();
        {
            let sources = ReadyIngressSources {
                initial: Vec::new(),
                observed_accounts: vec![AccountId(1)],
            };
            let ingress = sources.capture_roots(&mut session, None).unwrap();
            let (chain, _, _) = ingress.into_parts();
            assert!(
                chain.roots_source_drained(),
                "空根：capture 返回当场 root 源已排空（workers={workers}）"
            );
        }

        // 非空根形态（Sol 终裁二轮·1：**区分**真实 root 与种子）——
        // 真实 root=账户 2（本 tick 其候选产物合法为空：报价策略不提交；
        // 其真实产物=personal patch 归并，由 installs 计数差分归因）；
        // 种子=账户 1 执行请求（已知身份，首批完整集合**精确**核对）。
        // 实测：无 allocation 授予时真实 root 自动产单不发生（如实登记，
        // 八十七「产物来源=计划链候选在场」表述过强——勘误见台账）。
        let fixture_pool = crate::compute::ComputeContext::from_pool(std::sync::Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(workers)
                .build()
                .unwrap(),
        ));
        let (mut session, request) =
            crate::session::plan_chain_candidates_tests::execution_fixture_with_compute(
                fixture_pool,
            );
        let market = session.build_market_view();
        let mut roots = crate::session::plan_chain_candidates::PlanChainOperationBatch::accounts(
            vec![AccountId(2)],
            market,
            Default::default(),
            Default::default(),
            crate::calendar::CivilInstant::new(
                crate::calendar::CivilDate::from_iso("2030-01-02").unwrap(),
                0,
            )
            .unwrap(),
            Default::default(),
        );
        roots.push_execution(request);
        let installs_before = session
            .compute
            .t1_probes
            .root_personal_installs
            .load(std::sync::atomic::Ordering::SeqCst);
        let ingress = sources_2()
            .capture_roots(&mut session, Some(roots))
            .unwrap();
        // —— 完成边界：以下全部断言先于任何后续拉取 ——
        let installs_at_return = session
            .compute
            .t1_probes
            .root_personal_installs
            .load(std::sync::atomic::Ordering::SeqCst);
        assert!(
            installs_at_return > installs_before,
            "非空根：personal patch 在 capture 返回当场已归并（installs {installs_before}->{installs_at_return}，workers={workers}）"
        );
        let (chain, _, _) = ingress.into_parts();
        assert!(
            chain.roots_source_drained(),
            "非空根：capture 返回当场 root 源已排空（workers={workers}）"
        );
        // Luna 快检（八十六·1）：真实 Prepared 产物**同步取得**断言——闭环后
        // 无等待路径，首批拉取即取得全部 root 产物（计划链键在场=交付证据）。
        let mut ingress_recheck = ReadyIngress {
            initial: Vec::new(),
            chain,
            notifications: crate::session::pipeline::stock_stream::StockStreamNotifications::new(),
            receipts: Default::default(),
        };
        let mut ready = ingress_recheck.first_ready_batch(&mut session).unwrap();
        let (mut chain, _, _) = ingress_recheck.into_parts();
        loop {
            let batch = PlanContinuationDomain::scoped(&mut session, |domain| {
                chain.next_ready_batch(domain)
            })
            .unwrap();
            if batch.is_empty() {
                break;
            }
            ready.extend(batch);
        }
        // Sol 终裁二轮·1：首批**完整集合精确核对**（非 any 模糊匹配）——
        // 恰一条种子候选（owner=账户 1+计划链键）；真实 root（账户 2）候选
        // 产物为空属合法形态，其真实产物=personal 归并（上方 installs 差分）。
        assert_eq!(
            ready.len(),
            1,
            "首批恰一条候选（种子身份；实际 {:?}，workers={workers}）",
            ready
                .iter()
                .map(|c| (c.owner(), c.key().clone()))
                .collect::<Vec<_>>()
        );
        assert_eq!(ready[0].owner(), AccountId(1), "种子候选 owner=账户 1");
        assert!(
            matches!(
                ready[0].key(),
                crate::session::pipeline::IntentCandidateKey::PlanChain { .. }
            ),
            "种子候选=计划链键（实际 {:?}）",
            ready[0].key()
        );

        // 容量受阻形态（仅 workers>=2 承载——单 worker 池容量台账恒准入）：
        // 3 账户 root / 2 worker 专用池**外**排空——测试侧先钉满全部 spawn
        // 槽、后台线程延迟释放；排空必须先经 PendingCapacity 等待（探针
        // 计数增量）再被释放推进，并于返回当场闭环。
        if workers >= 2 {
            let tight = crate::compute::ComputeContext::with_workers(2).unwrap();
            let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
            setup.npcs.inst_count = 3;
            let mut session = GameSession::new(setup, 42).unwrap();
            session.rebind_compute_for_verification(tight.clone());
            let mut held = Vec::new();
            while let Some(guard) = session.compute.reserve_spawn_slot() {
                held.push(guard);
            }
            // 增量九十三·flake 加固（实测 g93-runner-run2.log：满套件负载下
            // 协调线程可在 50ms 窗口后才到达首个 reserve，释放先至⇒形态未
            // 进入⇒计数断言假红）：固定睡眠改为**会合式释放**——等
            // root_drain_capacity_waits 首次真实增加后再释放；10s 兜底（形态
            // 未进入时仍释放，让下方计数断言以明确信息失败，不悬挂）。
            let releaser_compute = tight.clone();
            let releaser = std::thread::spawn(move || {
                let started = std::time::Instant::now();
                while releaser_compute
                    .t1_probes
                    .root_drain_capacity_waits
                    .load(std::sync::atomic::Ordering::SeqCst)
                    == 0
                    && started.elapsed() < std::time::Duration::from_secs(10)
                {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                drop(held);
            });
            let sources = ReadyIngressSources {
                initial: Vec::new(),
                observed_accounts: vec![AccountId(1), AccountId(2), AccountId(3)],
            };
            let waits_before = session
                .compute
                .t1_probes
                .root_drain_capacity_waits
                .load(std::sync::atomic::Ordering::SeqCst);
            let ingress = sources.capture_roots(&mut session, None).unwrap();
            let (chain, _, _) = ingress.into_parts();
            assert!(
                chain.roots_source_drained(),
                "容量受阻形态：3 账户/2 worker 池外排空仍于返回当场闭环"
            );
            // Luna 快检（八十六·2）：容量探针**计数增量**断言——差≥1 证明
            // 排空确实进入 PendingCapacity 分支（非仅读后丢弃）。
            let waits_after = session
                .compute
                .t1_probes
                .root_drain_capacity_waits
                .load(std::sync::atomic::Ordering::SeqCst);
            assert!(
                waits_after > waits_before,
                "容量受阻分支真实进入（root_drain_capacity_waits {waits_before}->{waits_after}）"
            );
            releaser.join().expect("容量释放线程不得失败");
        }
    }
}

fn sources_2() -> ReadyIngressSources {
    ReadyIngressSources {
        initial: Vec::new(),
        observed_accounts: vec![AccountId(2)],
    }
}
