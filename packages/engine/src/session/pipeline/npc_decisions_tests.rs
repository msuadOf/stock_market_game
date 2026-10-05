use super::npc_decisions::{
    npc_rng_seed, run_npc_decisions, stream_npc_decisions, stream_npc_decisions_with,
    NpcDecisionSourceError,
};
use super::{DecisionAccountInput, DecisionSnapshot, IntentCandidateKey};
use crate::behavior::BehaviorMarketObservation;
use crate::observation::{
    AccountRiskObservation, EqualWeightMarketObservation, HorizonReturn, PricePathObservation,
};
use crate::strategy::{
    MarketView, MomentumStrategy, SelfView, StockView, StrategyState, ZiNoiseStrategy,
};
use crate::{AccountId, AccountKind, Intent, Money, SplitMix64, StockCode, TradingPhase};
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::sync::Arc;

#[test]
fn shared_receiver_accepts_player_between_fast_and_slow_npc_completions() {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 2;
    let mut session = crate::GameSession::new(setup, 42).unwrap();
    let source = session.shared_ingress();
    let code = session.state.setup.stocks[0].code.clone();
    let baseline = session
        .state
        .ingress_receipt_cursors
        .next_stock_ordinal
        .get(&code)
        .copied()
        .unwrap_or(0);
    let intent = Intent::PlaceLimit {
        code: code.clone(),
        side: crate::Side::Buy,
        price: crate::LimitPrice::Fixed(Money::from_cents(990)),
        qty: 100,
    };
    let (fast_tx, fast_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release_rx = std::sync::Mutex::new(release_rx);
    let mut npc_receipts = Vec::new();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap();
    std::thread::scope(|scope| {
        let player_intent = intent.clone();
        scope.spawn(move || {
            fast_rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .expect("快 NPC receipt 必须先完成");
            source
                .enqueue_player_intent(AccountId(0), player_intent)
                .unwrap();
            release_tx.send(()).unwrap();
        });
        pool.install(|| {
            stream_npc_decisions_with(
                snapshot(vec![AccountId(1), AccountId(2)]),
                &crate::GameConfig::proposed_defaults(),
                |account| {
                    if account == AccountId(1) {
                        release_rx
                            .lock()
                            .unwrap()
                            .recv_timeout(std::time::Duration::from_secs(1))
                            .expect("慢 NPC 必须等正式入口收到 Player 后才完成");
                    }
                },
                |completed| {
                    let received = session
                        .receive_private_intent(completed.account, intent.clone())
                        .unwrap();
                    npc_receipts.push((completed.account, received.stock_ordinal));
                    if completed.account == AccountId(2) {
                        fast_tx.send(()).unwrap();
                    }
                    Ok::<_, ()>(())
                },
            )
            .unwrap();
        });
    });
    assert_eq!(
        npc_receipts,
        vec![(AccountId(2), baseline), (AccountId(1), baseline + 2)]
    );
    let players = session.save().unwrap().pending_player;
    assert_eq!(players.len(), 1);
    assert_eq!(players[0].stock_ordinal, baseline + 1);
}

#[test]
fn npc_completion_channel_receives_fast_account_first_and_drains_after_consumer_failure() {
    let snapshot = snapshot(vec![AccountId(1), AccountId(2)]);
    let received = Arc::new(std::sync::Mutex::new(Vec::new()));
    let authority_receipts = crate::session::IngressReceiptCursors::default();
    let candidate_receipts = Arc::new(std::sync::Mutex::new(authority_receipts.clone()));
    assert!(rayon::current_num_threads() >= 2);
    let (release_sender, release_receiver) = std::sync::mpsc::channel();
    let release_receiver = std::sync::Mutex::new(release_receiver);
    let result = stream_npc_decisions_with(
        snapshot,
        &crate::GameConfig::proposed_defaults(),
        |account| {
            if account == AccountId(1) {
                release_receiver
                    .lock()
                    .unwrap()
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .expect("fast account must be consumed before slow worker is released");
            }
        },
        |completed| {
            received.lock().unwrap().push(completed.account);
            candidate_receipts
                .lock()
                .unwrap()
                .receive(
                    completed.account,
                    Intent::PlaceLimit {
                        code: StockCode("600000".to_owned()),
                        side: crate::Side::Buy,
                        price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                        qty: 100,
                    },
                )
                .unwrap();
            if completed.account == AccountId(2) {
                release_sender.send(()).unwrap();
                Err("injected consumer failure")
            } else {
                Ok(())
            }
        },
    );
    assert_eq!(*received.lock().unwrap(), vec![AccountId(2), AccountId(1)]);
    assert!(matches!(
        result,
        Err(super::npc_decisions::NpcDecisionStreamError::Consumer(
            "injected consumer failure"
        ))
    ));
    assert!(authority_receipts.next_account_ordinal.is_empty());
    assert!(authority_receipts.next_stock_ordinal.is_empty());
    assert_eq!(
        candidate_receipts.lock().unwrap().next_account_ordinal[&AccountId(1)],
        1
    );
    assert_eq!(
        candidate_receipts.lock().unwrap().next_account_ordinal[&AccountId(2)],
        1
    );
}

#[test]
fn npc_completion_channel_does_not_deadlock_with_one_rayon_worker() {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    let result = pool.install(|| {
        stream_npc_decisions(
            snapshot(vec![AccountId(1), AccountId(2)]),
            &crate::GameConfig::proposed_defaults(),
            |_| Ok::<_, ()>(()),
        )
    });
    assert!(result.is_ok());
}

#[test]
fn concurrent_sessions_can_queue_npcs_from_every_worker_in_a_two_worker_pool() {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap();
    let (done_sender, done_receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = pool.install(|| {
            [51_u64, 52]
                .into_par_iter()
                .map(|seed| {
                    let mut session = crate::GameSession::new(
                        crate::session::npc_working_quote_tests::retail_quote_setup(),
                        seed,
                    )
                    .unwrap();
                    let account = AccountId(1);
                    super::npc_state_projection_tests::use_buy_imbalance_analysis(
                        &mut session,
                        account,
                    );
                    let tick = session.state.tick;
                    session.state.pending_npc = None;
                    let attention = session.state.npc_attention.get_mut(&account).unwrap();
                    attention.next_attention_candidate_tick = tick;
                    attention.rng_state = 3;
                    let mut quiet_probe = attention.clone();
                    assert!(quiet_probe.evaluate_candidate_with_signal(
                        crate::AccountKind::Retail,
                        0.0,
                        tick,
                    ));
                    session.state.attention_scheduler.enqueue(tick, account);
                    super::npc_tick_preparation::queue_npc_for_next_tick(&mut session).unwrap();
                    session
                        .state
                        .pending_npc
                        .as_ref()
                        .unwrap()
                        .observed_accounts
                        .clone()
                })
                .collect::<Vec<_>>()
        });
        let _ = done_sender.send(result);
    });
    let results = done_receiver
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("two worker-held sessions must both complete their NPC queue");
    assert_eq!(results, vec![vec![AccountId(1)], vec![AccountId(1)]]);
}

fn account_input() -> DecisionAccountInput {
    DecisionAccountInput::new(
        AccountKind::Inst,
        SelfView {
            cash: Money::from_cents(10_000),
            positions: BTreeMap::new(),
        },
        StrategyState::Momentum(MomentumStrategy::new(5, 0.02, 100).unwrap()),
        None,
        None,
    )
}

fn snapshot(accounts: Vec<AccountId>) -> Arc<DecisionSnapshot> {
    let inputs: BTreeMap<_, _> = accounts
        .iter()
        .copied()
        .map(|account| (account, account_input()))
        .collect();
    Arc::new(
        DecisionSnapshot::new(
            7,
            42,
            TradingPhase::Continuous,
            11,
            MarketView {
                stocks: BTreeMap::new(),
                tick: 7,
                market_minute: 11,
            },
            None,
            accounts,
            inputs,
        )
        .unwrap(),
    )
}

fn unavailable_horizon() -> HorizonReturn {
    HorizonReturn {
        requested_span: 30,
        available_span: 0,
        return_ratio: None,
    }
}

fn retail_risk_snapshot(locked: bool, style: &str) -> Arc<DecisionSnapshot> {
    let base = retail_snapshot();
    let code = StockCode("600888".to_owned());
    let mut strategy = serde_json::to_value(ZiNoiseStrategy::new(1.0, 100, 0.5).unwrap()).unwrap();
    strategy["retail_style"] = serde_json::json!(style);
    let risk = AccountRiskObservation {
        equity: Money::from_cents(1_000_000),
        return_from_reference: None,
        drawdown_from_peak: Some(-0.20),
        positions: BTreeMap::from([(
            code.clone(),
            crate::observation::PositionRiskObservation {
                market_value: Money::from_cents(1_000_000),
                unrealized_return: Some(0.0),
                equity_weight: Some(1.0),
                drawdown_from_position_peak: None,
            },
        )]),
    };
    Arc::new(
        DecisionSnapshot::new(
            base.tick(),
            base.npc_seed_base(),
            base.phase(),
            base.market_minute(),
            base.market().clone(),
            base.behavior_market().cloned(),
            vec![AccountId(1)],
            BTreeMap::from([(
                AccountId(1),
                DecisionAccountInput::new(
                    AccountKind::Retail,
                    SelfView {
                        cash: Money::ZERO,
                        positions: BTreeMap::from([(
                            code,
                            crate::strategy::PositionView {
                                qty: 1_000,
                                sellable_qty: if locked { 0 } else { 1_000 },
                                cost_price: Some(Money::from_cents(1_000)),
                            },
                        )]),
                    },
                    StrategyState::ZiNoise(serde_json::from_value(strategy).unwrap()),
                    Some(risk),
                    Some(crate::RetailExperienceState::without_equity_reference()),
                ),
            )]),
        )
        .unwrap(),
    )
}

#[test]
fn urgency_retail_risk_reduction_is_urgent_without_overriding_personal_target_or_t1() {
    for locked in [false, true] {
        let output = run_npc_decisions(
            retail_risk_snapshot(locked, "Momentum"),
            &crate::GameConfig::proposed_defaults(),
        )
        .unwrap();
        let account = &output.account_outputs()[0];
        let decision = account.position_decision().unwrap();
        assert_eq!(decision.action, crate::behavior::PositionAction::Reduce);
        assert!(decision.desired_delta_shares < 0);
        assert!(decision.desired_delta_shares > -1_000);
        assert_eq!(decision.executable_delta_shares == 0, locked);
        assert!(
            format!("{account:?}").contains("RiskReductionDrawdown"),
            "production risk urgency must be assessed: {account:?}"
        );
        assert!(matches!(
            account.execution_urgency(),
            crate::plans::urgency::risk::RiskUrgencyAssessment::Assessed {
                urgency: crate::plans::Urgency::Urgent,
                account_drawdown_bp: Some(2_000),
                risk_reduction_active: true,
                reason: crate::plans::urgency::UrgencyReason::RiskReductionDrawdown,
            }
        ));
        assert_eq!(output.intents().is_empty(), locked);
    }
}

#[test]
fn urgency_retail_source_uses_frozen_drawdown_policy() {
    let base = retail_risk_snapshot(false, "Momentum");
    let policy = crate::plans::UrgencyPolicy {
        urgent_drawdown_threshold_bp: 2_001,
        ..crate::plans::UrgencyPolicy::default()
    };
    let snapshot = Arc::new(base.as_ref().clone().with_urgency_policy(policy).unwrap());
    let output = run_npc_decisions(snapshot, &crate::GameConfig::proposed_defaults()).unwrap();
    assert!(matches!(
        output.account_outputs()[0].execution_urgency(),
        crate::plans::urgency::risk::RiskUrgencyAssessment::Assessed {
            urgency: crate::plans::Urgency::Normal,
            account_drawdown_bp: Some(2_000),
            ..
        }
    ));
    assert_eq!(
        output.account_outputs()[0]
            .position_decision()
            .unwrap()
            .desired_delta_shares,
        -500
    );
}

fn retail_snapshot() -> Arc<DecisionSnapshot> {
    let code = StockCode("600888".to_owned());
    let market = MarketView {
        stocks: [(
            code.clone(),
            StockView {
                is_trading: true,
                best_bid: Some(Money::from_cents(999)),
                best_ask: Some(Money::from_cents(1_001)),
                last_price: Money::from_cents(1_000),
                max_buy_price: Money::from_cents(1_100),
                daily_upper_limit: Money::from_cents(1_100),
                min_sell_price: Money::from_cents(900),
                recent_prices: vec![Money::from_cents(1_000)],
                recent_market_minute_prices: Vec::new(),
                relative_volume: 1.0,
                order_book_imbalance: 0.0,
            },
        )]
        .into(),
        tick: 7,
        market_minute: 11,
    };
    let path = PricePathObservation {
        one_minute: unavailable_horizon(),
        thirty_minute: unavailable_horizon(),
        intraday: unavailable_horizon(),
        five_day: unavailable_horizon(),
        twenty_day: unavailable_horizon(),
        one_hundred_twenty_day: unavailable_horizon(),
        two_hundred_fifty_day: unavailable_horizon(),
        prior_thirty_minute_range: None,
    };
    let behavior_market = BehaviorMarketObservation {
        price_paths: [(code, path)].into(),
        thirty_minute_market: EqualWeightMarketObservation {
            total_stock_count: 1,
            observed_stock_count: 0,
            equal_weight_return: None,
            advance_fraction: None,
            decline_fraction: None,
            unchanged_fraction: None,
        },
    };
    let risk = AccountRiskObservation {
        equity: Money::from_cents(10_000_000),
        return_from_reference: None,
        drawdown_from_peak: None,
        positions: BTreeMap::new(),
    };
    let accounts = [AccountId(1), AccountId(2)]
        .into_iter()
        .map(|account| {
            (
                account,
                DecisionAccountInput::new(
                    AccountKind::Retail,
                    SelfView {
                        cash: Money::from_cents(10_000_000),
                        positions: BTreeMap::new(),
                    },
                    StrategyState::ZiNoise(ZiNoiseStrategy::new(1.0, 100, 0.5).unwrap()),
                    Some(risk.clone()),
                    Some(crate::RetailExperienceState::without_equity_reference()),
                ),
            )
        })
        .collect();
    Arc::new(
        DecisionSnapshot::new(
            7,
            8,
            TradingPhase::Continuous,
            11,
            market,
            Some(behavior_market),
            vec![AccountId(1), AccountId(2)],
            accounts,
        )
        .unwrap(),
    )
}

#[test]
fn retail_personal_analysis_controls_direction_without_institution_execution() {
    let base = retail_snapshot();
    let code = StockCode("600888".to_owned());
    for score in [-8_000, 8_000] {
        let account = AccountId(1);
        let input = base
            .account(account)
            .unwrap()
            .clone()
            .with_retail_analysis(Some(BTreeMap::from([(
                code.clone(),
                crate::plans::CandidateAssessment::Scored {
                    score: crate::plans::SignalScore::new(score).unwrap(),
                    used_weight_bp: 10_000,
                    excluded: Vec::new(),
                },
            )])));
        let snapshot = Arc::new(
            DecisionSnapshot::new(
                base.tick(),
                base.npc_seed_base(),
                base.phase(),
                base.market_minute(),
                base.market().clone(),
                base.behavior_market().cloned(),
                vec![account],
                BTreeMap::from([(account, input)]),
            )
            .unwrap(),
        );
        let output = run_npc_decisions(snapshot, &crate::GameConfig::proposed_defaults()).unwrap();
        assert!(!output.account_outputs()[0].uses_parent_order_execution());
        if score > 0 {
            assert_eq!(output.intents().len(), 1);
            assert!(matches!(
                output.intents()[0].intent(),
                crate::strategy::Intent::PlaceLimit {
                    side: crate::Side::Buy,
                    qty: 100,
                    ..
                }
            ));
        } else {
            assert!(output.intents().is_empty());
        }
    }
}

#[test]
fn retail_personal_analysis_cannot_override_account_risk_or_t1_lock() {
    for locked in [false, true] {
        let base = retail_risk_snapshot(locked, "Momentum");
        let account = AccountId(1);
        let input = base
            .account(account)
            .unwrap()
            .clone()
            .with_retail_analysis(Some(BTreeMap::from([(
                StockCode("600888".to_owned()),
                crate::plans::CandidateAssessment::Scored {
                    score: crate::plans::SignalScore::new(10_000).unwrap(),
                    used_weight_bp: 10_000,
                    excluded: Vec::new(),
                },
            )])));
        let snapshot = Arc::new(
            DecisionSnapshot::new(
                base.tick(),
                base.npc_seed_base(),
                base.phase(),
                base.market_minute(),
                base.market().clone(),
                base.behavior_market().cloned(),
                vec![account],
                BTreeMap::from([(account, input)]),
            )
            .unwrap(),
        );
        let output = run_npc_decisions(snapshot, &crate::GameConfig::proposed_defaults()).unwrap();
        let decision = output.account_outputs()[0].position_decision().unwrap();
        assert_eq!(
            decision.reason,
            if locked {
                crate::behavior::DecisionReason::T1Locked
            } else {
                crate::behavior::DecisionReason::AccountDrawdown
            }
        );
        assert!(decision.desired_delta_shares < 0);
        assert_eq!(decision.executable_delta_shares == 0, locked);
        assert_eq!(output.intents().is_empty(), locked);
    }
}

#[test]
fn retail_analysis_rejects_unknown_market_stock_before_deciding() {
    let base = retail_snapshot();
    let account = AccountId(1);
    let input = base
        .account(account)
        .unwrap()
        .clone()
        .with_retail_analysis(Some(BTreeMap::from([(
            StockCode("missing".to_owned()),
            crate::plans::CandidateAssessment::InsufficientInformation {
                excluded: Vec::new(),
            },
        )])));
    let error = DecisionSnapshot::new(
        base.tick(),
        base.npc_seed_base(),
        base.phase(),
        base.market_minute(),
        base.market().clone(),
        base.behavior_market().cloned(),
        vec![account],
        BTreeMap::from([(account, input)]),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        super::DecisionSnapshotError::UnknownRetailAnalysisStock {
            account: AccountId(1),
            ..
        }
    ));
}

#[test]
fn retail_analysis_insufficient_information_does_not_create_buy_orders() {
    let base = retail_snapshot();
    let account = AccountId(1);
    let input = base
        .account(account)
        .unwrap()
        .clone()
        .with_retail_analysis(Some(BTreeMap::from([(
            StockCode("600888".to_owned()),
            crate::plans::CandidateAssessment::InsufficientInformation {
                excluded: Vec::new(),
            },
        )])));
    let snapshot = Arc::new(
        DecisionSnapshot::new(
            base.tick(),
            base.npc_seed_base(),
            base.phase(),
            base.market_minute(),
            base.market().clone(),
            base.behavior_market().cloned(),
            vec![account],
            BTreeMap::from([(account, input)]),
        )
        .unwrap(),
    );
    let output = run_npc_decisions(snapshot, &crate::GameConfig::proposed_defaults()).unwrap();
    assert!(output.intents().is_empty());
    assert_eq!(
        output.account_outputs()[0]
            .position_decision()
            .unwrap()
            .action,
        crate::behavior::PositionAction::Watch
    );
}

#[test]
fn retail_analysis_consumes_dated_failure_influence_without_erasing_history() {
    let base = retail_snapshot();
    let account = AccountId(1);
    let original = base.account(account).unwrap();
    let mut experience = original.retail_experience().unwrap().clone();
    experience.consecutive_failed_buys = 100;
    let input = DecisionAccountInput::new(
        AccountKind::Retail,
        original.self_view().clone(),
        original.strategy_state().clone(),
        original.account_risk().cloned(),
        Some(experience),
    )
    .with_failure_influence(0)
    .with_retail_analysis(Some(BTreeMap::from([(
        StockCode("600888".to_owned()),
        crate::plans::CandidateAssessment::Scored {
            score: crate::plans::SignalScore::new(8_000).unwrap(),
            used_weight_bp: 10_000,
            excluded: Vec::new(),
        },
    )])));
    let snapshot = Arc::new(
        DecisionSnapshot::new(
            base.tick(),
            base.npc_seed_base(),
            base.phase(),
            base.market_minute(),
            base.market().clone(),
            base.behavior_market().cloned(),
            vec![account],
            BTreeMap::from([(account, input)]),
        )
        .unwrap(),
    );
    let output =
        run_npc_decisions(snapshot.clone(), &crate::GameConfig::proposed_defaults()).unwrap();
    assert_eq!(output.intents().len(), 1);
    assert_eq!(
        snapshot
            .account(account)
            .unwrap()
            .retail_experience()
            .unwrap()
            .consecutive_failed_buys,
        100
    );
}

#[test]
fn retail_negative_personal_analysis_reduces_a_real_sellable_holding() {
    let base = retail_risk_snapshot(false, "Momentum");
    let account = AccountId(1);
    let original = base.account(account).unwrap();
    let mut risk = original.account_risk().unwrap().clone();
    risk.drawdown_from_peak = None;
    let input = DecisionAccountInput::new(
        AccountKind::Retail,
        original.self_view().clone(),
        original.strategy_state().clone(),
        Some(risk),
        original.retail_experience().cloned(),
    )
    .with_retail_analysis(Some(BTreeMap::from([(
        StockCode("600888".to_owned()),
        crate::plans::CandidateAssessment::Scored {
            score: crate::plans::SignalScore::new(-8_000).unwrap(),
            used_weight_bp: 10_000,
            excluded: Vec::new(),
        },
    )])));
    let snapshot = Arc::new(
        DecisionSnapshot::new(
            base.tick(),
            base.npc_seed_base(),
            base.phase(),
            base.market_minute(),
            base.market().clone(),
            base.behavior_market().cloned(),
            vec![account],
            BTreeMap::from([(account, input)]),
        )
        .unwrap(),
    );
    let output = run_npc_decisions(snapshot, &crate::GameConfig::proposed_defaults()).unwrap();
    let decision = output.account_outputs()[0].position_decision().unwrap();
    assert_eq!(
        decision.reason,
        crate::behavior::DecisionReason::PersonalAnalysis
    );
    assert_eq!(decision.desired_delta_shares, -500);
    assert_eq!(decision.executable_delta_shares, -500);
    assert!(matches!(
        output.intents()[0].intent(),
        crate::strategy::Intent::PlaceLimit {
            side: crate::Side::Sell,
            qty: 100,
            ..
        }
    ));
}

#[test]
fn retail_personal_analysis_cannot_create_an_order_without_random_arrival() {
    let base = retail_snapshot();
    let account = AccountId(1);
    let original = base.account(account).unwrap();
    let input = DecisionAccountInput::new(
        AccountKind::Retail,
        original.self_view().clone(),
        StrategyState::ZiNoise(ZiNoiseStrategy::new(1e-12, 100, 0.5).unwrap()),
        original.account_risk().cloned(),
        original.retail_experience().cloned(),
    )
    .with_retail_analysis(Some(BTreeMap::from([(
        StockCode("600888".to_owned()),
        crate::plans::CandidateAssessment::Scored {
            score: crate::plans::SignalScore::new(8_000).unwrap(),
            used_weight_bp: 10_000,
            excluded: Vec::new(),
        },
    )])));
    let snapshot = Arc::new(
        DecisionSnapshot::new(
            base.tick(),
            base.npc_seed_base(),
            base.phase(),
            base.market_minute(),
            base.market().clone(),
            base.behavior_market().cloned(),
            vec![account],
            BTreeMap::from([(account, input)]),
        )
        .unwrap(),
    );
    let output = run_npc_decisions(snapshot, &crate::GameConfig::proposed_defaults()).unwrap();
    assert!(output.intents().is_empty());
    assert_eq!(
        output.account_outputs()[0]
            .position_decision()
            .unwrap()
            .reason,
        crate::behavior::DecisionReason::NoSignal
    );
}

fn multi_intent_snapshot() -> Arc<DecisionSnapshot> {
    let market = MarketView {
        stocks: ["600001", "600002"]
            .into_iter()
            .map(|code| {
                (
                    StockCode(code.to_owned()),
                    StockView {
                        is_trading: true,
                        best_bid: Some(Money::from_cents(1_049)),
                        best_ask: Some(Money::from_cents(1_051)),
                        last_price: Money::from_cents(1_050),
                        max_buy_price: Money::from_cents(1_155),
                        daily_upper_limit: Money::from_cents(1_155),
                        min_sell_price: Money::from_cents(945),
                        recent_prices: vec![
                            Money::from_cents(1_000),
                            Money::from_cents(1_020),
                            Money::from_cents(1_050),
                        ],
                        recent_market_minute_prices: vec![
                            Money::from_cents(1_000),
                            Money::from_cents(1_020),
                            Money::from_cents(1_050),
                        ],
                        relative_volume: 1.0,
                        order_book_imbalance: 0.0,
                    },
                )
            })
            .collect(),
        tick: 7,
        market_minute: 11,
    };
    let account = AccountId(9);
    let accounts = [(
        account,
        DecisionAccountInput::new(
            AccountKind::Hot,
            SelfView {
                cash: Money::from_cents(10_000_000),
                positions: BTreeMap::new(),
            },
            StrategyState::Momentum(MomentumStrategy::new(3, 0.02, 100).unwrap()),
            None,
            None,
        ),
    )]
    .into();
    Arc::new(
        DecisionSnapshot::new(
            7,
            8,
            TradingPhase::Continuous,
            11,
            market,
            None,
            vec![account],
            accounts,
        )
        .unwrap(),
    )
}

#[test]
fn npc_decisions_sizes_buys_with_the_explicit_session_fee_configuration() {
    let snapshot = multi_intent_snapshot();
    let mut config = crate::GameConfig::proposed_defaults();
    let standard = run_npc_decisions(snapshot.clone(), &config).unwrap();
    assert_eq!(standard.intents().len(), 2);

    // 佣金本身已需要全部现金，策略就没有资金再支付成交额。
    // 这也验证 DecisionShadow 不会悄悄使用默认佣金代替传入的会话配置。
    config.commission_min = snapshot.account(AccountId(9)).unwrap().self_view().cash;
    config.validate().unwrap();
    let expensive = run_npc_decisions(snapshot, &config).unwrap();
    assert!(expensive.intents().is_empty());
}

#[test]
fn npc_decisions_hydrates_in_stable_account_order_and_returns_state_without_mutating_snapshot() {
    let first = AccountId(1);
    let second = AccountId(2);
    let snapshot = snapshot(vec![first, second]);
    let snapshot_before = snapshot.clone();

    let output =
        run_npc_decisions(snapshot.clone(), &crate::GameConfig::proposed_defaults()).unwrap();

    assert_eq!(output.accounts(), &[first, second]);
    assert!(output.intents().is_empty());
    assert_eq!(
        output.strategy_state(first).unwrap(),
        snapshot.account(first).unwrap().strategy_state()
    );
    assert_eq!(
        output.strategy_state(second).unwrap(),
        snapshot.account(second).unwrap().strategy_state()
    );
    assert!(Arc::ptr_eq(&snapshot, &snapshot_before));
    assert_eq!(snapshot.due_npc_ids(), &[first, second]);
}

#[test]
fn npc_decisions_is_repeatable_for_the_same_sealed_snapshot() {
    let snapshot = snapshot(vec![AccountId(1)]);

    let first =
        run_npc_decisions(snapshot.clone(), &crate::GameConfig::proposed_defaults()).unwrap();
    let second = run_npc_decisions(snapshot, &crate::GameConfig::proposed_defaults()).unwrap();

    assert_eq!(first.accounts(), second.accounts());
    assert_eq!(
        serde_json::to_vec(first.intents()).unwrap(),
        serde_json::to_vec(second.intents()).unwrap()
    );
    for account in first.accounts() {
        assert_eq!(
            first.strategy_state(*account).unwrap(),
            second.strategy_state(*account).unwrap()
        );
    }
}

#[test]
fn npc_decisions_keeps_retail_decisions_and_candidate_keys_across_thread_pools() {
    let snapshot = retail_snapshot();
    let input_before: Vec<_> = snapshot
        .due_npc_ids()
        .iter()
        .map(|account| {
            let input = snapshot.account(*account).unwrap();
            (
                *account,
                serde_json::to_vec(input.self_view()).unwrap(),
                serde_json::to_vec(input.strategy_state()).unwrap(),
            )
        })
        .collect();
    let one_thread = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(|| {
            run_npc_decisions(snapshot.clone(), &crate::GameConfig::proposed_defaults()).unwrap()
        });
    let two_threads = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap()
        .install(|| {
            run_npc_decisions(snapshot.clone(), &crate::GameConfig::proposed_defaults()).unwrap()
        });

    assert_eq!(one_thread.accounts(), &[AccountId(1), AccountId(2)]);
    assert_eq!(
        serde_json::to_vec(one_thread.intents()).unwrap(),
        serde_json::to_vec(two_threads.intents()).unwrap()
    );
    assert_eq!(one_thread.intents().len(), 2);
    for (index, intent) in one_thread.intents().iter().enumerate() {
        assert_eq!(
            intent.key(),
            &IntentCandidateKey::npc(AccountId(u64::try_from(index + 1).unwrap()), 0)
        );
        assert!(matches!(intent.intent(), crate::Intent::PlaceLimit { .. }));
    }

    for account in snapshot.due_npc_ids() {
        let input = snapshot.account(*account).unwrap();
        let mut direct_strategy = input.strategy_state().clone().into_strategy().unwrap();
        let account_seed = snapshot.npc_seed_base()
            ^ snapshot.tick().wrapping_mul(0x9E3779B97F4A7C15)
            ^ account.0.wrapping_mul(0x6A09E667F3BCC908);
        let mut rng = SplitMix64::new(account_seed);
        let expected = direct_strategy.decide_with_experience(
            snapshot.market(),
            input.self_view(),
            snapshot.behavior_market(),
            input.account_risk(),
            input.retail_experience(),
            snapshot.market_minute(),
            &mut rng,
            &crate::GameConfig::proposed_defaults(),
        );
        let actual = one_thread
            .account_outputs()
            .iter()
            .find(|output| output.account() == *account)
            .unwrap();
        assert!(actual.position_decision().is_some());
        assert!(!actual.reviewed_stocks().is_empty());
        assert!(actual.updates_working_quotes());
        assert!(!actual.uses_parent_order_execution());
        assert_eq!(
            actual.position_decision(),
            expected.position_decision.as_ref()
        );
        let actual_intents: Vec<_> = one_thread
            .intents()
            .iter()
            .filter(|intent| matches!(intent.key(), IntentCandidateKey::Npc { account: owner, .. } if owner == account))
            .map(|intent| serde_json::to_vec(intent.intent()).unwrap())
            .collect();
        let expected_intents: Vec<_> = expected
            .intents
            .iter()
            .map(|intent| serde_json::to_vec(intent).unwrap())
            .collect();
        assert_eq!(actual_intents, expected_intents);
    }
    let input_after: Vec<_> = snapshot
        .due_npc_ids()
        .iter()
        .map(|account| {
            let input = snapshot.account(*account).unwrap();
            (
                *account,
                serde_json::to_vec(input.self_view()).unwrap(),
                serde_json::to_vec(input.strategy_state()).unwrap(),
            )
        })
        .collect();
    assert_eq!(input_after, input_before);
}

#[test]
fn npc_decisions_assigns_incrementing_local_indexes_to_one_accounts_multiple_intents() {
    let output = run_npc_decisions(
        multi_intent_snapshot(),
        &crate::GameConfig::proposed_defaults(),
    )
    .unwrap();

    assert_eq!(output.intents().len(), 2);
    assert_eq!(
        output.intents()[0].key(),
        &IntentCandidateKey::npc(AccountId(9), 0)
    );
    assert_eq!(
        output.intents()[1].key(),
        &IntentCandidateKey::npc(AccountId(9), 1)
    );
    assert!(output.intents().iter().all(|intent| matches!(
        intent.intent(),
        crate::Intent::PlaceLimit {
            side: crate::Side::Buy,
            ..
        }
    )));
}

#[test]
fn npc_decisions_derives_rng_from_tick_and_account() {
    let base = 0x1234_5678_9abc_def0;
    let tick = 97_u64;
    let account = AccountId(41);

    assert_eq!(
        npc_rng_seed(base, tick, account),
        base ^ tick.wrapping_mul(0x9E3779B97F4A7C15) ^ account.0.wrapping_mul(0x6A09E667F3BCC908)
    );
}

#[test]
fn npc_decisions_reports_first_invalid_account_independent_of_worker_count() {
    let first = AccountId(1);
    let second = AccountId(2);
    let third = AccountId(3);
    let mut invalid_state = serde_json::to_value(StrategyState::Momentum(
        MomentumStrategy::new(5, 0.02, 100).unwrap(),
    ))
    .unwrap();
    invalid_state["Momentum"]["order_size"] = serde_json::json!(0);
    let invalid_state: StrategyState = serde_json::from_value(invalid_state).unwrap();
    let mut accounts = BTreeMap::new();
    accounts.insert(first, account_input());
    accounts.insert(
        second,
        DecisionAccountInput::new(
            AccountKind::Inst,
            SelfView {
                cash: Money::from_cents(10_000),
                positions: BTreeMap::new(),
            },
            invalid_state.clone(),
            None,
            None,
        ),
    );
    accounts.insert(
        third,
        DecisionAccountInput::new(
            AccountKind::Inst,
            SelfView {
                cash: Money::from_cents(10_000),
                positions: BTreeMap::new(),
            },
            invalid_state,
            None,
            None,
        ),
    );
    let snapshot = Arc::new(
        DecisionSnapshot::new(
            7,
            42,
            TradingPhase::Continuous,
            11,
            MarketView {
                stocks: BTreeMap::new(),
                tick: 7,
                market_minute: 11,
            },
            None,
            vec![first, second, third],
            accounts,
        )
        .unwrap(),
    );
    let encoded_before =
        serde_json::to_vec(snapshot.account(second).unwrap().strategy_state()).unwrap();

    for workers in [1, 4] {
        let error = rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .unwrap()
            .install(|| {
                run_npc_decisions(snapshot.clone(), &crate::GameConfig::proposed_defaults())
            })
            .unwrap_err();
        assert!(matches!(
            error,
            NpcDecisionSourceError::StrategyHydration { account, .. } if account == second
        ));
    }
    assert_eq!(
        serde_json::to_vec(snapshot.account(second).unwrap().strategy_state()).unwrap(),
        encoded_before
    );
}
