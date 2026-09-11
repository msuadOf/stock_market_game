//! K5a account-local candidate scoring and target conversion.

mod signals;
mod targets;
mod types;

pub use signals::{
    experience_cost_signal, fundamental_range_signal, fundamental_signal, normalized_score,
    price_volume_signal, technical_signal, trend_signal,
};
pub use targets::{
    eligible_candidates, target_position_weight_bp, target_share_quantity, QuantityRounding,
    TargetShareQuantity,
};
pub use types::{
    CandidateAssessment, CandidateError, CandidateSignals, ExcludedSignal, SignalComponent,
    SignalContribution, SignalScore, SignalUnavailableReason,
};

use crate::strategy::AnalysisWeights;

use self::signals::div_round_half_even;

pub fn blend_candidate(
    weights: &AnalysisWeights,
    signals: &CandidateSignals,
) -> CandidateAssessment {
    let components = [
        (
            SignalComponent::Fundamental,
            weights.fundamental_bp(),
            &signals.fundamental,
        ),
        (SignalComponent::Trend, weights.trend_bp(), &signals.trend),
        (
            SignalComponent::PriceVolume,
            weights.price_volume_bp(),
            &signals.price_volume,
        ),
        (
            SignalComponent::Technical,
            weights.technical_bp(),
            &signals.technical,
        ),
        (
            SignalComponent::ExperienceCost,
            weights.experience_cost_bp(),
            &signals.experience,
        ),
    ];
    let mut numerator = 0_i128;
    let mut used_weight_bp = 0_u32;
    let mut excluded = Vec::new();
    for (component, weight, contribution) in components {
        match (weight, contribution.score) {
            (0, _) => excluded.push(ExcludedSignal {
                component,
                reason: SignalUnavailableReason::ZeroWeight,
            }),
            (weight, Some(score)) => {
                numerator += i128::from(weight) * i128::from(score.value());
                used_weight_bp += weight;
            }
            (_, None) => excluded.push(ExcludedSignal {
                component,
                reason: contribution
                    .unavailable_atoms
                    .first()
                    .cloned()
                    .unwrap_or(SignalUnavailableReason::MissingObservation),
            }),
        }
    }
    if used_weight_bp == 0 {
        CandidateAssessment::InsufficientInformation { excluded }
    } else {
        let value = i32::try_from(div_round_half_even(numerator, i128::from(used_weight_bp)))
            // SAFE-EXPECT: a weighted mean of bounded scores remains in -10_000..=10_000.
            .expect("weighted bounded scores remain in i32 range");
        CandidateAssessment::Scored {
            score: SignalScore::bounded(value),
            used_weight_bp,
            excluded,
        }
    }
}
