use super::*;
use crate::plans::urgency::risk::{assess_personal_risk_urgency, RiskUrgencyAssessment};
use crate::plans::UrgencyAssessment;

impl GameSession {
    pub(super) fn plan_execution_urgency(
        &self,
        plan: &TradingPlan,
        return_30min_bp: Option<i32>,
        return_1min_bp: Option<i32>,
    ) -> (UrgencyAssessment, RiskUrgencyAssessment) {
        let risk = assess_personal_risk_urgency(None, None, &self.urgency_policy).unwrap_or_else(
            |error| panic!("plan risk urgency failed for {:?}: {error}", plan.plan_id),
        );
        let (risk_reduction_active, account_drawdown_bp) = match &risk {
            RiskUrgencyAssessment::Assessed {
                risk_reduction_active,
                account_drawdown_bp,
                ..
            } => (*risk_reduction_active, *account_drawdown_bp),
            RiskUrgencyAssessment::Unavailable { .. } => (false, None),
        };
        let inputs = UrgencyInputs {
            side: plan.direction,
            return_30min_bp,
            return_1min_bp,
            risk_pressure_pause: false,
            adverse_selection_pause: false,
            risk_reduction_active,
            account_drawdown_bp,
            remaining_trading_days: u32::try_from(
                plan.last_valid_trading_day()
                    .saturating_sub(u64::from(self.day)),
            )
            .expect("live plan remaining days fit its u32 horizon"),
            confidence_bp: plan.confidence_bp,
            style: match self.belief_style(plan.account) {
                Some(crate::strategy::InstitutionStyle::DeepValue) => PatienceStyle::DeepValue,
                _ => PatienceStyle::Other,
            },
        };
        let urgency = assess_urgency(&inputs, &self.urgency_policy)
            .unwrap_or_else(|error| panic!("plan urgency failed for {:?}: {error}", plan.plan_id));
        (urgency, risk)
    }
}
