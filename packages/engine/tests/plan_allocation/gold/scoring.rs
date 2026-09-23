use engine::behavior::PositionAction;
use engine::plans::{
    blend_candidate, experience_cost_signal, fundamental_range_signal, normalized_score,
    price_volume_signal, reverse_crosses_threshold, technical_signal, trend_signal,
    CandidateAssessment, CandidateSignals, SignalComponent, SignalScore, SignalUnavailableReason,
};
use engine::strategy::{
    AnalysisWeights, RelativeStrengthIndex, SimpleMovingAverage, TechnicalError,
};
use engine::Side;

use super::super::{available, money};

#[test]
fn fundamental_and_technical_conflict_has_exact_blended_score() {
    // Given: fundamental says +5000, technical says -5000, weights are 7000/3000.
    let weights = AnalysisWeights::new(7_000, 0, 0, 3_000, 0).unwrap();
    let signals = CandidateSignals {
        fundamental: fundamental_range_signal(money(1_000), money(1_090), money(1_110)).unwrap(),
        trend: available(0),
        price_volume: available(0),
        technical: available(-5_000),
        experience: available(0),
    };

    // When: K5a mixes only non-zero, available dimensions.
    let assessment = blend_candidate(&weights, &signals);

    // Then: rhe((7000*5000 + 3000*-5000)/10000) = 2000.
    assert!(matches!(
        assessment,
        CandidateAssessment::Scored { score, used_weight_bp: 10_000, .. }
            if score.value() == 2_000
    ));
}

#[test]
fn normalized_score_uses_half_even_for_positive_and_negative_ties() {
    // Given / When / Then: exact halves round to the nearest even integer symmetrically.
    assert_eq!(normalized_score(1, 20_000).unwrap().value(), 0);
    assert_eq!(normalized_score(3, 20_000).unwrap().value(), 2);
    assert_eq!(normalized_score(-1, 20_000).unwrap().value(), 0);
    assert_eq!(normalized_score(-3, 20_000).unwrap().value(), -2);
}

#[test]
fn unavailable_dimension_reweights_remaining_weight_exactly() {
    // Given: an unavailable 7000bp fundamental and available -3333 technical at 3000bp.
    let weights = AnalysisWeights::new(7_000, 0, 0, 3_000, 0).unwrap();
    let signals = CandidateSignals {
        fundamental: engine::plans::SignalContribution::unavailable(
            SignalUnavailableReason::FundamentalUnavailable,
        ),
        trend: available(0),
        price_volume: available(0),
        technical: available(-3_333),
        experience: available(0),
    };

    // When / Then: available weight is renormalized, not replaced by a zero score.
    match blend_candidate(&weights, &signals) {
        CandidateAssessment::Scored {
            score,
            used_weight_bp,
            excluded,
        } => {
            assert_eq!(score.value(), -3_333);
            assert_eq!(used_weight_bp, 3_000);
            assert!(excluded
                .iter()
                .any(|item| item.component == SignalComponent::Fundamental));
        }
        other => panic!("expected scored assessment, got {other:?}"),
    }
}

#[test]
fn neutral_available_signals_produce_exact_zero() {
    // Given: every enabled dimension is genuinely available and neutral.
    let weights = AnalysisWeights::new(2_000, 2_000, 2_000, 2_000, 2_000).unwrap();
    let signals = CandidateSignals {
        fundamental: available(0),
        trend: available(0),
        price_volume: available(0),
        technical: available(0),
        experience: experience_cost_signal(PositionAction::Hold),
    };

    // When / Then: neutral evidence is distinct from insufficient information.
    assert!(matches!(
        blend_candidate(&weights, &signals),
        CandidateAssessment::Scored { score, used_weight_bp: 10_000, .. } if score.value() == 0
    ));
}

#[test]
fn trend_missing_window_reweights_atomic_signal_and_records_reason() {
    // Given: only the 30-minute return exists (+300bp => +10000).
    let signal = trend_signal(Some(300), None).unwrap();

    // When / Then: the 40% atom becomes the whole dimension and the absent atom is recorded.
    assert_eq!(signal.score().unwrap().value(), 10_000);
    assert_eq!(
        signal.unavailable_atoms(),
        &[SignalUnavailableReason::MissingFiveDayReturn]
    );
}

#[test]
fn technical_formula_has_exact_value_and_ignores_atr_direction() {
    // Given: SMA ratio is +500bp => +10000 and RSI=70 => -10000.
    let signal = technical_signal(
        &Ok(SimpleMovingAverage {
            window: 20,
            average: money(1_050),
        }),
        &Ok(SimpleMovingAverage {
            window: 60,
            average: money(1_000),
        }),
        &Ok(RelativeStrengthIndex {
            window: 14,
            valid_samples: 14,
            value: 70,
            all_flat: false,
        }),
    )
    .unwrap();
    assert_eq!(signal.score().unwrap().value(), 4_000);

    // An unavailable RSI atom reweights SMA without fabricating RSI=50.
    let missing = technical_signal(
        &Ok(SimpleMovingAverage {
            window: 20,
            average: money(1_050),
        }),
        &Ok(SimpleMovingAverage {
            window: 60,
            average: money(1_000),
        }),
        &Err(TechnicalError::InsufficientHistory {
            available: 10,
            required: 15,
        }),
    )
    .unwrap();
    assert_eq!(missing.score().unwrap().value(), 10_000);
}

#[test]
fn price_volume_formula_has_exact_value() {
    // Given: a negative 30-minute return, 2x relative volume, and +2000 imbalance.
    let signal = price_volume_signal(
        Some(-100),
        Some(20_000),
        Some(SignalScore::new(2_000).unwrap()),
    )
    .unwrap();

    // When / Then: 0.5*(-10000) + 0.5*(2000) = -4000.
    assert_eq!(signal.score().unwrap().value(), -4_000);
}

#[test]
fn reverse_hysteresis_reuses_exact_opposite_threshold_boundaries() {
    // Given / When / Then: equality crosses; one bp inside the deadband does not.
    assert!(reverse_crosses_threshold(
        Side::Sell,
        Side::Buy,
        2_000,
        2_000
    ));
    assert!(!reverse_crosses_threshold(
        Side::Sell,
        Side::Buy,
        1_999,
        2_000
    ));
    assert!(reverse_crosses_threshold(
        Side::Buy,
        Side::Sell,
        -2_000,
        2_000
    ));
    assert!(!reverse_crosses_threshold(
        Side::Buy,
        Side::Sell,
        -1_999,
        2_000
    ));
}
