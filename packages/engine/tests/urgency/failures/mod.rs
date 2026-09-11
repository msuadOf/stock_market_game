use crate::*;
use engine::plans::quote_policy::{decide_quote, QuoteAction, QuoteError, QuoteReason};
use engine::plans::urgency::{assess_recovery, assess_urgency, RecoveryAssessment, RecoveryInputs};

#[test]
fn unavailable_when_either_drop_window_is_missing() {
    // Given
    let cases = [(None, Some(-75)), (Some(-300), None)];

    // When / Then
    for (thirty, one) in cases {
        let mut inputs = base_urgency_inputs();
        inputs.return_30min_bp = thirty;
        inputs.return_1min_bp = one;
        assert!(matches!(
            assess_urgency(&inputs, &Default::default()).unwrap().pause,
            PauseAssessment::Unavailable { .. }
        ));
    }
}

#[test]
fn unavailable_drop_trigger_never_requests_a_cancel() {
    // Given
    let mut inputs = base_quote_inputs();
    inputs.pause = PauseAssessment::Unavailable {
        reason: "missing 1-minute return".into(),
    };
    inputs.active_order = Some(active_quote(1010));

    // When
    let decision = decide_quote(&inputs).unwrap();

    // Then
    assert_eq!(
        decision.action,
        QuoteAction::Keep {
            order_id: engine::OrderId(41)
        }
    );
}

#[test]
fn sell_plan_is_not_paused_by_the_buy_plan_drop_trigger() {
    // Given
    let mut inputs = base_urgency_inputs();
    inputs.side = engine::Side::Sell;
    inputs.return_30min_bp = Some(-301);
    inputs.return_1min_bp = Some(-76);

    // When
    let result = assess_urgency(&inputs, &Default::default()).unwrap();

    // Then
    assert_eq!(result.pause, PauseAssessment::Clear);
}

#[test]
fn invalid_tick_and_price_band_are_rejected_not_clamped() {
    // Given
    let mut invalid_tick = base_quote_inputs();
    invalid_tick.book.best_ask = Some(money(1011));
    let mut outside_band = base_quote_inputs();
    outside_band.urgency = Urgency::Urgent;
    outside_band.protection_limit = money(1110);
    outside_band.cage_bound = Some(money(1120));

    // When / Then
    assert!(matches!(
        decide_quote(&invalid_tick),
        Err(QuoteError::InvalidTick { .. })
    ));
    assert!(matches!(
        decide_quote(&outside_band),
        Err(QuoteError::OutsidePriceBand { .. })
    ));
}

#[test]
fn price_cage_is_rejected_by_router_equivalent_guard() {
    // Given
    let mut inputs = base_quote_inputs();
    inputs.urgency = Urgency::Urgent;
    inputs.protection_limit = money(1030);

    // When
    let result = decide_quote(&inputs);

    // Then
    assert!(matches!(result, Err(QuoteError::OutsidePriceCage { .. })));
}

#[test]
fn invalid_quantity_is_rejected_before_submit() {
    // Given
    let mut inputs = base_quote_inputs();
    inputs.desired_qty = 50;

    // When
    let result = decide_quote(&inputs);

    // Then
    assert_eq!(result.unwrap_err(), QuoteError::InvalidQuantity { qty: 50 });
}

#[test]
fn sell_quantity_above_available_inventory_is_rejected_even_with_matching_remainder() {
    // Given
    let quantities = [151, 250];

    // When / Then
    for qty in quantities {
        let mut inputs = base_quote_inputs();
        inputs.side = engine::Side::Sell;
        inputs.protection_limit = money(950);
        inputs.cage_bound = Some(money(980));
        inputs.desired_qty = qty;
        inputs.available_sell_qty = 150;
        assert_eq!(
            decide_quote(&inputs).unwrap_err(),
            QuoteError::InvalidQuantity { qty }
        );
    }
}

#[test]
fn noncancellable_cancel_is_pending_reconsideration_and_suppresses_new_quote() {
    // Given
    let mut inputs = base_quote_inputs();
    inputs.pause = PauseAssessment::PauseAndRequestCancel(engine::PauseReason::RiskPressure);
    inputs.active_order = Some(active_quote(1000));
    inputs.cancellable_now = false;

    // When
    let decision = decide_quote(&inputs).unwrap();

    // Then
    assert_eq!(
        decision.action,
        QuoteAction::Keep {
            order_id: engine::OrderId(41)
        }
    );
    assert_eq!(decision.reason, QuoteReason::PendingReconsideration);
}

#[test]
fn empty_book_waits_for_patient_and_does_not_assume_a_fill() {
    // Given
    let mut inputs = base_quote_inputs();
    inputs.urgency = Urgency::Patient;
    inputs.book = BookTop {
        best_bid: None,
        best_ask: None,
    };

    // When
    let decision = decide_quote(&inputs).unwrap();

    // Then
    assert_eq!(decision.action, QuoteAction::Wait);
    assert_eq!(decision.reason, QuoteReason::EmptyBook);
}

#[test]
fn one_sided_book_does_not_let_normal_take_an_unconfirmed_opponent_quote() {
    // Given
    let mut inputs = base_quote_inputs();
    inputs.book.best_bid = None;

    // When
    let decision = decide_quote(&inputs).unwrap();

    // Then
    assert_eq!(decision.action, QuoteAction::Wait);
    assert_eq!(decision.reason, QuoteReason::IncompleteBook);
}

#[test]
fn active_order_is_cancelled_when_liquidity_disappears() {
    // Given
    let mut inputs = base_quote_inputs();
    inputs.book = BookTop {
        best_bid: None,
        best_ask: None,
    };
    inputs.active_order = Some(active_quote(1010));

    // When
    let decision = decide_quote(&inputs).unwrap();

    // Then
    assert_eq!(
        decision.action,
        QuoteAction::Cancel {
            order_id: engine::OrderId(41)
        }
    );
    assert_eq!(decision.reason, QuoteReason::EmptyBook);
}

#[test]
fn recovery_remains_paused_while_trigger_is_unavailable() {
    // Given
    let inputs = RecoveryInputs {
        is_next_own_observation: true,
        pause: PauseAssessment::Unavailable {
            reason: "missing 30-minute return".into(),
        },
        signal_score_bp: 3000,
    };

    // When
    let result = assess_recovery(&inputs, &Default::default()).unwrap();

    // Then
    assert_eq!(result, RecoveryAssessment::RemainPaused);
}

#[test]
fn unsupported_policy_version_is_rejected() {
    // Given
    let policy = engine::plans::urgency::UrgencyPolicy {
        policy_version: 2,
        ..Default::default()
    };

    // When
    let result = assess_urgency(&base_urgency_inputs(), &policy);

    // Then
    assert!(result.is_err());
}
