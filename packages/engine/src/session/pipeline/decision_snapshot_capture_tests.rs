use super::decision_snapshot_capture::*;
use super::*;
use crate::session::npc_working_quote_tests;
use crate::{AccountId, AccountKind, Money};

#[test]
fn retail_capture_seals_personal_analysis_for_the_real_p2_source() {
    let mut setup = npc_working_quote_tests::retail_quote_setup();
    setup.strategy_params.retail.arrival_rate = 1.0;
    let mut shadow = GameSession::new(setup, 41).unwrap();
    let account = AccountId(1);
    let profile = shadow.state.accounts[&account]
        .strategy()
        .unwrap()
        .profile();
    let analysis = crate::strategy::AnalysisProfile::new(
        crate::strategy::AnalysisWeights::new(0, 0, 10_000, 0, 0).unwrap(),
        None,
    )
    .unwrap();
    *shadow
        .state
        .belief_participants
        .get_mut(&account)
        .unwrap()
        .belief_mut() = crate::strategy::BeliefBook::new(
        account,
        profile,
        analysis,
        &mut crate::SplitMix64::new(99),
    );
    let attention = shadow.state.npc_attention.get_mut(&account).unwrap();
    attention.next_attention_candidate_tick = 0;
    attention.rng_state = 3;
    shadow.state.attention_scheduler.clear();
    shadow.state.attention_scheduler.enqueue(0, account);
    let code = shadow.state.markets.keys().next().unwrap().clone();
    shadow
        .state
        .markets
        .get_mut(&code)
        .unwrap()
        .place(crate::Order {
            id: crate::OrderId(1),
            owner: AccountId(0),
            side: crate::Side::Buy,
            price: Money::from_cents(950),
            qty: 100,
            original_qty: 100,
            filled_qty: 0,
            filled_value: Money::ZERO,
            seq: 1,
        })
        .unwrap();
    let captured = capture_decision_snapshot(&mut shadow).unwrap();
    assert_eq!(captured.snapshot.due_npc_ids(), &[account]);
    let assessments = captured
        .snapshot
        .account(account)
        .unwrap()
        .retail_analysis()
        .unwrap();
    assert_eq!(assessments.len(), 1);
    let crate::plans::CandidateAssessment::Scored { score, .. } =
        assessments.values().next().unwrap()
    else {
        panic!("the personally observed buy-book imbalance must produce a scored candidate");
    };
    let output = super::npc_decisions::run_npc_decisions(
        captured.snapshot.clone(),
        &shadow.state.setup.config,
    )
    .unwrap();
    let decision = output.account_outputs()[0].position_decision().unwrap();
    assert_eq!(
        decision.reason,
        crate::behavior::DecisionReason::PersonalAnalysis
    );
    if score.value() > 0 {
        assert!(decision.desired_delta_shares > 0);
    } else {
        assert!(decision.desired_delta_shares <= 0);
    }
    assert!(!output.account_outputs()[0].uses_parent_order_execution());
}

#[test]
fn capture_only_copies_working_orders_owned_by_observed_npcs() {
    let mut setup = npc_working_quote_tests::two_stock_quote_setup();
    setup.npcs.inst_count = 2;
    let mut shadow = GameSession::new(setup, 41).unwrap();
    let observed = AccountId(1);
    let unobserved = AccountId(2);
    let player = AccountId(0);
    let codes = shadow.state.markets.keys().cloned().collect::<Vec<_>>();
    let tick = shadow.state.tick;
    shadow.state.attention_scheduler.clear();
    npc_working_quote_tests::force_attention_candidate(&mut shadow, observed, tick);
    shadow
        .state
        .npc_attention
        .get_mut(&unobserved)
        .unwrap()
        .next_attention_candidate_tick = tick + 1;
    shadow
        .state
        .attention_scheduler
        .enqueue(tick + 1, unobserved);
    for (code, id, owner) in [
        (&codes[0], 9, observed),
        (&codes[0], 3, observed),
        (&codes[0], 4, unobserved),
        (&codes[0], 5, player),
        (&codes[1], 11, observed),
    ] {
        shadow
            .state
            .markets
            .get_mut(code)
            .unwrap()
            .place(crate::Order {
                id: crate::OrderId(id),
                side: crate::Side::Buy,
                price: Money::from_cents(if id == 3 { 901 } else { 900 }),
                qty: 100,
                original_qty: 100,
                filled_qty: 0,
                filled_value: Money::ZERO,
                owner,
                seq: id,
            })
            .unwrap();
        shadow
            .state
            .auction_orders
            .entry(code.clone())
            .or_default()
            .push(crate::session::AuctionOrderSnap {
                owner,
                side: crate::Side::Buy,
                limit: Money::from_cents(900),
                qty: 100,
                order_id: id,
            });
    }

    let captured = capture_decision_snapshot(&mut shadow).unwrap();

    assert_eq!(captured.snapshot.due_npc_ids(), &[observed]);
    assert_eq!(
        captured
            .working_continuous
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        vec![observed]
    );
    assert_eq!(
        captured.working_auction.keys().copied().collect::<Vec<_>>(),
        vec![observed]
    );
    assert_eq!(
        captured.working_continuous[&observed]
            .iter()
            .map(|(code, order)| (code.clone(), order.id))
            .collect::<Vec<_>>(),
        vec![
            (codes[0].clone(), crate::OrderId(9)),
            (codes[0].clone(), crate::OrderId(3)),
            (codes[1].clone(), crate::OrderId(11)),
        ]
    );
    assert_eq!(
        captured.working_auction[&observed]
            .iter()
            .map(|(code, order)| (code.clone(), order.order_id))
            .collect::<Vec<_>>(),
        vec![
            (codes[0].clone(), 9),
            (codes[0].clone(), 3),
            (codes[1].clone(), 11),
        ]
    );
}

fn queue_entries(session: &GameSession) -> Vec<(u64, AccountId)> {
    let mut entries: Vec<_> = session.state.attention_scheduler.iter().copied().collect();
    entries.sort_unstable();
    entries
}

fn due_retail_shadow(seed: u64) -> (GameSession, GameSession, AccountId) {
    let account = AccountId(1);
    let source = GameSession::new(npc_working_quote_tests::retail_quote_setup(), seed).unwrap();
    let mut shadow = source.clone_for_tick_shadow().unwrap();
    let tick = shadow.state.tick;
    npc_working_quote_tests::force_attention_candidate(&mut shadow, account, tick);
    (source, shadow, account)
}

#[test]
fn accepted_retail_observation_dates_a_fixture_buy_and_decays_only_its_influence() {
    let (_, mut shadow, account) = due_retail_shadow(42);
    let code = shadow.state.setup.stocks[0].code.clone();
    let price = shadow.state.markets[&code].last_price();
    let moment = crate::experience::ExperienceMoment {
        civil_date: shadow.civil_date(),
        market_minute: shadow.current_market_minute(),
        trading_day: u64::from(shadow.state.day),
    };
    shadow
        .state
        .accounts
        .get_mut(&account)
        .unwrap()
        .fixture_insert_position(
            code.clone(),
            crate::Position::from_restored_parts(100, 100, price.cents() * 200, 0),
        );
    shadow
        .state
        .retail_experience
        .get_mut(&account)
        .unwrap()
        .record_fill_dated(
            &code,
            crate::Side::Buy,
            Money::from_cents(price.cents() * 2),
            0,
            100,
            None,
            Some(17),
            moment,
        )
        .unwrap();
    capture_decision_snapshot(&mut shadow).unwrap();
    let state = &shadow.state.retail_experience[&account];
    assert_eq!(state.feedback.failure_events.len(), 1);
    assert_eq!(state.feedback.failure_events[0].moment, moment);
    assert_eq!(state.consecutive_failed_buys, 1);
    shadow.state.day = 19;
    shadow.state.tick = u64::from(shadow.state.day) * shadow.state.setup.ticks_per_day;
    let tick = shadow.state.tick;
    npc_working_quote_tests::force_attention_candidate(&mut shadow, account, tick);
    let nineteen = capture_decision_snapshot(&mut shadow).unwrap();
    assert_eq!(
        nineteen
            .snapshot
            .account(account)
            .unwrap()
            .failure_influence(),
        Some(1)
    );
    shadow.state.day = 20;
    shadow.state.tick = u64::from(shadow.state.day) * shadow.state.setup.ticks_per_day;
    let tick = shadow.state.tick;
    npc_working_quote_tests::force_attention_candidate(&mut shadow, account, tick);
    let twenty = capture_decision_snapshot(&mut shadow).unwrap();
    assert_eq!(
        twenty
            .snapshot
            .account(account)
            .unwrap()
            .failure_influence(),
        Some(0)
    );
    let state = &shadow.state.retail_experience[&account];
    assert_eq!(state.consecutive_failed_buys, 1);
    assert_eq!(state.feedback.failure_events.len(), 1);
}

#[test]
fn capture_advances_attention_and_seals_owned_views_on_shadow_only() {
    let (source, mut shadow, account) = due_retail_shadow(0xC0FFEE);
    let source_before = source.session_state_hash().unwrap();
    let attention_before = shadow.state.npc_attention[&account].clone();
    let attention_keys_before: Vec<_> = shadow.state.npc_attention.keys().copied().collect();

    let captured = capture_decision_snapshot(&mut shadow).unwrap();
    // 夹具前置：quote_setup 的 float_shares=0，唯一 NPC 无持仓、无工作单。
    assert!(captured.working_continuous.is_empty());
    assert!(captured.working_auction.is_empty());
    let snapshot = &captured.snapshot;

    // 夹具把唯一 NPC 强制为到期且观察概率 1：due 列表就是该账户。
    assert_eq!(snapshot.due_npc_ids(), &[account]);
    assert_eq!(snapshot.npc_seed_base(), 0xC0FFEE);
    assert_eq!(snapshot.tick(), shadow.state.tick);
    assert_eq!(snapshot.phase(), shadow.phase());
    // 市场与行为观察必须取自捕获前的输入状态；source 是未被触碰的同源副本。
    assert_eq!(
        serde_json::to_vec(snapshot.market()).unwrap(),
        serde_json::to_vec(&source.build_market_view()).unwrap()
    );
    assert_eq!(
        snapshot.behavior_market(),
        Some(&source.behavior_market_observation())
    );
    let input = snapshot.account(account).unwrap();
    assert_eq!(input.kind(), AccountKind::Retail);
    // 显式期望：无工作单 → 冻结额为 0，视图现金即账户现金；无持仓 → 空持仓表。
    assert_eq!(
        input.self_view().cash,
        source.state.accounts[&account].cash()
    );
    assert!(input.self_view().positions.is_empty());
    // 风险观察存在即可：其内容契约由 observation 模块与 worker 一致性测试覆盖。
    assert!(input.account_risk().is_some());
    // 快照中的经验观察与捕获后的 shadow 状态一致（观察真实发生并写入 shadow）。
    assert_eq!(
        serde_json::to_vec(input.retail_experience().unwrap()).unwrap(),
        serde_json::to_vec(&shadow.state.retail_experience[&account]).unwrap()
    );
    // 注意力在 shadow 上推进：唯一账户候选刻后移，新候选已入队，且无逾期条目残留
    // （重复入队由 pop_due_npc_ids 的 next_attention_candidate_tick 比对过滤，属设计容忍）。
    assert_ne!(shadow.state.npc_attention[&account], attention_before);
    assert!(shadow.state.npc_attention[&account].next_attention_candidate_tick > shadow.state.tick);
    let next = shadow.state.npc_attention[&account].next_attention_candidate_tick;
    let entries = queue_entries(&shadow);
    assert!(entries.contains(&(next, account)));
    assert!(entries
        .iter()
        .all(|(scheduled, _)| *scheduled > shadow.state.tick));
    let attention_keys_after: Vec<_> = shadow.state.npc_attention.keys().copied().collect();
    assert_eq!(attention_keys_after, attention_keys_before);
    // 捕获只写 shadow：源会话状态哈希不变。
    assert_eq!(source.session_state_hash().unwrap(), source_before);
}

#[test]
fn capture_preserves_no_due_fast_path_without_fabricating_retail_observations() {
    let mut shadow = GameSession::new(npc_working_quote_tests::quote_setup(0), 17).unwrap();
    shadow.state.attention_scheduler.clear();
    let attention_before = shadow.state.npc_attention.clone();
    let experience_before = shadow.state.retail_experience.clone();

    let captured = capture_decision_snapshot(&mut shadow).unwrap();
    assert!(captured.working_continuous.is_empty());
    assert!(captured.working_auction.is_empty());
    let snapshot = &captured.snapshot;

    assert!(snapshot.due_npc_ids().is_empty());
    assert!(
        super::npc_decisions::run_npc_decisions(snapshot.clone(), &shadow.state.setup.config)
            .unwrap()
            .intents()
            .is_empty()
    );
    assert!(snapshot.behavior_market().is_none());
    assert_eq!(shadow.state.npc_attention, attention_before);
    assert_eq!(shadow.state.retail_experience, experience_before);
}

#[test]
fn capture_reports_retail_experience_overflow() {
    let (_, mut shadow, account) = due_retail_shadow(23);
    let code = shadow
        .state
        .markets
        .keys()
        .next()
        .cloned()
        .expect("retail fixture has a market");
    let last = shadow.state.markets[&code].last_price();
    shadow
        .state
        .accounts
        .get_mut(&account)
        .unwrap()
        .grant_position(code.clone(), 100, last)
        .unwrap();
    let market_minute = shadow.current_market_minute();
    let moment = crate::experience::ExperienceMoment {
        civil_date: shadow.civil_date(),
        market_minute,
        trading_day: u64::from(shadow.state.day),
    };
    let experience = shadow.state.retail_experience.get_mut(&account).unwrap();
    experience
        .initialize_holding_dated(&code, Some(last), last, moment)
        .unwrap();
    experience.consecutive_failed_buys = u16::MAX;
    let stock = experience.stocks.get_mut(&code).unwrap();
    stock.last_buy_price = Some(Money::from_cents(last.cents().checked_mul(2).unwrap()));
    stock.adverse_move_recorded = false;
    let error = capture_decision_snapshot(&mut shadow).unwrap_err();

    assert!(matches!(
        error,
        DecisionSnapshotCaptureError::Experience {
            account: failed,
            source: crate::ExperienceError::CounterOverflow,
        } if failed == account
    ));
}

#[test]
fn capture_reports_a_due_npc_without_a_strategy() {
    let (_, mut shadow, account) = due_retail_shadow(29);
    shadow
        .state
        .accounts
        .get_mut(&account)
        .unwrap()
        .fixture_set_strategy(None);
    let error = capture_decision_snapshot(&mut shadow).unwrap_err();

    assert!(matches!(
        error,
        DecisionSnapshotCaptureError::MissingStrategy(failed) if failed == account
    ));
}

#[test]
fn capture_reports_missing_due_attention_without_panic() {
    let (_, mut shadow, account) = due_retail_shadow(31);
    shadow.state.npc_attention.remove(&account);
    let error = capture_decision_snapshot(&mut shadow).unwrap_err();

    assert!(matches!(
        error,
        DecisionSnapshotCaptureError::MissingAttention(failed) if failed == account
    ));
}

#[test]
fn capture_reports_risk_view_failure() {
    let (_, mut shadow, account) = due_retail_shadow(37);
    shadow
        .state
        .accounts
        .get_mut(&account)
        .unwrap()
        .fixture_set_cash(Money::from_cents(-1));
    let error = capture_decision_snapshot(&mut shadow).unwrap_err();

    assert!(matches!(
        error,
        DecisionSnapshotCaptureError::Observation {
            location: "account risk",
            account: Some(failed),
            source: crate::observation::ObservationError::NegativeCash { cents: -1 },
        } if failed == account
    ));
}

#[test]
fn capture_reports_invalid_attention_after_an_earlier_account() {
    let mut setup = npc_working_quote_tests::retail_quote_setup();
    setup.npcs.retail_count = 3;
    let mut shadow = GameSession::new(setup, 0xA77E).unwrap();
    shadow.state.tick = 1;
    let first = AccountId(1);
    let second = AccountId(2);
    npc_working_quote_tests::force_attention_candidate(&mut shadow, first, 1);
    npc_working_quote_tests::force_attention_candidate(&mut shadow, second, 1);
    shadow.state.attention_scheduler.enqueue(0, first);
    shadow
        .state
        .npc_attention
        .get_mut(&second)
        .unwrap()
        .base_probability = 0.0;
    let error = capture_decision_snapshot(&mut shadow).unwrap_err();

    assert!(
        matches!(error, DecisionSnapshotCaptureError::InvalidAttentionProbability { account, .. } if account == second)
    );
}

#[test]
fn capture_two_retail_accounts_is_identical_with_one_or_four_workers() {
    let mut setup = npc_working_quote_tests::retail_quote_setup();
    setup.npcs.retail_count = 2;
    let mut source = GameSession::new(setup, 0xC011EC7).unwrap();
    let tick = source.state.tick;
    for account in [AccountId(1), AccountId(2)] {
        npc_working_quote_tests::force_attention_candidate(&mut source, account, tick);
    }
    let run = |workers| {
        let mut shadow = source.clone_for_tick_shadow().unwrap();
        let snapshot = rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .unwrap()
            .install(|| capture_decision_snapshot(&mut shadow))
            .unwrap();
        let snapshot = &snapshot.snapshot;
        let inputs = snapshot
            .due_npc_ids()
            .iter()
            .map(|account| {
                let input = snapshot.account(*account).unwrap();
                (
                    *account,
                    serde_json::to_vec(input.self_view()).unwrap(),
                    input.account_risk().unwrap().clone(),
                    serde_json::to_vec(input.retail_experience().unwrap()).unwrap(),
                    serde_json::to_vec(input.strategy_state()).unwrap(),
                )
            })
            .collect::<Vec<_>>();
        (
            snapshot.due_npc_ids().to_vec(),
            inputs,
            serde_json::to_vec(snapshot.market()).unwrap(),
            snapshot.behavior_market().cloned(),
            shadow.session_state_hash().unwrap(),
        )
    };

    let one = run(1);
    let four = run(4);
    assert_eq!(one, four);
    assert_eq!(one.0, vec![AccountId(1), AccountId(2)]);
}

#[test]
fn capture_held_retail_positions_share_equity_peaks_and_t1_with_experience() {
    let mut setup = npc_working_quote_tests::two_stock_quote_setup();
    setup.npcs.inst_count = 0;
    setup.npcs.retail_count = 1;
    setup.stocks[1].initial_price = Money::from_cents(2_000);
    let mut source = GameSession::new(setup, 53).unwrap();
    let account = AccountId(1);
    let codes = source.state.markets.keys().cloned().collect::<Vec<_>>();
    let mut experience = crate::RetailExperienceState::without_equity_reference();
    for (index, qty, locked, peak) in [(0, 100, 40, 2_000), (1, 200, 150, 1_500)] {
        let price = source.state.markets[&codes[index]].last_price();
        let entry = source.state.accounts.get_mut(&account).unwrap();
        entry
            .grant_position(codes[index].clone(), qty, price)
            .unwrap();
        entry.fixture_insert_position(
            codes[index].clone(),
            crate::Position::from_restored_parts(qty, locked, i64::from(qty) * price.cents(), 0),
        );
        experience
            .initialize_holding_dated(
                &codes[index],
                Some(price),
                Money::from_cents(peak),
                crate::experience::ExperienceMoment {
                    civil_date: source.civil_date(),
                    market_minute: 0,
                    trading_day: u64::from(source.state.day),
                },
            )
            .unwrap();
    }
    // 两个持仓不占未持仓关注列表额度；九个未持仓项目中只移除最旧者。
    let unheld = (0..9)
        .map(|index| crate::StockCode(format!("watch-{index}")))
        .collect::<Vec<_>>();
    for (index, code) in unheld.iter().enumerate() {
        experience.observe_stock(code, index as u64);
    }
    source.state.retail_experience.insert(account, experience);
    let tick = source.state.tick;
    npc_working_quote_tests::force_attention_candidate(&mut source, account, tick);
    let before = source.session_state_hash().unwrap();
    let raw_cash = source.state.accounts[&account].cash();
    let mut shadow = source.clone_for_tick_shadow().unwrap();

    let captured = capture_decision_snapshot(&mut shadow).unwrap();
    let input = captured.snapshot.account(account).unwrap();
    let experience = input.retail_experience().unwrap();
    let risk = input.account_risk().unwrap();
    let equity = raw_cash.add(Money::from_cents(500_000)).unwrap();
    assert_eq!(risk.equity, equity);
    assert_eq!(experience.reference_equity, Some(equity));
    assert_eq!(experience.peak_equity, Some(equity));
    assert_eq!(
        risk.positions[&codes[0]].market_value,
        Money::from_cents(100_000)
    );
    assert_eq!(
        risk.positions[&codes[1]].market_value,
        Money::from_cents(400_000)
    );
    assert_eq!(
        risk.positions[&codes[0]].drawdown_from_position_peak,
        Some(-0.5)
    );
    assert_eq!(
        risk.positions[&codes[1]].drawdown_from_position_peak,
        Some(0.0)
    );
    for (index, qty, sellable, peak) in [(0, 100, 60, 2_000), (1, 200, 50, 2_000)] {
        let position = &input.self_view().positions[&codes[index]];
        assert_eq!((position.qty, position.sellable_qty), (qty, sellable));
        let stock = &experience.stocks[&codes[index]];
        assert_eq!(stock.peak_price_since_entry, Some(Money::from_cents(peak)));
        assert_eq!(
            stock.last_observed_market_minute,
            captured.snapshot.market_minute()
        );
    }
    assert!(!experience.stocks.contains_key(&unheld[0]));
    assert!(unheld[1..]
        .iter()
        .all(|code| experience.stocks.contains_key(code)));
    assert_eq!(experience, &shadow.state.retail_experience[&account]);
    assert_eq!(source.session_state_hash().unwrap(), before);
}

#[test]
fn capture_non_retail_experience_keeps_self_positions_without_retail_risk() {
    let mut shadow = GameSession::new(npc_working_quote_tests::quote_setup(0), 59).unwrap();
    let account = AccountId(1);
    let code = shadow.state.markets.keys().next().unwrap().clone();
    let price = shadow.state.markets[&code].last_price();
    shadow
        .state
        .accounts
        .get_mut(&account)
        .unwrap()
        .grant_position(code.clone(), 100, price)
        .unwrap();
    let mut experience = crate::RetailExperienceState::without_equity_reference();
    experience
        .initialize_holding(&code, Some(price), price, 0)
        .unwrap();
    shadow.state.retail_experience.insert(account, experience);
    let tick = shadow.state.tick;
    npc_working_quote_tests::force_attention_candidate(&mut shadow, account, tick);

    let captured = capture_decision_snapshot(&mut shadow).unwrap();
    let input = captured.snapshot.account(account).unwrap();
    assert_eq!(input.kind(), AccountKind::Inst);
    assert!(input.account_risk().is_none());
    assert!(captured.snapshot.behavior_market().is_none());
    assert_eq!(input.self_view().positions[&code].qty, 100);
    assert_eq!(
        input.retail_experience(),
        Some(&shadow.state.retail_experience[&account])
    );
}

#[test]
fn capture_missing_position_market_does_not_update_the_source_session() {
    let (mut source, _, account) = due_retail_shadow(61);
    let code = crate::StockCode("600999".to_owned());
    source
        .state
        .accounts
        .get_mut(&account)
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(1_000))
        .unwrap();
    let tick = source.state.tick;
    npc_working_quote_tests::force_attention_candidate(&mut source, account, tick);
    let before = source.session_state_hash().unwrap();
    let mut shadow = source.clone_for_tick_shadow().unwrap();
    let error = capture_decision_snapshot(&mut shadow).unwrap_err();

    assert!(
        matches!(error, DecisionSnapshotCaptureError::MissingMarketData {
        location: "retail experience position", code: failed
    } if failed == code)
    );
    assert_eq!(source.session_state_hash().unwrap(), before);
}

#[test]
fn capture_equity_overflow_does_not_install_observed_experience() {
    let (mut source, _, account) = due_retail_shadow(67);
    let code = source.state.markets.keys().next().unwrap().clone();
    let entry = source.state.accounts.get_mut(&account).unwrap();
    entry
        .grant_position(code, 100, Money::from_cents(1_000))
        .unwrap();
    entry.fixture_set_cash(Money::from_cents(i64::MAX));
    let tick = source.state.tick;
    npc_working_quote_tests::force_attention_candidate(&mut source, account, tick);
    let before = source.session_state_hash().unwrap();
    let experience_before = source.state.retail_experience.clone();
    let mut shadow = source.clone_for_tick_shadow().unwrap();
    let error = capture_decision_snapshot(&mut shadow).unwrap_err();

    assert!(matches!(error, DecisionSnapshotCaptureError::Money {
        location: "retail experience equity", account: failed, ..
    } if failed == account));
    assert_eq!(shadow.state.retail_experience, experience_before);
    assert_eq!(source.session_state_hash().unwrap(), before);
}

#[test]
fn capture_self_view_money_error_precedes_risk_and_missing_strategy() {
    let (_, mut shadow, account) = due_retail_shadow(71);
    let code = shadow.state.markets.keys().next().unwrap().clone();
    let entry = shadow.state.accounts.get_mut(&account).unwrap();
    entry.fixture_set_cash(Money::from_cents(i64::MIN));
    entry.fixture_set_strategy(None);
    shadow
        .state
        .markets
        .get_mut(&code)
        .unwrap()
        .place(crate::Order {
            id: crate::OrderId(1),
            owner: account,
            side: crate::Side::Buy,
            price: Money::from_cents(900),
            qty: 100,
            original_qty: 100,
            filled_qty: 0,
            filled_value: Money::ZERO,
            seq: 1,
        })
        .unwrap();
    let error = capture_decision_snapshot(&mut shadow).unwrap_err();

    assert!(matches!(error, DecisionSnapshotCaptureError::Money {
        location: "available self-view cash", account: failed, ..
    } if failed == account));
}

#[test]
fn capture_risk_error_precedes_missing_strategy() {
    let (_, mut shadow, account) = due_retail_shadow(73);
    let entry = shadow.state.accounts.get_mut(&account).unwrap();
    entry.fixture_set_cash(Money::from_cents(-1));
    entry.fixture_set_strategy(None);
    let error = capture_decision_snapshot(&mut shadow).unwrap_err();

    assert!(matches!(error, DecisionSnapshotCaptureError::Observation {
        location: "account risk", account: Some(failed),
        source: crate::observation::ObservationError::NegativeCash { cents: -1 },
    } if failed == account));
}

#[test]
fn capture_mixed_working_orders_only_restore_cash_for_the_cancelable_subset() {
    let mut setup = npc_working_quote_tests::retail_quote_setup();
    setup.ticks_per_day = 15_300;
    setup.auction_ticks = 900;
    setup.closing_auction_ticks = 300;
    let source = GameSession::new(setup, 79).unwrap();
    let account = AccountId(1);
    let code = source.state.markets.keys().next().unwrap().clone();
    for (tick, expected_phase, replace_continuous, replace_auction) in [
        (0, crate::TradingPhase::CallAuction, false, true),
        (300, crate::TradingPhase::CallAuction, false, false),
        (600, crate::TradingPhase::PreOpen, false, false),
        (900, crate::TradingPhase::Continuous, true, false),
        (15_000, crate::TradingPhase::ClosingAuction, false, false),
    ] {
        let mut shadow = source.clone_for_tick_shadow().unwrap();
        shadow.state.tick = tick;
        shadow.state.attention_scheduler.clear();
        npc_working_quote_tests::force_attention_candidate(&mut shadow, account, tick);
        for (id, side) in [(1, crate::Side::Buy), (2, crate::Side::Sell)] {
            shadow
                .state
                .markets
                .get_mut(&code)
                .unwrap()
                .place(crate::Order {
                    id: crate::OrderId(id),
                    owner: account,
                    side,
                    price: Money::from_cents(if side == crate::Side::Buy { 900 } else { 1_100 }),
                    qty: 100,
                    original_qty: 100,
                    filled_qty: 0,
                    filled_value: Money::ZERO,
                    seq: id,
                })
                .unwrap();
            shadow
                .state
                .auction_orders
                .entry(code.clone())
                .or_default()
                .push(crate::session::AuctionOrderSnap {
                    owner: account,
                    side,
                    limit: Money::from_cents(800),
                    qty: 100,
                    order_id: id + 2,
                });
        }
        // 买单预留分别为成交额 + 500 分佣金 + 1 分过户费；卖单不预留现金。
        let continuous = Money::from_cents(90_501);
        let auction = Money::from_cents(80_501);
        let mut expected = shadow.state.accounts[&account]
            .cash()
            .sub(continuous)
            .unwrap()
            .sub(auction)
            .unwrap();
        if replace_continuous {
            expected = expected.add(continuous).unwrap();
        }
        if replace_auction {
            expected = expected.add(auction).unwrap();
        }

        let captured = capture_decision_snapshot(&mut shadow).unwrap();
        assert_eq!(captured.snapshot.phase(), expected_phase);
        assert_eq!(
            captured.snapshot.account(account).unwrap().self_view().cash,
            expected
        );
    }
}
