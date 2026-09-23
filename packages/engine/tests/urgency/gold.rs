use super::*;
use engine::plans::quote_policy::{decide_quote, QuoteAction, QuoteReason};
use engine::plans::urgency::{
    assess_recovery, assess_urgency, RecoveryAssessment, RecoveryInputs, UrgencyReason,
};

#[test]
fn patient_when_confidence_is_below_boundary() {
    // Given
    let mut inputs = base_urgency_inputs();
    inputs.confidence_bp = 3999;

    // When
    let decision = assess_urgency(&inputs, &Default::default()).unwrap();

    // Then
    assert_eq!(decision.urgency, Urgency::Patient);
    assert_eq!(decision.reason, UrgencyReason::LowConfidence);
}

#[test]
fn normal_when_confidence_is_exactly_patient_boundary() {
    // Given
    let mut inputs = base_urgency_inputs();
    inputs.confidence_bp = 4000;

    // When
    let decision = assess_urgency(&inputs, &Default::default()).unwrap();

    // Then
    assert_eq!(decision.urgency, Urgency::Normal);
}

#[test]
fn patient_when_style_is_long_term_or_deep_value() {
    // Given
    let policy = Default::default();
    let styles = [PatienceStyle::LongTerm, PatienceStyle::DeepValue];

    // When / Then
    for style in styles {
        let mut inputs = base_urgency_inputs();
        inputs.style = style;
        assert_eq!(
            assess_urgency(&inputs, &policy).unwrap().urgency,
            Urgency::Patient
        );
    }
}

#[test]
fn normal_when_no_urgent_or_patient_condition_applies() {
    // Given
    let inputs = base_urgency_inputs();

    // When
    let decision = assess_urgency(&inputs, &Default::default()).unwrap();

    // Then
    assert_eq!(decision.urgency, Urgency::Normal);
    assert_eq!(decision.reason, UrgencyReason::DefaultNormal);
}

#[test]
fn urgent_at_drawdown_boundary_only_for_risk_reduction() {
    // Given
    let mut at_boundary = base_urgency_inputs();
    at_boundary.risk_reduction_active = true;
    at_boundary.account_drawdown_bp = Some(2000);
    let mut below_boundary = at_boundary.clone();
    below_boundary.account_drawdown_bp = Some(1999);

    // When
    let urgent = assess_urgency(&at_boundary, &Default::default()).unwrap();
    let normal = assess_urgency(&below_boundary, &Default::default()).unwrap();

    // Then
    assert_eq!(urgent.urgency, Urgency::Urgent);
    assert_eq!(urgent.reason, UrgencyReason::RiskReductionDrawdown);
    assert_eq!(normal.urgency, Urgency::Normal);
}

#[test]
fn urgent_when_exactly_one_trading_day_remains() {
    // Given
    let mut inputs = base_urgency_inputs();
    inputs.remaining_trading_days = 1;

    // When
    let decision = assess_urgency(&inputs, &Default::default()).unwrap();

    // Then
    assert_eq!(decision.urgency, Urgency::Urgent);
    assert_eq!(decision.reason, UrgencyReason::HorizonDeadline);
}

#[test]
fn cheap_but_withdraw_when_both_drop_windows_reach_boundaries() {
    // Given: the fundamental opinion remains bullish while execution sees an accelerating drop.
    let open = bullish_buy_open();
    let mut inputs = base_urgency_inputs();
    inputs.return_30min_bp = Some(-300);
    inputs.return_1min_bp = Some(-75);

    // When
    let urgency = assess_urgency(&inputs, &Default::default()).unwrap();
    let mut quote_inputs = base_quote_inputs();
    quote_inputs.pause = urgency.pause.clone();
    quote_inputs.active_order = Some(active_quote(1000));
    let quote = decide_quote(&quote_inputs).unwrap();

    // Then
    assert_eq!(open.opinion.signal_score_bp, 2600);
    assert_eq!(
        urgency.pause,
        PauseAssessment::PauseAndRequestCancel(engine::PauseReason::IntradayDropAcceleration)
    );
    assert_eq!(
        quote.action,
        QuoteAction::Cancel {
            order_id: engine::OrderId(41)
        }
    );
    assert_eq!(quote.reason, QuoteReason::PauseRequested);
}

#[test]
fn risk_and_adverse_selection_can_pause_without_changing_the_opinion_score() {
    // Given
    let policy = Default::default();
    let mut risk = base_urgency_inputs();
    risk.risk_pressure_pause = true;
    let mut adverse = base_urgency_inputs();
    adverse.adverse_selection_pause = true;

    // When / Then
    assert_eq!(
        assess_urgency(&risk, &policy).unwrap().pause,
        PauseAssessment::PauseAndRequestCancel(engine::PauseReason::RiskPressure)
    );
    assert_eq!(
        assess_urgency(&adverse, &policy).unwrap().pause,
        PauseAssessment::PauseAndRequestCancel(engine::PauseReason::AdverseSelection)
    );
}

#[test]
fn drop_trigger_is_inclusive_and_requires_both_windows() {
    // Given
    let policy = Default::default();
    let cases = [
        (-300, -75, true),
        (-301, -75, true),
        (-300, -76, true),
        (-299, -75, false),
        (-300, -74, false),
    ];

    // When / Then
    for (thirty, one, pauses) in cases {
        let mut inputs = base_urgency_inputs();
        inputs.return_30min_bp = Some(thirty);
        inputs.return_1min_bp = Some(one);
        assert_eq!(
            assess_urgency(&inputs, &policy)
                .unwrap()
                .pause
                .is_pause_requested(),
            pauses
        );
    }
}

#[test]
fn quote_policy_maps_patient_normal_and_urgent_to_distinct_prices() {
    // Given
    let mut patient = base_quote_inputs();
    patient.urgency = Urgency::Patient;
    let normal = base_quote_inputs();
    let mut urgent = base_quote_inputs();
    urgent.urgency = Urgency::Urgent;

    // When
    let patient_decision = decide_quote(&patient).unwrap();
    let normal_decision = decide_quote(&normal).unwrap();
    let urgent_decision = decide_quote(&urgent).unwrap();

    // Then
    assert_eq!(
        patient_decision.action,
        QuoteAction::Submit {
            price: money(1000),
            qty: 100
        }
    );
    assert_eq!(
        normal_decision.action,
        QuoteAction::Submit {
            price: money(1010),
            qty: 100
        }
    );
    assert_eq!(
        urgent_decision.action,
        QuoteAction::Submit {
            price: money(1020),
            qty: 100
        }
    );
}

#[test]
fn replace_reports_queue_priority_loss_when_price_changes() {
    // Given
    let mut inputs = base_quote_inputs();
    inputs.active_order = Some(active_quote(1000));

    // When
    let decision = decide_quote(&inputs).unwrap();

    // Then
    assert_eq!(
        decision.action,
        QuoteAction::Replace {
            order_id: engine::OrderId(41),
            price: money(1010),
            qty: 100,
        }
    );
    assert_eq!(decision.reason, QuoteReason::QueuePriorityLost);
}

#[test]
fn sell_quote_accepts_exact_available_inventory_with_odd_lot_remainder() {
    // Given
    let mut inputs = base_quote_inputs();
    inputs.side = Side::Sell;
    inputs.protection_limit = money(950);
    inputs.cage_bound = Some(money(980));
    inputs.desired_qty = 150;
    inputs.available_sell_qty = 150;

    // When
    let decision = decide_quote(&inputs).unwrap();

    // Then
    assert_eq!(
        decision.action,
        QuoteAction::Submit {
            price: money(1000),
            qty: 150
        }
    );
}

#[test]
fn recovery_requires_next_own_observation_clear_trigger_and_bullish_score() {
    // Given
    let base = RecoveryInputs {
        is_next_own_observation: true,
        pause: PauseAssessment::Clear,
        signal_score_bp: 2000,
    };

    // When / Then
    assert_eq!(
        assess_recovery(&base, &Default::default()).unwrap(),
        RecoveryAssessment::Resume
    );
    assert_eq!(
        assess_recovery(
            &RecoveryInputs {
                is_next_own_observation: false,
                ..base.clone()
            },
            &Default::default()
        )
        .unwrap(),
        RecoveryAssessment::WaitForOwnObservation
    );
    assert_eq!(
        assess_recovery(
            &RecoveryInputs {
                signal_score_bp: 1999,
                ..base
            },
            &Default::default()
        )
        .unwrap(),
        RecoveryAssessment::ReviseOrTerminate
    );
}

#[test]
fn urgency_policy_is_versioned_and_round_trips() {
    // Given
    let policy = engine::plans::urgency::UrgencyPolicy::default();

    // When
    let encoded = serde_json::to_string(&policy).unwrap();
    let decoded: engine::plans::urgency::UrgencyPolicy = serde_json::from_str(&encoded).unwrap();

    // Then
    assert_eq!(decoded, policy);
    assert_eq!(decoded.policy_version, 1);
}
