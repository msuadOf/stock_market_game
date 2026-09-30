use super::{UrgencyError, UrgencyPolicy, UrgencyReason};
use crate::behavior::{PositionAction, PositionDecision};
use crate::observation::AccountRiskObservation;
use crate::plans::Urgency;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RiskUrgencyAssessment {
    Unavailable {
        reason: String,
    },
    Assessed {
        risk_reduction_active: bool,
        account_drawdown_bp: Option<u32>,
        urgency: Urgency,
        reason: UrgencyReason,
    },
}

pub fn assess_personal_risk_urgency(
    decision: Option<&PositionDecision>,
    risk: Option<&AccountRiskObservation>,
    policy: &UrgencyPolicy,
) -> Result<RiskUrgencyAssessment, UrgencyError> {
    policy.validate()?;
    let Some(risk) = risk else {
        return Ok(RiskUrgencyAssessment::Unavailable {
            reason: "no personal account-risk observation".to_owned(),
        });
    };
    let Some(decision) = decision else {
        return Ok(RiskUrgencyAssessment::Unavailable {
            reason: "no personal position decision".to_owned(),
        });
    };
    let account_drawdown_bp = risk
        .drawdown_from_peak
        .map(|change| {
            if !change.is_finite() || change < -1.0 {
                return Err(UrgencyError::InvalidObservation {
                    field: "drawdown_from_peak",
                    value: change.to_string(),
                });
            }
            Ok(((-change).max(0.0) * 10_000.0).round_ties_even() as u32)
        })
        .transpose()?;
    let risk_reduction_active = matches!(
        decision.action,
        PositionAction::Reduce | PositionAction::Exit
    ) && decision.desired_delta_shares < 0;
    let urgent = risk_reduction_active
        && account_drawdown_bp
            .is_some_and(|drawdown| drawdown >= policy.urgent_drawdown_threshold_bp);
    Ok(RiskUrgencyAssessment::Assessed {
        risk_reduction_active,
        account_drawdown_bp,
        urgency: if urgent {
            Urgency::Urgent
        } else {
            Urgency::Normal
        },
        reason: if urgent {
            UrgencyReason::RiskReductionDrawdown
        } else {
            UrgencyReason::DefaultNormal
        },
    })
}
