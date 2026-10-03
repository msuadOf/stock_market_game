use super::*;
use crate::{AccountId, Side};

fn session_at(tick: u64, closing: bool) -> GameSession {
    let mut setup =
        crate::session::npc_working_quote_tests::quote_setup(if closing { 0 } else { 900 });
    if closing {
        setup.closing_auction_ticks = 10;
    }
    let mut session = GameSession::new(setup, 42).unwrap();
    session.state.tick = tick;
    session
}

#[test]
fn checked_auction_boundary_distinguishes_opening_rollover_and_closing_day_end() {
    for (session, tick_after, completion, day_end) in [
        (session_at(0, false), 1, false, false),
        (session_at(599, false), 600, true, false),
        (session_at(90, true), 91, false, false),
        (session_at(99, true), 100, true, true),
    ] {
        let boundary = AuctionTickBoundary::capture(&session).unwrap();
        assert_eq!(boundary.tick_after(), tick_after);
        assert_eq!(boundary.finish_auction(), completion);
        assert_eq!(boundary.finish_day(), day_end);
    }
}

#[test]
fn checked_auction_boundary_preserves_phase_tick_and_day_first_errors() {
    let mut continuous = session_at(90, true);
    continuous.state.setup.closing_auction_ticks = 0;
    assert!(
        matches!(AuctionTickBoundary::capture(&continuous), Err(AuctionDayEndError::Precondition(StepFatal::InvariantViolation { description, .. })) if description == "Auction/DayEnd tick 的 Auction 事务要求 OpeningAuction 或 ClosingAuction 阶段")
    );
    let mut overflow = session_at(0, false);
    overflow.state.tick = u64::MAX;
    overflow.state.setup.ticks_per_day = 1;
    overflow.state.setup.auction_ticks = 1;
    assert!(
        matches!(AuctionTickBoundary::capture(&overflow), Err(AuctionDayEndError::Precondition(StepFatal::InvariantViolation { description, .. })) if description == "tick overflow")
    );
    let mut day_overflow = session_at(99, true);
    day_overflow.state.day = u32::MAX;
    assert!(
        matches!(AuctionTickBoundary::capture(&day_overflow), Err(AuctionDayEndError::Precondition(StepFatal::InvariantViolation { description, .. })) if description == "auction trading day overflow")
    );
}

#[test]
fn day_end_transition_rejects_each_live_order_family_before_mutating_candidate() {
    for family in 0..3 {
        let mut session = session_at(99, true);
        let code = session.state.markets.keys().next().unwrap().clone();
        let order = crate::Order {
            id: OrderId(10),
            side: Side::Buy,
            price: Money::from_cents(900),
            qty: 100,
            original_qty: 100,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: AccountId(0),
            seq: 0,
        };
        if family == 0 || family == 2 {
            session
                .state
                .markets
                .get_mut(&code)
                .unwrap()
                .place(order)
                .unwrap();
            if family == 2 {
                session.hydrate_or_validate_envelope_ledger().unwrap();
                session.state.markets.get_mut(&code).unwrap().end_of_day();
            }
        } else {
            session.state.auction_orders.insert(
                code,
                vec![AuctionOrderSnap {
                    owner: AccountId(0),
                    side: Side::Buy,
                    limit: Money::from_cents(900),
                    qty: 100,
                    order_id: 10,
                }],
            );
        }
        let before = session.business_state_hash().unwrap();
        assert!(
            matches!(TradingDayEndTransition::new(&mut session).apply(), Err(AuctionDayEndError::Precondition(StepFatal::InvariantViolation { description, .. })) if description == "DayEnd finalizer retained a live order or envelope")
        );
        assert_eq!(session.business_state_hash().unwrap(), before);
    }
}

#[test]
fn day_end_transition_overflow_keeps_original_candidate_partial_progress() {
    let mut session = session_at(99, true);
    session.state.day = u32::MAX;
    session.state.setup.t1_enabled = true;
    let code = session.state.markets.keys().next().unwrap().clone();
    let account = session.state.accounts.get_mut(&AccountId(0)).unwrap();
    account
        .grant_position(code.clone(), 100, Money::from_cents(900))
        .unwrap();
    account.fixture_insert_position(
        code.clone(),
        crate::Position::from_restored_parts(100, 100, 90_000, 0),
    );
    session.update_active_daily_candle(&code, Money::from_cents(950), 100);
    let candles_before = session.state.candle_book.histories()[&code].len();
    let result = TradingDayEndTransition::new(&mut session).apply();
    assert!(
        matches!(result, Err(AuctionDayEndError::Precondition(StepFatal::InvariantViolation { description, .. })) if description == "trading day overflow")
    );
    assert_eq!(session.state.day, u32::MAX);
    assert_eq!(
        session.state.accounts[&AccountId(0)].positions()[&code].t1_locked(),
        0
    );
    assert!(session.state.candle_book.active().is_empty());
    assert_eq!(
        session.state.candle_book.histories()[&code].len(),
        candles_before + 1
    );
}

#[test]
fn auction_lifecycle_duplicate_identity_does_not_commit_projection_buffers() {
    let mut session = session_at(0, false);
    let code = session.state.markets.keys().next().unwrap().clone();
    let fact = AuctionLifecycleFact::Rejected {
        candidate_key: IntentCandidateKey::player(0),
        sealed_index: 0,
        account: AccountId(0),
        code,
        order_id: None,
        reason: RejectionReason::OrderNotFound,
    };
    let before = session.business_state_hash().unwrap();
    let result = AuctionLifecycleProjector::new(&session).apply(
        &mut session,
        std::iter::empty(),
        &[fact.clone(), fact],
        &[],
        false,
        None,
    );
    assert!(
        matches!(result, Err(StepFatal::InvariantViolation { description, .. }) if description == "auction lifecycle facts contain duplicate sealed identity")
    );
    assert_eq!(session.business_state_hash().unwrap(), before);
    assert!(session.state.pending_plan_events.is_empty());
    assert!(session.state.last_retail_order_events.is_empty());
}

#[test]
fn auction_lifecycle_rejects_zero_and_regressing_fill_before_committing_buffers() {
    for (qty_after, description) in [
        (100, "auction lifecycle received a zero-quantity fill"),
        (101, "auction fill receipt has a regressing quantity chain"),
    ] {
        let mut session = session_at(0, false);
        let code = session.state.markets.keys().next().unwrap().clone();
        let order = AuctionOrder {
            envelope: Envelope::created_at_validation(
                EnvelopeKey {
                    account: AccountId(0),
                    stock: code,
                    order: OrderId(10),
                    side: Side::Sell,
                },
                Money::ZERO,
                100,
                crate::session::pipeline::EnvelopeAudit {
                    limit: Money::from_cents(900),
                    remaining_qty: 100,
                    filled_qty: 0,
                    filled_value: Money::ZERO,
                    nominal: crate::session::pipeline::FeeComponents::ZERO,
                    charged: crate::session::pipeline::FeeComponents::ZERO,
                },
            ),
            arrival_seq: 0,
        };
        let mut receipt = reject_receipt(&order, 0).unwrap();
        receipt.kind = ReceiptKind::Fill;
        receipt.qty_after = qty_after;
        let before = session.business_state_hash().unwrap();
        let error = AuctionLifecycleProjector::new(&session)
            .apply(
                &mut session,
                std::iter::empty(),
                &[],
                &[receipt],
                false,
                None,
            )
            .unwrap_err();
        assert_eq!(error, lifecycle_invariant(description));
        assert_eq!(session.business_state_hash().unwrap(), before);
    }
}

#[test]
fn auction_lifecycle_child_quantity_mismatch_discards_pending_parent_changes() {
    let mut session = session_at(0, false);
    let code = session.state.markets.keys().next().unwrap().clone();
    session
        .state
        .parent_orders
        .entry(AccountId(0))
        .or_default()
        .insert(
            code.clone(),
            crate::session::ParentOrderPlan::from_facts(
                code.clone(),
                Side::Buy,
                100,
                0,
                100,
                None,
                Some(crate::PlanId(1)),
                Money::from_cents(900),
                240,
            ),
        );
    let facts = [
        AuctionLifecycleFact::Accepted {
            candidate_key: IntentCandidateKey::player(0),
            sealed_index: 0,
            account: AccountId(0),
            code: code.clone(),
            order_id: OrderId(10),
            side: Side::Buy,
            qty: 100,
        },
        AuctionLifecycleFact::Canceled {
            candidate_key: IntentCandidateKey::player(1),
            sealed_index: 1,
            account: AccountId(0),
            code: code.clone(),
            order_id: OrderId(10),
            remaining_qty: 50,
        },
    ];
    let parents_before = session.state.parent_orders.clone();
    let result = AuctionLifecycleProjector::new(&session).apply(
        &mut session,
        std::iter::empty(),
        &facts,
        &[],
        false,
        None,
    );
    assert_eq!(
        result.unwrap_err(),
        lifecycle_invariant("auction cancellation disagrees with linked parent child quantity")
    );
    assert_eq!(session.state.parent_orders, parents_before);
    assert!(session.state.pending_plan_events.is_empty());
    assert!(session.state.last_retail_order_events.is_empty());
}

#[test]
fn day_end_transition_is_phase_independent_and_publishes_one_closed_candle_boundary() {
    let mut closing = session_at(99, true);
    let mut continuous = session_at(90, true);
    continuous.state.setup.closing_auction_ticks = 0;
    assert_eq!(closing.phase(), TradingPhase::ClosingAuction);
    assert_eq!(continuous.phase(), TradingPhase::Continuous);
    for session in [&mut closing, &mut continuous] {
        session.state.day = 3;
        let code = session.state.markets.keys().next().unwrap().clone();
        session.update_active_daily_candle(&code, Money::from_cents(950), 100);
    }
    let closing_facts = TradingDayEndTransition::new(&mut closing).apply().unwrap();
    let continuous_facts = TradingDayEndTransition::new(&mut continuous)
        .apply()
        .unwrap();
    assert_eq!(closing_facts.len(), 1);
    assert_eq!(continuous_facts.len(), 1);
    assert_eq!(
        serde_json::to_value(&closing_facts[0].event).unwrap(),
        serde_json::to_value(&continuous_facts[0].event).unwrap()
    );
    assert_eq!(closing.state.day, 4);
    assert_eq!(continuous.state.day, 4);
    assert!(closing.state.candle_book.active().is_empty());
    assert!(continuous.state.candle_book.active().is_empty());
}

#[test]
fn day_end_transition_plan_failure_precedes_day_overflow_and_leaves_authority_untouched() {
    let mut authority = session_at(99, true);
    authority.state.day = u32::MAX;
    authority.state.setup.t1_enabled = true;
    let code = authority.state.markets.keys().next().unwrap().clone();
    authority
        .state
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .fixture_insert_position(
            code.clone(),
            crate::Position::from_restored_parts(100, 100, 90_000, 0),
        );
    authority.update_active_daily_candle(&code, Money::from_cents(950), 100);
    authority
        .state
        .pending_plan_events
        .push(PendingPlanEvent::Accepted {
            plan_id: crate::PlanId(777),
            order_id: OrderId(10),
            trading_day: u64::from(u32::MAX),
        });
    let before = authority.business_state_hash().unwrap();
    let mut candidate = authority.clone_for_tick_shadow().unwrap();
    let error = TradingDayEndTransition::new(&mut candidate)
        .apply()
        .unwrap_err();
    assert!(
        matches!(error, AuctionDayEndError::Lifecycle(StepFatal::InvariantViolation { description, .. }) if description.starts_with("plan synchronization failed:"))
    );
    assert_eq!(candidate.state.day, u32::MAX);
    assert_eq!(
        candidate.state.accounts[&AccountId(0)].positions()[&code].t1_locked(),
        0
    );
    assert!(!candidate.state.candle_book.active().is_empty());
    assert_eq!(authority.business_state_hash().unwrap(), before);
    assert_eq!(
        authority.state.accounts[&AccountId(0)].positions()[&code].t1_locked(),
        100
    );
}
