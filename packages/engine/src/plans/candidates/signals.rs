use crate::behavior::PositionAction;
use crate::strategy::{
    RelativeStrengthIndex, SimpleMovingAverage, TechnicalError, ValuationOutcome,
};
use crate::Money;

use super::types::{CandidateError, SignalContribution, SignalScore, SignalUnavailableReason};

pub fn normalized_score(x: i64, threshold: i64) -> Result<SignalScore, CandidateError> {
    if threshold <= 0 {
        return Err(CandidateError::InvalidThreshold { threshold });
    }
    let numerator =
        i128::from(x)
            .checked_mul(10_000)
            .ok_or(CandidateError::ArithmeticOverflow {
                step: "normalized score",
            })?;
    let rounded = div_round_half_even(numerator, i128::from(threshold)).clamp(-10_000, 10_000);
    SignalScore::new(
        i32::try_from(rounded).map_err(|_| CandidateError::ArithmeticOverflow {
            step: "normalized score conversion",
        })?,
    )
}

pub fn fundamental_range_signal(
    current: Money,
    pessimistic: Money,
    optimistic: Money,
) -> Result<SignalContribution, CandidateError> {
    require_positive("current price", current)?;
    require_positive("pessimistic valuation", pessimistic)?;
    require_positive("optimistic valuation", optimistic)?;
    if pessimistic > optimistic {
        return Err(CandidateError::InvertedValuationRange {
            low_cents: pessimistic.cents(),
            high_cents: optimistic.cents(),
        });
    }
    if (pessimistic..=optimistic).contains(&current) {
        return Ok(SignalContribution::available(SignalScore::bounded(0)));
    }
    let midpoint = div_round_half_even(
        i128::from(pessimistic.cents()) + i128::from(optimistic.cents()),
        2,
    );
    let relative_bp = div_round_half_even(
        (midpoint - i128::from(current.cents())) * 10_000,
        i128::from(current.cents()),
    );
    let relative_bp =
        i64::try_from(relative_bp).map_err(|_| CandidateError::ArithmeticOverflow {
            step: "fundamental relative bp",
        })?;
    Ok(SignalContribution::available(normalized_score(
        relative_bp,
        2_000,
    )?))
}

pub fn fundamental_signal(
    current: Money,
    valuation: &ValuationOutcome,
) -> Result<SignalContribution, CandidateError> {
    match valuation {
        ValuationOutcome::Available { per_share, .. } => {
            fundamental_range_signal(current, per_share.pessimistic, per_share.optimistic)
        }
        ValuationOutcome::Unavailable { .. } => Ok(SignalContribution::unavailable(
            SignalUnavailableReason::FundamentalUnavailable,
        )),
    }
}

pub fn trend_signal(
    thirty_minute_return_bp: Option<i32>,
    five_day_return_bp: Option<i32>,
) -> Result<SignalContribution, CandidateError> {
    weighted_atoms(&[
        atom(
            thirty_minute_return_bp,
            300,
            4,
            SignalUnavailableReason::MissingThirtyMinuteReturn,
        )?,
        atom(
            five_day_return_bp,
            1_000,
            6,
            SignalUnavailableReason::MissingFiveDayReturn,
        )?,
    ])
}

pub fn price_volume_signal(
    thirty_minute_return_bp: Option<i32>,
    relative_volume_bp: Option<i32>,
    imbalance: Option<SignalScore>,
) -> Result<SignalContribution, CandidateError> {
    let volume_score = match (thirty_minute_return_bp, relative_volume_bp) {
        (Some(ret), Some(volume)) => {
            let score = normalized_score(i64::from(volume) - 10_000, 10_000)?;
            Some(SignalScore::bounded(score.value() * ret.signum()))
        }
        (None, _) | (Some(_), None) => None,
    };
    let volume_reason = if thirty_minute_return_bp.is_none() {
        SignalUnavailableReason::MissingThirtyMinuteReturn
    } else {
        SignalUnavailableReason::MissingRelativeVolume
    };
    weighted_atoms(&[
        (volume_score, 5, volume_reason),
        (
            imbalance,
            5,
            SignalUnavailableReason::MissingOrderBookImbalance,
        ),
    ])
}

pub fn technical_signal(
    sma20: &Result<SimpleMovingAverage, TechnicalError>,
    sma60: &Result<SimpleMovingAverage, TechnicalError>,
    rsi14: &Result<RelativeStrengthIndex, TechnicalError>,
) -> Result<SignalContribution, CandidateError> {
    let sma_score = match (sma20, sma60) {
        (Ok(short), Ok(long)) => {
            require_positive("SMA60", long.average)?;
            let difference = i128::from(short.average.cents()) - i128::from(long.average.cents());
            let ratio_bp =
                div_round_half_even(difference * 10_000, i128::from(long.average.cents()));
            Some(normalized_score(
                i64::try_from(ratio_bp).map_err(|_| CandidateError::ArithmeticOverflow {
                    step: "SMA ratio bp",
                })?,
                500,
            )?)
        }
        (Err(_), _) | (_, Err(_)) => None,
    };
    let sma_reason = if sma20.is_err() {
        SignalUnavailableReason::MissingSma20
    } else {
        SignalUnavailableReason::MissingSma60
    };
    let rsi_score = rsi14
        .as_ref()
        .ok()
        .map(|rsi| normalized_score(i64::from(50 - i32::from(rsi.value)), 20))
        .transpose()?;
    weighted_atoms(&[
        (sma_score, 7, sma_reason),
        (rsi_score, 3, SignalUnavailableReason::MissingRsi14),
    ])
}

pub fn experience_cost_signal(action: PositionAction) -> SignalContribution {
    let score = match action {
        PositionAction::Hold | PositionAction::Watch => 0,
        PositionAction::TryBuy => 2_500,
        PositionAction::Add => 5_000,
        PositionAction::Reduce => -5_000,
        PositionAction::Exit => -10_000,
    };
    SignalContribution::available(SignalScore::bounded(score))
}

fn atom(
    value: Option<i32>,
    threshold: i64,
    weight: u32,
    reason: SignalUnavailableReason,
) -> Result<(Option<SignalScore>, u32, SignalUnavailableReason), CandidateError> {
    Ok((
        value
            .map(|value| normalized_score(i64::from(value), threshold))
            .transpose()?,
        weight,
        reason,
    ))
}

fn weighted_atoms(
    atoms: &[(Option<SignalScore>, u32, SignalUnavailableReason)],
) -> Result<SignalContribution, CandidateError> {
    let mut numerator = 0_i128;
    let mut weight = 0_u32;
    let mut unavailable_atoms = Vec::new();
    for (score, atom_weight, reason) in atoms {
        match score {
            Some(score) => {
                numerator += i128::from(*atom_weight) * i128::from(score.value());
                weight += *atom_weight;
            }
            None => unavailable_atoms.push(reason.clone()),
        }
    }
    if weight == 0 {
        return Ok(SignalContribution {
            score: None,
            unavailable_atoms,
        });
    }
    let score =
        i32::try_from(div_round_half_even(numerator, i128::from(weight))).map_err(|_| {
            CandidateError::ArithmeticOverflow {
                step: "atomic weighted score",
            }
        })?;
    Ok(SignalContribution {
        score: Some(SignalScore::new(score)?),
        unavailable_atoms,
    })
}

fn require_positive(field: &'static str, money: Money) -> Result<(), CandidateError> {
    if money.cents() <= 0 {
        return Err(CandidateError::NonPositiveMoney {
            field,
            cents: money.cents(),
        });
    }
    Ok(())
}

pub(super) fn div_round_half_even(numerator: i128, denominator: i128) -> i128 {
    debug_assert!(denominator > 0);
    let sign = if numerator < 0 { -1 } else { 1 };
    let numerator = numerator.unsigned_abs();
    let denominator = denominator.unsigned_abs();
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    let rounded =
        if remainder * 2 > denominator || (remainder * 2 == denominator && quotient % 2 == 1) {
            quotient + 1
        } else {
            quotient
        };
    // SAFE-EXPECT: all callers supply bounded products whose rounded quotient is at most i64.
    sign * i128::try_from(rounded).expect("absolute i128 quotient fits i128")
}
