use crate::session::{RetailOrderDiagnosticEvent, StepFatal};
use crate::strategy::ZiNoiseStrategy;
use crate::{
    AccountId, Event, GameSession, Intent, Money, OrderId, RejectionReason, Side, TradingPhase,
};

fn empty_session_at(tick: u64, closing_auction_ticks: u64) -> GameSession {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(900);
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.closing_auction_ticks = closing_auction_ticks;
    let mut session = GameSession::new(setup, 42).unwrap();
    session.tick = tick;
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
    session.tick = tick;
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
            session.save().unwrap().runtime_v2.next_receipt_base
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
                    super::b1_continuous_transaction::prepare_b1_continuous_tick(&mut session)
                        .unwrap(),
                );
            }
            TradingPhase::CallAuction | TradingPhase::ClosingAuction => {
                drop(super::b2_auction_transaction::prepare_b2_auction_tick(&mut session).unwrap());
            }
            TradingPhase::PreOpen => {
                drop(super::pre_open_transaction::prepare_pre_open_tick(&mut session).unwrap());
            }
        }
        assert_eq!(session.business_state_hash().unwrap(), business_before);
        assert_eq!(session.session_state_hash().unwrap(), session_before);
        assert_eq!(session.tick(), tick);
    }
}

#[test]
fn two_npc_decisions_and_player_order_keep_business_order_through_the_current_tick() {
    let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
    setup.npcs.retail_count = 2;
    let code = setup.stocks[0].code.clone();
    let mut session = GameSession::new(setup, 8).unwrap();
    let first = AccountId(1);
    let second = AccountId(2);
    let player = AccountId(0);
    for account in [first, second] {
        session.accounts.get_mut(&account).unwrap().cash = Money::from_cents(1_000_000);
        session
            .accounts
            .get_mut(&account)
            .unwrap()
            .set_strategy(Box::new(ZiNoiseStrategy::new(1.0, 9_000, 0.5, 1).unwrap()));
        crate::session::npc_working_quote_tests::force_attention_candidate(
            &mut session,
            account,
            0,
        );
    }
    session
        .enqueue_player_intent(
            player,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(900),
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
    // P7 publishes events by entity; order IDs record the canonical admission order.
    accepted.sort_by_key(|(_, id, _)| *id);
    assert_eq!(
        accepted,
        [
            (first, OrderId(1), 200),
            (second, OrderId(2), 200),
            (player, OrderId(3), 100)
        ]
    );
    assert_eq!(session.markets[&code].resting_orders_for(first)[0].qty, 200);
    assert_eq!(
        session.markets[&code].resting_orders_for(second)[0].qty,
        200
    );
    assert_eq!(
        session.markets[&code].resting_orders_for(player)[0].qty,
        100
    );
    assert!(session.pending_player.is_empty());
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
            session.step().unwrap();
        }

        assert_eq!(session.tick(), tick_after);
        assert_eq!(session.phase(), expected_phase);
        assert_eq!(session.day(), expected_day);

        let save = session.save().unwrap_or_else(|error| {
            panic!("tick {tick_after} P9 must leave a legal save quiet point: {error}")
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
fn public_step_discards_each_phase_candidate_when_p9_preparation_fails() {
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
    let code = session.markets.keys().next().unwrap().clone();
    session.retail_experience.insert(
        AccountId(0),
        crate::experience::RetailExperienceState::without_equity_reference(),
    );

    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(900),
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
                price: Money::from_cents(900),
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
    assert_eq!(session.markets[&code].resting_order_count(), 1);

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
    assert_eq!(session.markets[&code].resting_order_count(), 0);
}

#[test]
fn public_step_reports_zero_quantity_in_event_and_retail_diagnostic() {
    let mut session = empty_session_at(900, 10);
    let code = session.markets.keys().next().unwrap().clone();
    session.retail_experience.insert(
        AccountId(0),
        crate::experience::RetailExperienceState::without_equity_reference(),
    );
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(900),
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
    assert_eq!(session.markets[&code].resting_order_count(), 0);
}
