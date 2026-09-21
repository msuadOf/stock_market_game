use super::*;
use crate::session::{RetailOrderDiagnosticEvent, StepFatal};
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
fn public_step_dispatches_each_market_phase_through_one_complete_p0_p9_trace() {
    let cases = [
        (0, 10, TradingPhase::CallAuction),
        (600, 10, TradingPhase::PreOpen),
        (900, 10, TradingPhase::Continuous),
        (15_290, 10, TradingPhase::ClosingAuction),
    ];

    for (tick, closing_auction_ticks, expected_phase) in cases {
        let mut session = reachable_session_at(tick, closing_auction_ticks);
        assert_eq!(session.phase(), expected_phase);
        COMMIT_TRACES.with_borrow_mut(Vec::clear);

        let events = session.step().unwrap();

        assert_eq!(session.tick(), tick + 1);
        COMMIT_TRACES.with_borrow(|traces| assert_eq!(traces, &[TickPhase::ALL.to_vec()]));
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
            COMMIT_TRACES.with_borrow_mut(Vec::clear);
            session.step().unwrap();
        }

        assert_eq!(session.tick(), tick_after);
        assert_eq!(session.phase(), expected_phase);
        assert_eq!(session.day(), expected_day);
        COMMIT_TRACES.with_borrow(|traces| assert_eq!(traces, &[TickPhase::ALL.to_vec()]));

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
        COMMIT_TRACES.with_borrow_mut(Vec::clear);

        assert_eq!(session.step(), Err(fatal.clone()));

        assert_eq!(session.tick(), tick);
        assert_eq!(session.business_state_hash().unwrap(), expected_business);
        assert_eq!(session.poison_reason(), Some(&fatal));
        assert_eq!(session.save().unwrap_err(), fatal);
        COMMIT_TRACES.with_borrow(|traces| assert!(traces.is_empty()));
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
