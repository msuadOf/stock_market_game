use crate::session::{RetailOrderDiagnosticEvent, StepFatal};
use crate::strategy::ZiNoiseStrategy;
use crate::{
    AccountId, Event, GameSession, Intent, Money, OrderId, RejectionReason, Side, StockId,
    TradingPhase,
};

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn single_pool_filled_leaves_use_actual_session_registry() {
    use crate::compute::{ComputeContext, ComputeWork};
    use std::sync::{Arc, Mutex};
    const CHILD: &str = "STOCK_SINGLE_POOL_ENGINE_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "session::pipeline::authoritative_tick_tests::single_pool_filled_leaves_use_actual_session_registry", "--nocapture"])
            .env(CHILD, "1")
            .status().unwrap();
        assert!(status.success(), "独立 engine leaf child 失败：{status}");
        return;
    }
    // 此 child 独占 global 初始化；其余 case 不共享这一 registry。
    rayon::ThreadPoolBuilder::new()
        .num_threads(3)
        .build_global()
        .unwrap();
    let foreign = rayon::ThreadPoolBuilder::new()
        .num_threads(3)
        .build()
        .unwrap();
    let same_size_other = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap();
    let mut wrong_registry = Vec::new();
    // V1：覆盖 workers 1/2/4（任务要求）；caller 覆盖普通线程、global 池、外来池与同尺寸异池。
    for workers in [1, 2, 4] {
        for caller in ["ordinary", "global3", "foreign3", "other2"] {
            for evidence in [false, true] {
                let compute = ComputeContext::with_workers(workers).unwrap();
                let samples = Arc::new(Mutex::new(Vec::new()));
                let observed = samples.clone();
                compute.set_observer(Arc::new(move |sample| {
                    observed.lock().unwrap().push(sample)
                }));
                // 同尺寸池的负控：worker 数相同不代表属于本会话。
                same_size_other.install(|| compute.record(ComputeWork::Observe));
                let control = samples.lock().unwrap().pop().unwrap();
                assert!(control.owner_worker.is_none());
                assert_eq!(control.executing_workers, 2);
                let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
                setup.npcs.retail_count = 2;
                setup.stocks[0].float_shares = 200;
                setup.float_allocation = crate::FloatAllocation::class_percentages(
                    1.0,
                    0.0,
                    0.0,
                    crate::WithinKindDistribution::EqualPercentage,
                );
                let mut session =
                    GameSession::new_with_compute(setup, 42, compute.clone()).unwrap();
                let cash_before: i128 = session
                    .state
                    .accounts
                    .values()
                    .map(|a| i128::from(a.cash().cents()))
                    .sum();
                // T3 形状适配（入口切换批登记）：原 fixture 手动注入跨 tick
                // `pending_npc` 批（2 笔 NPC 卖单@1000）——该载体已退役出生产
                // 路径；等价改接为玩家侧同价同量真实 enqueue（2 卖 1 买），持仓
                // 经 fixture restore_balances 注入玩家账户——价格/数量/成交
                // （2×100@1000、fills=4、费用与现金守恒）逐项保持。本测试断言
                // 面=计算 registry 纪律+成交守恒；跨来源受理语义由
                // shared_ingress 竞争测试族承载（ADR-0032 R4.1）。
                let player_cash = session.state.accounts[&AccountId(0)].cash();
                session
                    .fixture_account_mut(&AccountId(0))
                    .unwrap()
                    .restore_balances(
                        player_cash,
                        [(
                            crate::StockId(0),
                            crate::account::Position::from_restored_parts(200, 0, 20_000, 0),
                        )]
                        .into_iter()
                        .collect(),
                    );
                for _ in 0..2 {
                    session
                        .enqueue_player_intent(
                            AccountId(0),
                            Intent::PlaceLimit {
                                code: crate::StockId(0),
                                side: Side::Sell,
                                price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                                qty: 100,
                            },
                        )
                        .unwrap();
                }
                session
                    .enqueue_player_intent(
                        AccountId(0),
                        Intent::PlaceLimit {
                            code: crate::StockId(0),
                            side: Side::Buy,
                            price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                            qty: 200,
                        },
                    )
                    .unwrap();
                samples.lock().unwrap().clear();
                let mut step = || {
                    if evidence {
                        let (events, commit) = session.step_with_commit_evidence().unwrap();
                        let fills: Vec<_> = commit
                            .receipts()
                            .iter()
                            .filter(|r| r.kind == super::ReceiptKind::Fill)
                            .collect();
                        assert_eq!(fills.len(), 4);
                        assert!(fills
                            .iter()
                            .all(|r| r.qty_before > r.qty_after && r.value_after > r.value_before));
                        let fees: i128 = fills
                            .iter()
                            .map(|r| i128::from(r.charged.total().unwrap().cents()))
                            .sum();
                        let cash_after: i128 = session
                            .state
                            .accounts
                            .values()
                            .map(|a| i128::from(a.cash().cents()))
                            .sum();
                        assert_eq!(cash_before - cash_after, fees);
                        events
                    } else {
                        session.step().unwrap()
                    }
                };
                let events = match caller {
                    "ordinary" => step(),
                    "global3" => rayon::scope(|_| step()),
                    "foreign3" => foreign.install(step),
                    "other2" => same_size_other.install(step),
                    _ => unreachable!(),
                };
                let trades: Vec<_> = events
                    .iter()
                    .filter_map(|e| match e {
                        Event::Trade { qty, price, .. } => Some((*qty, *price)),
                        _ => None,
                    })
                    .collect();
                assert_eq!(trades.len(), 2);
                assert!(trades
                    .iter()
                    .all(|(qty, price)| *qty == 100 && *price == Money::from_cents(1_000)));
                let position =
                    &session.state.accounts[&AccountId(0)].positions()[&crate::StockId(0)];
                assert_eq!(position.qty(), 200);
                assert_eq!(position.t1_locked(), 200);
                // T3 形状适配（同批登记）：原「NPC 卖方清仓+exit_records=1」的载体
                // 断言随注入批退役——等价改接后卖方=玩家；NPC 账户持仓应保持其
                // 配售份额不动（100 股/账户）。NPC 卖出生命周期语义由
                // npc_lifecycle_projection 测试族承载。
                for id in [AccountId(1), AccountId(2)] {
                    assert_eq!(
                        session.state.accounts[&id].positions()[&crate::StockId(0)].qty(),
                        100,
                        "NPC 配售持仓不被本用例触碰"
                    );
                }
                let samples = samples.lock().unwrap();
                for work in [ComputeWork::SettlementAccount] {
                    let hits: Vec<_> = samples.iter().filter(|s| s.work == work).collect();
                    // T3 形状适配（同批登记）：原最小次数（3/2）=注入形状下
                    // 参与结算的账户扇出；等价改接后参与账户=玩家 1 个，
                    // SettlementAccount 收窄为 ≥1（真实 leaf 必须发生且经会话
                    // registry——池身份纪律断言不变）。ReceiptProjectionAccount
                    // 为 NPC 参与驱动的 leaf（本等价改接无 NPC 参与，零样本为
                    // 新形状合法值；该 leaf 的池身份纪律由 NPC 参与用例承载）。
                    assert!(!hits.is_empty(), "非空真实 leaf 必须发生");
                    eprintln!("single_pool workers={workers} caller={caller} evidence={evidence} work={work:?} samples={hits:?}");
                    for sample in hits {
                        if sample.executing_workers != workers
                            || !sample.owner_worker.is_some_and(|i| i < workers)
                        {
                            wrong_registry.push(format!(
                                "workers={workers} caller={caller} evidence={evidence} {sample:?}"
                            ));
                        }
                    }
                }
            }
        }
    }
    assert!(
        wrong_registry.is_empty(),
        "真实 leaf 逃离指定 registry：{wrong_registry:#?}"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn compute_context_production_direct_does_not_follow_callers_pool() {
    use crate::compute::{ComputeContext, ComputeWork};
    use std::sync::{Arc, Mutex};
    for workers in [1, 2] {
        let compute = ComputeContext::with_workers(workers).unwrap();
        let samples = Arc::new(Mutex::new(Vec::new()));
        let observed = samples.clone();
        compute.set_observer(Arc::new(move |sample| {
            observed.lock().unwrap().push(sample)
        }));
        let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
        setup.npcs.retail_count = 4;
        let mut second = setup.stocks[0].clone();
        second.code = crate::StockCode("600889".into());
        setup.stocks.push(second);
        let mut session = GameSession::new_with_compute(setup, 42, compute.clone()).unwrap();
        for account in 1..=4 {
            crate::session::npc_working_quote_tests::force_attention_candidate(
                &mut session,
                AccountId(account),
                0,
            );
        }
        session
            .enqueue_player_intent(
                AccountId(0),
                Intent::PlaceLimit {
                    code: crate::StockId(0),
                    side: Side::Buy,
                    price: crate::LimitPrice::Fixed(Money::from_cents(900)),
                    qty: 100,
                },
            )
            .unwrap();
        // 普通 caller 与已安装另一池的 caller 都必须进入会话指定的同一 registry。
        session.step_with_commit_evidence().unwrap();
        let foreign = rayon::ThreadPoolBuilder::new()
            .num_threads(3)
            .build()
            .unwrap();
        foreign.install(|| session.step_with_commit_evidence().unwrap());
        assert!(Arc::ptr_eq(session.compute_context(), &compute));
        let samples = samples.lock().unwrap();
        assert!(samples
            .iter()
            .any(|sample| sample.work == ComputeWork::CandidatePrepare));
        assert!(samples
            .iter()
            .any(|sample| sample.work == ComputeWork::ReadOnlyPrepare));
        for work in [
            ComputeWork::Observe,
            ComputeWork::Analyse,
            ComputeWork::Decide,
            ComputeWork::Project,
            ComputeWork::StockPrepare,
            ComputeWork::StockMatch,
            ComputeWork::StockFinish,
        ] {
            assert!(
                samples.iter().any(|sample| sample.work == work),
                "未覆盖真实任务 {work:?}"
            );
        }
        for sample in samples.iter() {
            assert_eq!(sample.executing_workers, workers, "{sample:?}");
            assert!(
                sample.owner_worker.is_some_and(|index| index < workers),
                "{sample:?}"
            );
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn compute_context_day_end_and_dense_ledger_use_session_pool() {
    use crate::compute::{ComputeContext, ComputeWork};
    use std::sync::{Arc, Mutex};
    for workers in [1, 2] {
        let compute = ComputeContext::with_workers(workers).unwrap();
        let samples = Arc::new(Mutex::new(Vec::new()));
        let observed = samples.clone();
        compute.set_observer(Arc::new(move |sample| {
            observed.lock().unwrap().push(sample)
        }));
        let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
        setup.npcs.retail_count = 2;
        setup.ticks_per_day = 6;
        // 默认起始日是休市日；日结门禁需要真实交易日（与 civil 夹具一致的 2030-01-02）。
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        let mut session = GameSession::new_with_compute(setup, 42, compute.clone()).unwrap();
        for account in 1..=2 {
            crate::session::npc_working_quote_tests::force_attention_candidate(
                &mut session,
                AccountId(account),
                0,
            );
        }
        // 密集 ledger：600 张同股限价买单使 live envelope 行数越过 512 并行阈值。
        // 价格取 990：仍在价格笼子（98%×1000=980）之内；夹具只有 StockId(0) 一只股票。
        for _ in 0..600 {
            session
                .enqueue_player_intent(
                    AccountId(0),
                    Intent::PlaceLimit {
                        code: crate::StockId(0),
                        side: Side::Buy,
                        price: crate::LimitPrice::Fixed(Money::from_cents(990)),
                        qty: 100,
                    },
                )
                .unwrap();
        }
        // 前 5 个 tick 内 600 张挂单持续在簿（≥512 行）；第 6 个 tick 是日界，
        // 日界清簿会把 live envelope 清空，因此密集断言放在日界前。
        for tick in 0..5 {
            session.step_with_commit_evidence().unwrap();
            assert!(
                session.project_live_envelopes().unwrap().len() >= 512,
                "tick {tick} 密集 ledger 前置不成立"
            );
        }
        session.step_with_commit_evidence().unwrap();
        session.end_civil_day().unwrap();
        let samples = samples.lock().unwrap();
        for work in [ComputeWork::LedgerRebase, ComputeWork::DayEndDeliver] {
            let hits = samples
                .iter()
                .filter(|sample| sample.work == work)
                .collect::<Vec<_>>();
            assert!(!hits.is_empty(), "未覆盖生产任务 {work:?}");
            for sample in hits {
                assert_eq!(sample.executing_workers, workers, "{sample:?}");
                assert!(
                    sample.owner_worker.is_some_and(|index| index < workers),
                    "{sample:?}"
                );
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn compute_context_shared_sessions_preserve_budget_and_failure_atomicity() {
    let compute = crate::ComputeContext::with_workers(2).unwrap();
    compute.install(|| {
        use rayon::prelude::*;
        (0..4).into_par_iter().for_each(|seed| {
            let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
            setup.npcs.retail_count = 2;
            let mut session = GameSession::new_with_compute(setup, seed, compute.clone()).unwrap();
            for account in 1..=2 {
                crate::session::npc_working_quote_tests::force_attention_candidate(
                    &mut session,
                    AccountId(account),
                    0,
                );
            }
            session.step_with_commit_evidence().unwrap();
            let before = session.business_state_hash().unwrap();
            session.inject_post_shadow_failure(StepFatal::InvariantViolation {
                description: "计算任务完成后注入失败".into(),
                location: "compute_context_atomicity".into(),
            });
            assert!(session.step().is_err());
            assert_eq!(session.business_state_hash().unwrap(), before);
            assert!(std::sync::Arc::ptr_eq(session.compute_context(), &compute));
        });
    });
}

fn empty_session_at(tick: u64, closing_auction_ticks: u64) -> GameSession {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(900);
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.closing_auction_ticks = closing_auction_ticks;
    let mut session = GameSession::new(setup, 42).unwrap();
    session.state.tick = tick;
    session.fixture_sync_market_modes_for_test();
    if tick > 0 {
        crate::session::pipeline::staged_decision::stage(crate::session::pipeline::NpcDecisionHandover::new(tick, Vec::new(), Vec::new(), Vec::new()));
    }
    session
}

fn reachable_session_at(tick: u64, closing_auction_ticks: u64) -> GameSession {
    if tick < 600 {
        return empty_session_at(tick, closing_auction_ticks);
    }
    let mut session = empty_session_at(599, closing_auction_ticks);
    session
        .step()
        .expect("opening completion must establish the day's candle state");
    session.state.tick = tick;
    session.fixture_sync_market_modes_for_test();
    crate::session::pipeline::staged_decision::stage(crate::session::pipeline::NpcDecisionHandover::new(tick, Vec::new(), Vec::new(), Vec::new()));
    session
}

fn small_boundary_session() -> GameSession {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(3);
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.ticks_per_day = 8;
    setup.closing_auction_ticks = 1;
    GameSession::new(setup, 42).unwrap()
}

#[test]
fn public_step_dispatches_each_market_phase_and_commits_its_evidence() {
    let cases = [
        (0, 10, TradingPhase::CallAuction),
        (600, 10, TradingPhase::PreOpen),
        (900, 10, TradingPhase::Continuous),
        (15_290, 10, TradingPhase::ClosingAuction),
    ];

    for (tick, closing_auction_ticks, expected_phase) in cases {
        let mut session = reachable_session_at(tick, closing_auction_ticks);
        assert_eq!(session.phase(), expected_phase);
        let (events, evidence) = session.step_with_commit_evidence().unwrap();

        assert_eq!(session.tick(), tick + 1);
        assert_eq!(
            evidence.next_receipt_index(),
            session.save().unwrap().runtime_state.next_receipt_base
        );
        match expected_phase {
            TradingPhase::CallAuction | TradingPhase::ClosingAuction => assert!(events.iter().any(
                |event| matches!(event, Event::AuctionTick { phase, .. } if *phase == expected_phase)
            )),
            TradingPhase::PreOpen => assert!(events.is_empty()),
            TradingPhase::Continuous => assert!(
                events
                    .iter()
                    .any(|event| matches!(event, Event::PriceTick { .. }))
            ),
        }
    }
}

#[test]
fn preparing_each_market_phase_leaves_authority_untouched_until_commit() {
    for (tick, closing_auction_ticks) in [(0, 10), (600, 10), (900, 10), (15_290, 10)] {
        let mut session = reachable_session_at(tick, closing_auction_ticks);
        let business_before = session.business_state_hash().unwrap();
        let session_before = session.session_state_hash().unwrap();
        match session.phase() {
            TradingPhase::Continuous => {
                drop(
                    super::market_tick_transaction::prepare_market_tick(&mut session)
                        .unwrap(),
                );
            }
            TradingPhase::CallAuction | TradingPhase::ClosingAuction => {
                drop(super::market_tick_transaction::prepare_market_tick(&mut session).unwrap());
            }
            TradingPhase::PreOpen => {
                drop(
                    super::market_tick_transaction::prepare_market_tick_with_evidence(
                        &mut session,
                        true,
                    )
                    .unwrap(),
                );
            }
        }
        assert_eq!(session.business_state_hash().unwrap(), business_before);
        assert_eq!(session.session_state_hash().unwrap(), session_before);
        assert_eq!(session.tick(), tick);
    }
}

#[test]
fn two_npc_requests_and_player_request_share_stock_price_time_rules() {
    let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
    setup.npcs.retail_count = 2;
    let code = setup.stocks[0].code.clone();
    let mut session = GameSession::new(setup, 8).unwrap();
    let code = session.stock_id(&code.0).unwrap();
    let first = AccountId(1);
    let second = AccountId(2);
    let player = AccountId(0);
    for account in [first, second] {
        session
            .fixture_account_mut(&account)
            .unwrap()
            .fixture_set_cash(Money::from_cents(1_000_000));
    }
    let intents = [first, second]
        .into_iter()
        .map(|account| {
            session
                .state
                .ingress_receipt_cursors
                .receive(
                    account,
                    Intent::PlaceLimit {
                        code: code.clone(),
                        side: Side::Buy,
                        price: crate::LimitPrice::Fixed(Money::from_cents(900)),
                        qty: 200,
                    },
                )
                .unwrap()
        })
        .collect();
    crate::session::pipeline::staged_decision::stage(crate::session::pipeline::NpcDecisionHandover::new(session.state.tick, vec![first, second], intents, Vec::new()));
    session
        .enqueue_player_intent(
            player,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(900)),
                qty: 100,
            },
        )
        .unwrap();

    let events = session.step().unwrap();

    let mut accepted = events
        .iter()
        .filter_map(|event| match event {
            Event::OrderAccepted {
                account,
                id,
                remaining_qty,
                ..
            } => Some((*account, *id, *remaining_qty)),
            _ => None,
        })
        .collect::<Vec<_>>();
    accepted.sort_by_key(|(account, _, _)| *account);
    assert_eq!(
        accepted
            .iter()
            .map(|(account, _, qty)| (*account, *qty))
            .collect::<Vec<_>>(),
        vec![(player, 100), (first, 200), (second, 200)]
    );
    let ids = accepted
        .iter()
        .map(|(_, id, _)| *id)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        ids,
        [OrderId(1), OrderId(2), OrderId(3)].into_iter().collect()
    );
    assert_eq!(
        session.state.markets[&code].bid_depth(),
        vec![(Money::from_cents(900), 500)]
    );
    assert_eq!(
        session.state.markets[&code].resting_orders_for(first)[0].qty,
        200
    );
    assert_eq!(
        session.state.markets[&code].resting_orders_for(second)[0].qty,
        200
    );
    assert_eq!(
        session.state.markets[&code].resting_orders_for(player)[0].qty,
        100
    );
    assert!(session.state.pending_player.is_empty());
}

#[test]
fn npc_request_uses_available_cash_when_it_enters_the_next_tick() {
    for has_exact_cash in [true, false] {
        let mut session = GameSession::new(
            crate::session::npc_working_quote_tests::retail_quote_setup(),
            8,
        )
        .unwrap();
        let account = AccountId(1);
        session
            .fixture_account_mut(&account)
            .unwrap()
            .set_strategy(Box::new(ZiNoiseStrategy::new(1.0, 900, 0.5).unwrap()));
        super::npc_state_projection_tests::use_buy_imbalance_analysis(&mut session, account);
        let observed_tick = session.tick();
        crate::session::npc_working_quote_tests::force_attention_candidate(
            &mut session,
            account,
            observed_tick + 1,
        );

        // T3 形状适配（A4 失败 4 处置+B 类登记）：D(a) 不再跨调用可见——先推
        // 进到消费 tick 前一拍，再经 cfg(test) 适配器把该 epoch 批次显式排队
        // （等价旧 tail 产物的可检视窗口），保留「已知数量订单 vs 现金额外
        // 复核（ADR-0022）」的原始断言面；生产路径不受影响。
        session.step().unwrap();
        crate::session::pipeline::queue_npc_for_next_tick(&mut session).unwrap();
        let queued = crate::session::pipeline::staged_decision::peek_any().unwrap();
        assert_eq!(queued.observed_tick(), session.tick());
        let [crate::session::ReceiptBearingIntent {
            owner,
            intent:
                Intent::PlaceLimit {
                    code,
                    side: Side::Buy,
                    price: crate::LimitPrice::Highest,
                    qty,
                },
            ..
        }] = queued.intents()
        else {
            panic!("expected one queued Highest buy, got {:?}", queued.intents());
        };
        assert_eq!(*owner, account);
        assert!(*qty > 0 && *qty % 100 == 0);
        // ADR-0022 sizes Highest buys against the daily upper limit including fees.
        // The test concerns next-tick admission, not the strategy's chosen position size.
        let required_cash = crate::session::buy_order_reservation(
            &session.state.setup.config,
            session.state.markets[code].up_stop().unwrap(),
            *qty,
            Money::ZERO,
        )
        .unwrap();
        assert!(session.state.accounts[&account].cash() >= required_cash);
        let expected_qty = *qty;
        session
            .fixture_account_mut(&account)
            .unwrap()
            .fixture_set_cash(if has_exact_cash {
                required_cash
            } else {
                required_cash.sub(Money::from_cents(1)).unwrap()
            });
        let events = session.step().unwrap();
        if has_exact_cash {
            assert!(events.iter().any(|event| matches!(
                event,
                Event::OrderAccepted { account: owner, remaining_qty, .. }
                    if *owner == account && *remaining_qty == expected_qty
            )));
        } else {
            assert!(events.iter().any(|event| matches!(
                event,
                Event::IntentRejected {
                    account: rejected,
                    reason: RejectionReason::InsufficientCash,
                    ..
                } if *rejected == account
            )));
        }
    }
}

#[test]
fn public_step_commits_phase_boundaries_once_and_leaves_a_saveable_quiet_point() {
    let boundaries = [
        (2, TradingPhase::PreOpen, 0),
        (3, TradingPhase::Continuous, 0),
        (7, TradingPhase::ClosingAuction, 0),
        (8, TradingPhase::CallAuction, 1),
    ];
    let mut session = small_boundary_session();

    for (tick_after, expected_phase, expected_day) in boundaries {
        while session.tick() < tick_after {
            // 双时钟契约驱动（known-6 #1 处置，同增量八十九 tests/session.rs）：
            // 开局 2030-01-01 为元旦休市，直接步进会使已入账日 K 钉休市日、
            // 恢复校验 fault2066 拒绝自产档；休市日/已收盘日先 end_civil_day，
            // 盘中为无操作（判据 completed_trading_sessions_expected==day+1）。
            while session.civil_clock().phase() == crate::session::CivilPhase::ClosedDay
                || session
                    .civil_clock()
                    .completed_trading_sessions_expected()
                    .unwrap()
                    != session.day() + 1
            {
                session.end_civil_day().unwrap();
            }
            session.step().unwrap();
        }

        assert_eq!(session.tick(), tick_after);
        assert_eq!(session.phase(), expected_phase);
        assert_eq!(session.day(), expected_day);

        let save = session.save().unwrap_or_else(|error| {
            panic!("tick {tick_after} 的 CommitTick 必须留下合法存档静止点：{error}")
        });
        let restored = GameSession::restore(&save).unwrap_or_else(|error| {
            panic!("tick {tick_after} committed phase must restore: {error}")
        });
        assert_eq!(
            serde_json::to_value(restored.save().unwrap()).unwrap(),
            serde_json::to_value(save).unwrap(),
        );
    }
}

#[test]
fn public_step_discards_each_phase_candidate_when_commit_preparation_fails() {
    let cases = [(0, 10), (600, 10), (900, 10), (15_290, 10)];

    for (tick, closing_auction_ticks) in cases {
        let mut session = reachable_session_at(tick, closing_auction_ticks);
        let expected_business = session.business_state_hash().unwrap();
        let fatal = StepFatal::InvariantViolation {
            description: format!("reject prepared phase {:?}", session.phase()),
            location: "authoritative_tick_tests::post_shadow".to_owned(),
        };
        session.inject_post_shadow_failure(fatal.clone());
        assert!(matches!(
            session.step_with_commit_evidence(),
            Err(error) if error == fatal
        ));

        assert_eq!(session.tick(), tick);
        assert_eq!(session.business_state_hash().unwrap(), expected_business);
        assert_eq!(session.poison_reason(), Some(&fatal));
        assert_eq!(session.save().unwrap_err(), fatal);
    }
}

#[test]
fn public_step_routes_pre_open_rejection_and_continuous_place_cancel_lifecycle() {
    let mut session = small_boundary_session();
    while session.tick() < 2 {
        session.step().unwrap();
    }
    assert_eq!(session.phase(), TradingPhase::PreOpen);
    let code = session.state.markets.keys().next().unwrap().clone();
    session.state.retail_experience.insert(
        AccountId(0),
        crate::experience::RetailExperienceState::without_equity_reference(),
    );

    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(900)),
                qty: 100,
            },
        )
        .unwrap();
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::Cancel {
                code: code.clone(),
                id: OrderId(77),
            },
        )
        .unwrap();
    let rejected = session.step().unwrap();
    assert!(rejected.iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            account: AccountId(0),
            code: rejected_code,
            reason: RejectionReason::AuctionOrderEntryClosed,
            ..
        } if rejected_code == &code
    )));
    assert!(rejected.iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            account: AccountId(0),
            code: rejected_code,
            reason: RejectionReason::AuctionOrderNotCancelable,
            ..
        } if rejected_code == &code
    )));
    assert!(session
        .last_retail_order_events()
        .iter()
        .any(|event| matches!(
            event,
            RetailOrderDiagnosticEvent::Rejected {
                account: AccountId(0),
                code: rejected_code,
                reason: RejectionReason::AuctionOrderEntryClosed,
            } if rejected_code == &code
        )));
    assert!(session
        .last_retail_order_events()
        .iter()
        .any(|event| matches!(
            event,
            RetailOrderDiagnosticEvent::Rejected {
                account: AccountId(0),
                code: rejected_code,
                reason: RejectionReason::AuctionOrderNotCancelable,
            } if rejected_code == &code
        )));
    assert_eq!(session.phase(), TradingPhase::Continuous);

    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(900)),
                qty: 100,
            },
        )
        .unwrap();
    let accepted = session.step().unwrap();
    let order_id = accepted
        .iter()
        .find_map(|event| match event {
            Event::OrderAccepted {
                account: AccountId(0),
                code: accepted_code,
                id,
                remaining_qty: 100,
                ..
            } if accepted_code == &code => Some(*id),
            _ => None,
        })
        .expect("continuous public step must publish the accepted resting order");
    assert_eq!(session.state.markets[&code].resting_order_count(), 1);

    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::Cancel {
                code: code.clone(),
                id: OrderId(order_id.0),
            },
        )
        .unwrap();
    let canceled = session.step().unwrap();
    assert!(canceled.iter().any(|event| matches!(
        event,
        Event::OrderCanceled {
            account: AccountId(0),
            code: canceled_code,
            id,
            remaining_qty: 100,
            ..
        } if canceled_code == &code && *id == order_id
    )));
    assert_eq!(session.state.markets[&code].resting_order_count(), 0);
}

#[test]
fn unified_market_auction_keeps_orders_in_the_production_book_without_matching() {
    let mut session = small_boundary_session();
    let code = *session.state.markets.keys().next().unwrap();
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 200,
            },
        )
        .unwrap();
    let (events, evidence) = session.step_with_commit_evidence().unwrap();
    assert!(!events
        .iter()
        .any(|event| matches!(event, Event::Trade { .. })));
    let market = &session.state.markets[&code];
    assert_eq!(
        market.matching_mode(),
        crate::orderbook::MatchingMode::Auction
    );
    assert_eq!(
        market.resting_order_count(),
        1,
        "生产竞价委托必须直接进入持久 OrderBook"
    );
    let order = market.resting_orders()[0].clone();
    assert_eq!(order.qty, 200);
    assert_eq!(evidence.auction_finalizers().len(), 1);
    let save = session.save().unwrap();
    let mut restored = GameSession::restore(&save).unwrap();
    assert_eq!(
        restored.state.markets[&code].matching_mode(),
        crate::orderbook::MatchingMode::Auction
    );
    assert_eq!(
        restored.state.markets[&code].resting_orders(),
        vec![order.clone()]
    );
    let (_, clearing) = restored.step_with_commit_evidence().unwrap();
    assert_eq!(
        clearing.auction_finalizers()[0].auction_completion_passes(),
        1
    );
    assert_eq!(
        restored.state.markets[&code].matching_mode(),
        crate::orderbook::MatchingMode::Continuous
    );
    assert_eq!(restored.state.markets[&code].resting_orders(), vec![order]);
}

#[test]
fn unified_market_post_clearing_failure_preserves_book_mode_cash_and_shares() {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(3);
    setup.ticks_per_day = 8;
    setup.closing_auction_ticks = 1;
    setup.stocks[0].float_shares = 1_000;
    let mut session = GameSession::new(setup, 909).unwrap();
    let code = StockId(0);
    session
        .fixture_account_mut(&AccountId(1))
        .unwrap()
        .fixture_insert_position(
            code.clone(),
            crate::Position::from_restored_parts(1_000, 0, 0, 0),
        );
    crate::session::npc_working_quote_tests::force_attention_candidate(
        &mut session,
        AccountId(1),
        100,
    );
    let mut seeded = Vec::new();
    for (owner, side, qty) in [
        (AccountId(0), Side::Buy, 200),
        (AccountId(1), Side::Sell, 100),
    ] {
        session.seed_auction_order_for_test(
            owner,
            Intent::PlaceLimit {
                code,
                side,
                price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                qty,
            },
            &mut seeded,
        );
    }
    let first = session.step().unwrap();
    assert!(!first
        .iter()
        .any(|event| matches!(event, Event::Trade { .. })));
    let before_hash = session.business_state_hash().unwrap();
    let before_orders = session.state.markets[&code].resting_orders();
    let before_snapshot = session.snapshot_inner(false, true);
    let failure = StepFatal::InvariantViolation {
        description: "清算和结算完成后拒绝 CommitTick".to_owned(),
        location: "unified_market_post_clearing_failure".to_owned(),
    };
    session.inject_post_shadow_failure(failure.clone());
    assert_eq!(session.step_with_commit_evidence().unwrap_err(), failure);
    assert_eq!(session.business_state_hash().unwrap(), before_hash);
    assert_eq!(session.tick(), 1);
    assert_eq!(
        session.state.markets[&code].matching_mode(),
        crate::orderbook::MatchingMode::Auction
    );
    assert_eq!(session.state.markets[&code].resting_orders(), before_orders);
    let after = session.snapshot_inner(false, true);
    for account in [AccountId(0), AccountId(1)] {
        assert_eq!(
            after.accounts[&account].cash,
            before_snapshot.accounts[&account].cash
        );
        assert_eq!(
            serde_json::to_value(&after.accounts[&account].positions).unwrap(),
            serde_json::to_value(&before_snapshot.accounts[&account].positions).unwrap()
        );
        assert_eq!(
            after.accounts[&account].reserved_cash,
            before_snapshot.accounts[&account].reserved_cash
        );
        assert_eq!(
            after.accounts[&account].reserved_sell_qty,
            before_snapshot.accounts[&account].reserved_sell_qty
        );
    }
}

#[test]
fn unified_market_closing_auction_uses_the_same_resting_order_lifecycle() {
    let mut session = small_boundary_session();
    while session.tick() < 3 {
        session.step().unwrap();
    }
    let code = *session.state.markets.keys().next().unwrap();
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(900)),
                qty: 100,
            },
        )
        .unwrap();
    session.step().unwrap();
    let before = session.state.markets[&code].resting_orders();
    while session.tick() < 7 {
        session.step().unwrap();
    }
    assert_eq!(session.phase(), TradingPhase::ClosingAuction);
    assert_eq!(
        session.state.markets[&code].matching_mode(),
        crate::orderbook::MatchingMode::Auction,
        "切到收盘竞价必须切换已有订单簿，不建立第二份活动委托"
    );
    assert_eq!(session.state.markets[&code].resting_orders(), before);
    let (events, evidence) = session.step_with_commit_evidence().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::AuctionCompleted { .. }))
            .count(),
        1
    );
    assert_eq!(
        evidence.auction_finalizers()[0].auction_completion_passes(),
        1
    );
    assert_eq!(evidence.auction_finalizers()[0].day_end_passes(), 1);
    assert_eq!(session.state.markets[&code].resting_order_count(), 0);
    assert_eq!(
        session.state.markets[&code].matching_mode(),
        crate::orderbook::MatchingMode::Auction
    );
    assert_eq!(session.day(), 1);
    session.hydrate_or_validate_envelope_ledger().unwrap();
}

#[test]
fn public_step_reports_zero_quantity_in_event_and_retail_diagnostic() {
    let mut session = empty_session_at(900, 10);
    let code = session.state.markets.keys().next().unwrap().clone();
    session.state.retail_experience.insert(
        AccountId(0),
        crate::experience::RetailExperienceState::without_equity_reference(),
    );
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(900)),
                qty: 0,
            },
        )
        .unwrap();

    let events = session.step().unwrap();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            account: AccountId(0),
            code: rejected_code,
            reason: RejectionReason::InvalidQuantity,
            ..
        } if rejected_code == &code
    )));
    assert!(session
        .last_retail_order_events()
        .iter()
        .any(|event| matches!(
            event,
            RetailOrderDiagnosticEvent::Rejected {
                account: AccountId(0),
                code: rejected_code,
                reason: RejectionReason::InvalidQuantity,
            } if rejected_code == &code
        )));
    assert_eq!(session.state.markets[&code].resting_order_count(), 0);
}
