use super::*;
use crate::observation::build_account_risk_observation;
use crate::plans::urgency::risk::{assess_existing_reduction_urgency, RiskUrgencyAssessment};
use crate::plans::UrgencyAssessment;

impl GameSession {
    pub(super) fn initial_plan_urgency(
        &self,
        account: AccountId,
        direction: Side,
        confidence_bp: u32,
        horizon_trading_days: u32,
    ) -> Urgency {
        // 新方向不等于风险减仓；回撤不能凭空生成卖出意图。
        let inputs = UrgencyInputs {
            side: direction,
            return_30min_bp: None,
            return_1min_bp: None,
            risk_pressure_pause: false,
            adverse_selection_pause: false,
            risk_reduction_active: false,
            account_drawdown_bp: None,
            remaining_trading_days: horizon_trading_days.saturating_sub(1),
            confidence_bp,
            style: match self.belief_style(account) {
                Some(crate::strategy::InstitutionStyle::DeepValue) => PatienceStyle::DeepValue,
                _ => PatienceStyle::Other,
            },
        };
        assess_urgency(&inputs, &self.state.urgency_policy)
            .unwrap_or_else(|error| panic!("initial plan urgency failed for {account:?}: {error}"))
            .urgency
    }

    #[cfg(test)]
    pub(super) fn plan_execution_urgency(
        &self,
        plan: &TradingPlan,
        return_30min_bp: Option<i32>,
        return_1min_bp: Option<i32>,
    ) -> (UrgencyAssessment, RiskUrgencyAssessment) {
        self.plan_execution_urgency_at_view(
            plan,
            return_30min_bp,
            return_1min_bp,
            &self.build_market_view(),
        )
    }

    pub(super) fn plan_execution_urgency_at_view(
        &self,
        plan: &TradingPlan,
        return_30min_bp: Option<i32>,
        return_1min_bp: Option<i32>,
        market_view: &MarketView,
    ) -> (UrgencyAssessment, RiskUrgencyAssessment) {
        let experience = self
            .state
            .belief_participants
            .get(&plan.account())
            .map(|participant| participant.belief())
            .unwrap_or_else(|| {
                panic!(
                    "plan account {:?} has no personal belief book",
                    plan.account()
                )
            })
            .experience();
        let risk = match experience.peak_equity {
            None => RiskUrgencyAssessment::Unavailable {
                reason: "no own-observed institution equity peak".to_owned(),
            },
            Some(peak) => {
                let account = &self.state.accounts[&plan.account()];
                let equity = account
                    .positions()
                    .iter()
                    .try_fold(account.cash(), |equity, (code, position)| {
                        equity.add(
                            market_view.stocks[code]
                                .last_price
                                .mul_shares(position.qty())?,
                        )
                    })
                    .unwrap_or_else(|error| {
                        panic!("institution equity observation failed: {error}")
                    });
                let positions = account
                    .positions()
                    .iter()
                    .map(|(code, position)| {
                        let price = market_view.stocks[code].last_price;
                        (
                            code.clone(),
                            crate::observation::RiskPositionInput {
                                qty: position.qty(),
                                cost_price: position.cost_price().filter(|cost| cost.cents() > 0),
                                last_price: price,
                                peak_price_since_entry: experience
                                    .stocks
                                    .get(code)
                                    .and_then(|stock| stock.peak_price_since_entry)
                                    .map(|observed| observed.max(price)),
                            },
                        )
                    })
                    .collect();
                // 本次自身权益也属于观察；不是全市场或其他机构共享的峰值。
                let observed = build_account_risk_observation(
                    account.cash(),
                    &positions,
                    experience.reference_equity,
                    Some(peak.max(equity)),
                )
                .unwrap_or_else(|error| panic!("institution risk observation failed: {error}"));
                let reducing = plan.direction() == Side::Sell
                    && plan.remaining_share_qty().is_some_and(|qty| qty > 0)
                    && account
                        .positions()
                        .get(plan.code())
                        .is_some_and(|position| position.qty() > 0);
                // 只提升已有卖出减仓计划的紧迫度，绝不按回撤新建卖出方向。
                assess_existing_reduction_urgency(reducing, &observed, &self.state.urgency_policy)
                    .unwrap_or_else(|error| {
                        panic!("plan risk urgency failed for {:?}: {error}", plan.plan_id())
                    })
            }
        };
        let (risk_reduction_active, account_drawdown_bp) = match &risk {
            RiskUrgencyAssessment::Assessed {
                risk_reduction_active,
                account_drawdown_bp,
                ..
            } => (*risk_reduction_active, *account_drawdown_bp),
            RiskUrgencyAssessment::Unavailable { .. } => (false, None),
        };
        let belief = &self.state.belief_participants[&plan.account()].belief();
        let behavior = self.institution_behavior(
            plan.account(),
            belief,
            plan.code(),
            market_view,
            match plan.status() {
                PlanStatus::Paused { reason } => Some(reason),
                _ => None,
            },
        );
        let inputs = UrgencyInputs {
            side: plan.direction(),
            return_30min_bp,
            return_1min_bp,
            risk_pressure_pause: plan.direction() == Side::Buy
                && behavior.risk_pressure_pause == Some(true),
            adverse_selection_pause: plan.direction() == Side::Buy
                && behavior.adverse_selection_pause,
            risk_reduction_active,
            account_drawdown_bp,
            remaining_trading_days: u32::try_from(
                plan.last_valid_trading_day()
                    .saturating_sub(u64::from(self.state.day)),
            )
            .expect("live plan remaining days fit its u32 horizon"),
            confidence_bp: plan.confidence_bp(),
            style: match self.belief_style(plan.account()) {
                Some(crate::strategy::InstitutionStyle::DeepValue) => PatienceStyle::DeepValue,
                _ => PatienceStyle::Other,
            },
        };
        let urgency = assess_urgency(&inputs, &self.state.urgency_policy).unwrap_or_else(|error| {
            panic!("plan urgency failed for {:?}: {error}", plan.plan_id())
        });
        (urgency, risk)
    }
}
