use super::decision_snapshot_capture::*;
use super::*;
use crate::session::npc_working_quote_tests;
use crate::{AccountId, AccountKind, Money};

#[test]
fn capture_only_copies_working_orders_owned_by_observed_npcs() {
    let mut setup = npc_working_quote_tests::two_stock_quote_setup();
    setup.npcs.inst_count = 2;
    let mut shadow = GameSession::new(setup, 41).unwrap();
    let observed = AccountId(1);
    let unobserved = AccountId(2);
    let player = AccountId(0);
    let codes = shadow.markets.keys().cloned().collect::<Vec<_>>();
    let tick = shadow.tick;
    shadow.attention_queue.clear();
    npc_working_quote_tests::force_attention_candidate(&mut shadow, observed, tick);
    shadow
        .npc_attention
        .get_mut(&unobserved)
        .unwrap()
        .next_attention_candidate_tick = tick + 1;
    shadow
        .attention_queue
        .push(std::cmp::Reverse((tick + 1, unobserved)));
    for (code, id, owner) in [
        (&codes[0], 9, observed),
        (&codes[0], 3, observed),
        (&codes[0], 4, unobserved),
        (&codes[0], 5, player),
        (&codes[1], 11, observed),
    ] {
        shadow
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
        shadow.auction_orders.entry(code.clone()).or_default().push(
            crate::session::AuctionOrderSnap {
                owner,
                side: crate::Side::Buy,
                limit: Money::from_cents(900),
                qty: 100,
                order_id: id,
            },
        );
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
    let mut entries: Vec<_> = session
        .attention_queue
        .iter()
        .map(|entry| entry.0)
        .collect();
    entries.sort_unstable();
    entries
}

fn due_retail_shadow(seed: u64) -> (GameSession, GameSession, AccountId) {
    let account = AccountId(1);
    let source = GameSession::new(npc_working_quote_tests::retail_quote_setup(), seed).unwrap();
    let mut shadow = source.clone_for_tick_shadow().unwrap();
    let tick = shadow.tick;
    npc_working_quote_tests::force_attention_candidate(&mut shadow, account, tick);
    (source, shadow, account)
}

#[test]
fn capture_advances_attention_and_seals_owned_views_on_shadow_only() {
    let (source, mut shadow, account) = due_retail_shadow(0xC0FFEE);
    let source_before = source.session_state_hash().unwrap();
    let attention_before = shadow.npc_attention[&account].clone();
    let mut expected_shadow = shadow.clone_for_tick_shadow().unwrap();

    let market = expected_shadow.build_market_view();
    let (due, _) = expected_shadow.pop_due_npc_ids(expected_shadow.tick);
    let mut accepted = Vec::new();
    for id in due {
        let observes = expected_shadow.evaluate_attention_candidate(id, &market);
        let next = expected_shadow.npc_attention[&id].next_attention_candidate_tick;
        expected_shadow
            .attention_queue
            .push(std::cmp::Reverse((next, id)));
        if observes {
            accepted.push(id);
        }
    }
    expected_shadow
        .observe_retail_experience(&accepted)
        .unwrap();
    let (continuous, auction) = expected_shadow.working_orders_by_account();
    let expected_self = expected_shadow
        .build_self_views_for(&accepted, expected_shadow.phase(), &continuous, &auction)
        .remove(&account)
        .unwrap();
    let expected_behavior = expected_shadow.behavior_market_observation();
    let expected_risk = expected_shadow
        .account_risk_observations_for(&accepted)
        .remove(&account)
        .unwrap();
    let expected_experience = expected_shadow.retail_experience[&account].clone();

    let snapshot = capture_decision_snapshot(&mut shadow).unwrap();
    let snapshot = &snapshot.snapshot;

    assert_eq!(snapshot.due_npc_ids(), accepted);
    assert_eq!(snapshot.npc_seed_base(), 0xC0FFEE);
    assert_eq!(snapshot.tick(), shadow.tick);
    assert_eq!(snapshot.phase(), shadow.phase());
    assert_eq!(
        serde_json::to_vec(snapshot.market()).unwrap(),
        serde_json::to_vec(&market).unwrap()
    );
    assert_eq!(snapshot.behavior_market(), Some(&expected_behavior));
    let input = snapshot.account(account).unwrap();
    assert_eq!(input.kind(), AccountKind::Retail);
    assert_eq!(
        serde_json::to_vec(input.self_view()).unwrap(),
        serde_json::to_vec(&expected_self).unwrap()
    );
    assert_eq!(input.account_risk(), Some(&expected_risk));
    assert_eq!(input.retail_experience(), Some(&expected_experience));
    assert_ne!(shadow.npc_attention[&account], attention_before);
    assert!(shadow.npc_attention[&account].next_attention_candidate_tick > shadow.tick);
    assert_eq!(shadow.npc_attention, expected_shadow.npc_attention);
    assert_eq!(queue_entries(&shadow), queue_entries(&expected_shadow));
    assert_eq!(shadow.retail_experience, expected_shadow.retail_experience);
    assert_eq!(source.session_state_hash().unwrap(), source_before);
}

#[test]
fn capture_preserves_no_due_fast_path_without_fabricating_retail_observations() {
    let mut shadow = GameSession::new(npc_working_quote_tests::quote_setup(0), 17).unwrap();
    shadow.attention_queue.clear();
    let attention_before = shadow.npc_attention.clone();
    let experience_before = shadow.retail_experience.clone();

    let captured = capture_decision_snapshot(&mut shadow).unwrap();
    assert!(captured.working_continuous.is_empty());
    assert!(captured.working_auction.is_empty());
    let snapshot = &captured.snapshot;

    assert!(snapshot.due_npc_ids().is_empty());
    assert!(super::npc_p2_source::run_npc_p2_source(snapshot.clone())
        .unwrap()
        .intents()
        .is_empty());
    assert!(snapshot.behavior_market().is_none());
    assert_eq!(shadow.npc_attention, attention_before);
    assert_eq!(shadow.retail_experience, experience_before);
}

#[test]
fn capture_reports_retail_experience_overflow() {
    let (_, mut shadow, account) = due_retail_shadow(23);
    let code = shadow
        .markets
        .keys()
        .next()
        .cloned()
        .expect("retail fixture has a market");
    let last = shadow.markets[&code].last_price();
    shadow
        .accounts
        .get_mut(&account)
        .unwrap()
        .grant_position(code.clone(), 100, last)
        .unwrap();
    let market_minute = shadow.current_market_minute();
    let experience = shadow.retail_experience.get_mut(&account).unwrap();
    experience
        .initialize_holding(&code, Some(last), last, market_minute)
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
    shadow.accounts.get_mut(&account).unwrap().strategy = None;
    let error = capture_decision_snapshot(&mut shadow).unwrap_err();

    assert!(matches!(
        error,
        DecisionSnapshotCaptureError::MissingStrategy(failed) if failed == account
    ));
}

#[test]
fn capture_reports_missing_due_attention_without_panic() {
    let (_, mut shadow, account) = due_retail_shadow(31);
    shadow.npc_attention.remove(&account);
    let error = capture_decision_snapshot(&mut shadow).unwrap_err();

    assert!(matches!(
        error,
        DecisionSnapshotCaptureError::MissingAttention(failed) if failed == account
    ));
}

#[test]
fn capture_reports_risk_view_failure() {
    let (_, mut shadow, account) = due_retail_shadow(37);
    shadow.accounts.get_mut(&account).unwrap().cash = Money::from_cents(-1);
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
    shadow.tick = 1;
    let first = AccountId(1);
    let second = AccountId(2);
    npc_working_quote_tests::force_attention_candidate(&mut shadow, first, 1);
    npc_working_quote_tests::force_attention_candidate(&mut shadow, second, 1);
    shadow.attention_queue.push(std::cmp::Reverse((0, first)));
    shadow
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
    let tick = source.tick;
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
