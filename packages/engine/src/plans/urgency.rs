//! K5a/K6 execution urgency, independent from valuation and direction scoring.

use super::{PauseReason, Urgency};
use crate::orderbook::Side;

mod policy;
pub use policy::{UrgencyPolicy, URGENCY_POLICY_VERSION};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatienceStyle {
    LongTerm,
    DeepValue,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PauseAssessment {
    Clear,
    Unavailable { reason: String },
    PauseAndRequestCancel(PauseReason),
}

impl PauseAssessment {
    pub const fn is_pause_requested(&self) -> bool {
        matches!(self, Self::PauseAndRequestCancel(_))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UrgencyInputs {
    pub side: Side,
    pub return_30min_bp: Option<i32>,
    pub return_1min_bp: Option<i32>,
    pub risk_pressure_pause: bool,
    pub adverse_selection_pause: bool,
    pub risk_reduction_active: bool,
    pub account_drawdown_bp: Option<u32>,
    pub remaining_trading_days: u32,
    pub confidence_bp: u32,
    pub style: PatienceStyle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UrgencyReason {
    RiskReductionDrawdown,
    HorizonDeadline,
    LowConfidence,
    PatientStyle,
    DefaultNormal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UrgencyAssessment {
    pub urgency: Urgency,
    pub reason: UrgencyReason,
    pub pause: PauseAssessment,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryInputs {
    pub is_next_own_observation: bool,
    pub pause: PauseAssessment,
    pub signal_score_bp: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryAssessment {
    WaitForOwnObservation,
    RemainPaused,
    Resume,
    ReviseOrTerminate,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum UrgencyError {
    #[error("unsupported urgency policy version {value}; expected {expected}")]
    UnsupportedPolicyVersion { value: u32, expected: u32 },
    #[error("invalid urgency policy field {field} = {value}")]
    InvalidPolicy { field: &'static str, value: i64 },
    #[error("invalid urgency input field {field} = {value} bp")]
    InvalidInput { field: &'static str, value: i64 },
}

pub fn assess_urgency(
    inputs: &UrgencyInputs,
    policy: &UrgencyPolicy,
) -> Result<UrgencyAssessment, UrgencyError> {
    policy.validate()?;
    validate_inputs(inputs)?;

    let pause = if inputs.risk_pressure_pause {
        PauseAssessment::PauseAndRequestCancel(PauseReason::RiskPressure)
    } else if inputs.adverse_selection_pause {
        PauseAssessment::PauseAndRequestCancel(PauseReason::AdverseSelection)
    } else if inputs.side == Side::Buy {
        match (inputs.return_30min_bp, inputs.return_1min_bp) {
            (Some(thirty), Some(one))
                if thirty <= policy.drop_30min_threshold_bp
                    && one <= policy.drop_1min_threshold_bp =>
            {
                PauseAssessment::PauseAndRequestCancel(PauseReason::IntradayDropAcceleration)
            }
            (Some(_), Some(_)) => PauseAssessment::Clear,
            (None, Some(_)) => PauseAssessment::Unavailable {
                reason: "missing 30-minute return".to_owned(),
            },
            (Some(_), None) => PauseAssessment::Unavailable {
                reason: "missing 1-minute return".to_owned(),
            },
            (None, None) => PauseAssessment::Unavailable {
                reason: "missing 30-minute and 1-minute returns".to_owned(),
            },
        }
    } else {
        PauseAssessment::Clear
    };

    let (urgency, reason) = if inputs.risk_reduction_active
        && inputs
            .account_drawdown_bp
            .is_some_and(|drawdown| drawdown >= policy.urgent_drawdown_threshold_bp)
    {
        (Urgency::Urgent, UrgencyReason::RiskReductionDrawdown)
    } else if inputs.remaining_trading_days <= policy.urgent_remaining_trading_days {
        (Urgency::Urgent, UrgencyReason::HorizonDeadline)
    } else if inputs.confidence_bp < policy.patient_confidence_threshold_bp {
        (Urgency::Patient, UrgencyReason::LowConfidence)
    } else {
        match inputs.style {
            PatienceStyle::LongTerm | PatienceStyle::DeepValue => {
                (Urgency::Patient, UrgencyReason::PatientStyle)
            }
            PatienceStyle::Other => (Urgency::Normal, UrgencyReason::DefaultNormal),
        }
    };

    Ok(UrgencyAssessment {
        urgency,
        reason,
        pause,
    })
}

pub fn assess_recovery(
    inputs: &RecoveryInputs,
    policy: &UrgencyPolicy,
) -> Result<RecoveryAssessment, UrgencyError> {
    policy.validate()?;
    if !(-10_000..=10_000).contains(&inputs.signal_score_bp) {
        return Err(UrgencyError::InvalidInput {
            field: "signal_score_bp",
            value: i64::from(inputs.signal_score_bp),
        });
    }
    if !inputs.is_next_own_observation {
        return Ok(RecoveryAssessment::WaitForOwnObservation);
    }
    match &inputs.pause {
        PauseAssessment::PauseAndRequestCancel(_) | PauseAssessment::Unavailable { .. } => {
            Ok(RecoveryAssessment::RemainPaused)
        }
        PauseAssessment::Clear if inputs.signal_score_bp >= policy.resume_signal_threshold_bp => {
            Ok(RecoveryAssessment::Resume)
        }
        PauseAssessment::Clear => Ok(RecoveryAssessment::ReviseOrTerminate),
    }
}

fn validate_inputs(inputs: &UrgencyInputs) -> Result<(), UrgencyError> {
    if inputs.confidence_bp > 10_000 {
        return Err(UrgencyError::InvalidInput {
            field: "confidence_bp",
            value: i64::from(inputs.confidence_bp),
        });
    }
    if inputs
        .account_drawdown_bp
        .is_some_and(|value| value > 10_000)
    {
        return Err(UrgencyError::InvalidInput {
            field: "account_drawdown_bp",
            value: i64::from(inputs.account_drawdown_bp.unwrap_or_default()),
        });
    }
    for (field, value) in [
        ("return_30min_bp", inputs.return_30min_bp),
        ("return_1min_bp", inputs.return_1min_bp),
    ] {
        if value.is_some_and(|bp| !(-10_000..=10_000).contains(&bp)) {
            return Err(UrgencyError::InvalidInput {
                field,
                value: i64::from(value.unwrap_or_default()),
            });
        }
    }
    Ok(())
}
