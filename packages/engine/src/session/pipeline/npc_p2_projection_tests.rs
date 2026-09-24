use super::decision_snapshot_capture::capture_decision_snapshot;
use super::npc_p2_projection::{
    project_npc_p2, remove_first_matching_raw, take_exact_raw_source, NpcP2ProjectionError,
    NpcReconciliationDecision,
};
use super::npc_p2_source::run_npc_p2_source;
use super::*;
use crate::session::npc_working_quote_tests;
use crate::strategy::ZiNoiseStrategy;
use crate::{AccountId, Money};

fn due_retail(seed: u64) -> (GameSession, AccountId) {
    let account = AccountId(1);
    let mut shadow = GameSession::new(npc_working_quote_tests::retail_quote_setup(), seed).unwrap();
    shadow
        .accounts
        .get_mut(&account)
        .unwrap()
        .set_strategy(Box::new(ZiNoiseStrategy::new(1.0, 100, 0.5, 1).unwrap()));
    let tick = shadow.tick;
    npc_working_quote_tests::force_attention_candidate(&mut shadow, account, tick);
    (shadow, account)
}

fn sealed_resources(session: &GameSession) -> DecisionResourceSnapshot {
    plan_tick(PhaseInput { session })
        .unwrap()
        .decision_resources()
        .unwrap()
        .clone()
}

#[test]
fn exact_keep_consumes_the_first_duplicate_raw_key_before_survivorship_mapping() {
    let account = AccountId(1);
    let intent = crate::Intent::PlaceLimit {
        code: crate::StockCode("600888".to_owned()),
        side: crate::Side::Buy,
        price: Money::from_cents(900),
        qty: 100,
    };
    let mut raw = vec![
        (P2CandidateKey::npc(account, 0), intent.clone()),
        (P2CandidateKey::npc(account, 1), intent.clone()),
    ];

    remove_first_matching_raw(&mut raw, &intent);

    assert_eq!(raw.len(), 1);
    assert_eq!(raw[0].0, P2CandidateKey::npc(account, 1));
}

#[test]
fn rewritten_parent_child_shape_cannot_borrow_a_raw_key_by_stock_and_side() {
    let account = AccountId(1);
    let code = crate::StockCode("600888".to_owned());
    let raw_intent = crate::Intent::PlaceLimit {
        code: code.clone(),
        side: crate::Side::Buy,
        price: Money::from_cents(1_000),
        qty: 400,
    };
    let child = crate::Intent::PlaceLimit {
        code,
        side: crate::Side::Buy,
        price: Money::from_cents(1_000),
        qty: 100,
    };
    let mut raw = vec![(P2CandidateKey::npc(account, 0), raw_intent)];

    assert!(take_exact_raw_source(&mut raw, &child).is_none());
    assert_eq!(raw.len(), 1);
}

#[test]
fn projection_applies_nonempty_source_without_routing_and_preserves_raw_identity() {
    let (mut shadow, account) = due_retail(8);
    let snapshot = capture_decision_snapshot(&mut shadow).unwrap();
    let source = run_npc_p2_source(snapshot.clone()).unwrap();
    assert_eq!(source.accounts(), &[account]);
    assert!(
        !source.intents().is_empty(),
        "fixture must emit NPC intents"
    );
    let expected_strategy = source.strategy_state(account).unwrap().clone();
    let raw_keys: Vec<_> = source
        .intents()
        .iter()
        .map(|intent| intent.key().clone())
        .collect();
    let market_before = shadow
        .markets
        .iter()
        .map(|(code, market)| {
            (
                code.clone(),
                serde_json::to_vec(&market.hash_projection()).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let parent_before = shadow.parent_orders.clone();
    let resources = sealed_resources(&shadow);
    let output = project_npc_p2(&mut shadow, &snapshot, &source, &resources).unwrap();

    assert_eq!(output.accepted_due_npc_ids(), &[account]);
    assert_eq!(output.reconciliation_decisions().len(), 0);
    assert_eq!(
        output
            .residual_intents()
            .iter()
            .filter_map(|intent| intent.source_key().cloned())
            .collect::<Vec<_>>(),
        raw_keys
    );
    assert_eq!(output.residual_intents().len(), source.intents().len());
    for (projected, raw) in output.residual_intents().iter().zip(source.intents()) {
        assert_eq!(projected.account(), account);
        assert_eq!(projected.source_key(), Some(raw.key()));
        assert_eq!(
            serde_json::to_value(projected.projected_intent()).unwrap(),
            serde_json::to_value(raw.intent()).unwrap()
        );
    }
    assert_eq!(
        shadow.accounts[&account]
            .strategy
            .as_ref()
            .unwrap()
            .production_state()
            .unwrap(),
        expected_strategy
    );
    assert_eq!(shadow.parent_orders, parent_before);
    assert_eq!(
        shadow
            .markets
            .iter()
            .map(|(code, market)| {
                (
                    code.clone(),
                    serde_json::to_vec(&market.hash_projection()).unwrap(),
                )
            })
            .collect::<Vec<_>>(),
        market_before
    );
}

#[test]
fn projection_failure_is_typed_and_preserves_the_entire_shadow() {
    let (mut shadow, _) = due_retail(9);
    let snapshot = capture_decision_snapshot(&mut shadow).unwrap();
    let source = run_npc_p2_source(snapshot.clone()).unwrap();
    let resources = sealed_resources(&shadow);
    shadow.tick = shadow.tick.checked_add(1).unwrap();
    let before = shadow.session_state_hash().unwrap();

    let error = project_npc_p2(&mut shadow, &snapshot, &source, &resources).unwrap_err();

    assert!(matches!(error, NpcP2ProjectionError::ClockMismatch { .. }));
    assert_eq!(shadow.session_state_hash().unwrap(), before);
}

#[test]
fn projection_late_failure_discards_strategy_and_diagnostic_changes_atomically() {
    let (mut shadow, account) = due_retail(10);
    let snapshot = capture_decision_snapshot(&mut shadow).unwrap();
    let source = run_npc_p2_source(snapshot.clone()).unwrap();
    let resources = sealed_resources(&shadow);
    shadow.retail_experience.remove(&account);
    let strategy_before = shadow.accounts[&account]
        .strategy
        .as_ref()
        .unwrap()
        .production_state()
        .unwrap();
    let diagnostics_before = shadow.last_retail_decisions.clone();
    let parent_before = shadow.parent_orders.clone();

    let error = project_npc_p2(&mut shadow, &snapshot, &source, &resources).unwrap_err();

    assert!(matches!(
        error,
        NpcP2ProjectionError::MissingRetailExperience(failed) if failed == account
    ));
    assert_eq!(
        shadow.accounts[&account]
            .strategy
            .as_ref()
            .unwrap()
            .production_state()
            .unwrap(),
        strategy_before
    );
    assert_eq!(shadow.last_retail_decisions, diagnostics_before);
    assert_eq!(shadow.parent_orders, parent_before);
    assert!(!shadow.retail_experience.contains_key(&account));
}

#[test]
fn later_npc_failure_restores_every_earlier_account_patch() {
    let mut setup = npc_working_quote_tests::retail_quote_setup();
    setup.npcs.retail_count = 2;
    let mut shadow = GameSession::new(setup, 10).unwrap();
    for account in [AccountId(1), AccountId(2)] {
        shadow
            .accounts
            .get_mut(&account)
            .unwrap()
            .set_strategy(Box::new(ZiNoiseStrategy::new(1.0, 100, 0.5, 1).unwrap()));
        let tick = shadow.tick;
        npc_working_quote_tests::force_attention_candidate(&mut shadow, account, tick);
    }
    let snapshot = capture_decision_snapshot(&mut shadow).unwrap();
    assert_eq!(snapshot.due_npc_ids(), &[AccountId(1), AccountId(2)]);
    let source = run_npc_p2_source(snapshot.clone()).unwrap();
    let resources = sealed_resources(&shadow);
    shadow.retail_experience.remove(&AccountId(2));
    let before = shadow.session_state_hash().unwrap();

    let error = project_npc_p2(&mut shadow, &snapshot, &source, &resources).unwrap_err();

    assert!(matches!(
        error,
        NpcP2ProjectionError::MissingRetailExperience(AccountId(2))
    ));
    assert_eq!(shadow.session_state_hash().unwrap(), before);
    assert!(!shadow.retail_experience.contains_key(&AccountId(2)));
}

#[test]
fn projection_cash_cap_reports_resize_without_rekeying_or_routing() {
    let (mut shadow, account) = due_retail(33);
    shadow
        .accounts
        .get_mut(&account)
        .unwrap()
        .set_strategy(Box::new(ZiNoiseStrategy::new(1.0, 900, 0.5, 1).unwrap()));
    let snapshot = capture_decision_snapshot(&mut shadow).unwrap();
    let source = run_npc_p2_source(snapshot.clone()).unwrap();
    let raw = source
        .intents()
        .first()
        .expect("arrival rate one emits an intent");
    // The source sees the high-cash decision view. P1 is then sealed at the
    // lower balance; a later shadow mutation must not change that cap input.
    shadow.accounts.get_mut(&account).unwrap().cash = Money::from_cents(500_000);
    let resources = sealed_resources(&shadow);
    shadow.accounts.get_mut(&account).unwrap().cash = Money::from_cents(10_000_000);
    let books_before = serde_json::to_vec(&shadow.snapshot()).unwrap();

    let output = project_npc_p2(&mut shadow, &snapshot, &source, &resources).unwrap();

    let projected = output
        .residual_intents()
        .first()
        .expect("affordable board lot survives");
    assert_eq!(projected.account(), account);
    assert_eq!(projected.source_key(), Some(raw.key()));
    assert_eq!(
        serde_json::to_vec(projected.source_intent().unwrap()).unwrap(),
        serde_json::to_vec(raw.intent()).unwrap()
    );
    assert!(matches!(
        projected.projected_intent(),
        crate::Intent::PlaceLimit { qty, .. } if qty <= &400
    ));
    assert!(output.cash_cap_changes().iter().any(|change| {
        let (owner, requested, capped, dropped) = change.contract_parts();
        owner == account
            && requested == Some(900)
            && capped.is_some_and(|qty| qty <= 400)
            && dropped.is_none()
            && change.source_key() == Some(raw.key())
            && change.is_resized()
    }));
    let books_after = serde_json::to_vec(&shadow.snapshot()).unwrap();
    assert_eq!(
        books_after, books_before,
        "P2 projection must not route or settle"
    );
}

#[test]
fn projection_exposes_working_decisions_before_residuals_without_canceling_the_book() {
    let (mut shadow, account) = due_retail(41);
    let code = shadow.setup.stocks[0].code.clone();
    let mut setup_events = Vec::new();
    shadow.route_intent(
        account,
        crate::Intent::PlaceLimit {
            code: code.clone(),
            side: crate::Side::Buy,
            price: Money::from_cents(900),
            qty: 1_000,
        },
        &mut setup_events,
    );
    let resting_id = shadow.markets[&code]
        .resting_orders_for(account)
        .first()
        .expect("setup order is inside the daily price limit")
        .id;
    let reserved = crate::session::buy_order_reservation(
        &shadow.setup.config,
        Money::from_cents(900),
        1_000,
        Money::ZERO,
    )
    .unwrap();
    shadow.accounts.get_mut(&account).unwrap().cash = reserved;
    let resources = sealed_resources(&shadow);
    let snapshot = capture_decision_snapshot(&mut shadow).unwrap();
    let source = run_npc_p2_source(snapshot.clone()).unwrap();
    let raw_key = source
        .intents()
        .first()
        .expect("arrival rate one emits a replacement candidate")
        .key()
        .clone();
    let book_before: Vec<_> = shadow.markets[&code]
        .resting_orders_for(account)
        .into_iter()
        .map(|order| (order.id, order.side, order.price, order.qty))
        .collect();

    let output = project_npc_p2(&mut shadow, &snapshot, &source, &resources).unwrap();

    assert!(matches!(
        output.reconciliation_decisions().first(),
        Some(
            NpcReconciliationDecision::Cancel { order_id, .. }
                | NpcReconciliationDecision::Replace {
                    old_order_id: order_id,
                    ..
                }
                | NpcReconciliationDecision::Keep { order_id, .. }
        ) if *order_id == resting_id
    ));
    let (decision_owner, decision_order, decision_code, replacement) = output
        .reconciliation_decisions()
        .first()
        .unwrap()
        .contract_parts();
    assert_eq!((decision_owner, decision_order), (account, resting_id));
    assert!(
        decision_code.is_some() || replacement.is_some() || output.residual_intents().is_empty()
    );
    assert!(
        output.cash_cap_changes().iter().any(|change| {
            change.source_key() == Some(&raw_key)
                && matches!(
                    change.contract_parts(),
                    (owner, None, None, Some(_)) if owner == account
                )
        }),
        "ADR-0017 #2 keeps the replaced order reservation sealed for this batch"
    );
    assert!(output
        .residual_intents()
        .iter()
        .all(|intent| intent.source_key() != Some(&raw_key)));
    assert_eq!(
        shadow.markets[&code]
            .resting_orders_for(account)
            .into_iter()
            .map(|order| (order.id, order.side, order.price, order.qty))
            .collect::<Vec<_>>(),
        book_before,
        "structured reconciliation must precede residual routing, not execute it"
    );
}

#[test]
fn production_source_does_not_fabricate_the_currently_unreachable_parent_capability() {
    let account = AccountId(1);
    let mut shadow = GameSession::new(npc_working_quote_tests::quote_setup(0), 991).unwrap();
    let code = shadow.setup.stocks[0].code.clone();
    let existing = crate::session::ParentOrderPlan {
        code: code.clone(),
        side: crate::Side::Buy,
        target_qty: 400,
        filled_qty: 0,
        child_qty: 100,
        active_child_order_id: None,
        active_child_remaining_qty: None,
        linked_plan_id: None,
        limit_price: Money::from_cents(1_000),
        expires_market_minute: 100,
    };
    shadow
        .parent_orders
        .entry(account)
        .or_default()
        .insert(code.clone(), existing.clone());
    let tick = shadow.tick;
    npc_working_quote_tests::force_attention_candidate(&mut shadow, account, tick);
    let snapshot = capture_decision_snapshot(&mut shadow).unwrap();
    let source = run_npc_p2_source(snapshot.clone()).unwrap();
    assert!(!source.account_outputs()[0].uses_parent_order_execution());

    let resources = sealed_resources(&shadow);
    let output = project_npc_p2(&mut shadow, &snapshot, &source, &resources).unwrap();

    assert!(output
        .residual_intents()
        .iter()
        .all(|intent| intent.source_key().is_some()));
    assert_eq!(shadow.parent_orders[&account][&code], existing);
    assert!(shadow.markets[&code].resting_orders_for(account).is_empty());
}

#[test]
fn retail_review_reconciles_only_the_stock_actually_observed() {
    let account = AccountId(1);
    let mut setup = npc_working_quote_tests::two_stock_quote_setup();
    setup.npcs.retail_count = 1;
    setup.npcs.inst_count = 0;
    let mut shadow = GameSession::new(setup, 43).unwrap();
    shadow
        .accounts
        .get_mut(&account)
        .unwrap()
        .set_strategy(Box::new(ZiNoiseStrategy::new(0.0, 100, 0.5, 1).unwrap()));
    let codes = shadow.markets.keys().cloned().collect::<Vec<_>>();
    for code in &codes {
        shadow.route_intent(
            account,
            crate::Intent::PlaceLimit {
                code: code.clone(),
                side: crate::Side::Buy,
                price: Money::from_cents(900),
                qty: 100,
            },
            &mut Vec::new(),
        );
    }
    let resting = codes
        .iter()
        .map(|code| {
            (
                code.clone(),
                shadow.markets[code].resting_orders_for(account)[0].id,
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    npc_working_quote_tests::force_attention_candidate(&mut shadow, account, 0);
    let resources = sealed_resources(&shadow);
    let snapshot = capture_decision_snapshot(&mut shadow).unwrap();
    let source = run_npc_p2_source(snapshot.clone()).unwrap();
    let reviewed = source.account_outputs()[0].reviewed_stocks();
    assert_eq!(reviewed.len(), 1);
    let observed = reviewed.iter().next().unwrap();

    let projected = project_npc_p2(&mut shadow, &snapshot, &source, &resources).unwrap();

    assert!(matches!(
        projected.reconciliation_decisions(),
        [NpcReconciliationDecision::Cancel { account: owner, code, order_id }]
            if *owner == account && code == observed && *order_id == resting[observed]
    ));
    assert!(projected.residual_intents().is_empty());
    assert_eq!(codes.len(), 2);
    assert_eq!(
        shadow.markets[observed].resting_orders_for(account).len(),
        1,
        "P2 only proposes the cancellation; P4 still owns the book mutation"
    );
}
