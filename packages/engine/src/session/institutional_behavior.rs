use crate::experience::{ExperienceMoment, RetailExperienceState};
use crate::plans::{
    experience_cost_signal, PauseReason, SignalContribution, SignalUnavailableReason,
};
use crate::strategy::{InstitutionExperiencePolicy, InstitutionLossResponse};
use crate::{Money, StockCode};

pub(super) fn observe_institution_account_risk(
    belief: &mut crate::strategy::BeliefBook,
    equity: Money,
    moment: ExperienceMoment,
) -> Result<(), crate::experience::ExperienceError> {
    belief.observe_institution_account_risk(equity, moment)
}

pub(super) fn observe_institution_position(
    experience: &mut RetailExperienceState,
    code: &StockCode,
    price: Money,
    moment: ExperienceMoment,
    policy: &InstitutionExperiencePolicy,
) -> Result<(), crate::experience::ExperienceError> {
    experience.observe_institution_position_dated(
        code,
        price,
        moment,
        policy.adverse_move_threshold_bp(),
    )
}

impl crate::GameSession {
    pub(super) fn institution_behavior(
        &self,
        account: crate::AccountId,
        belief: &crate::strategy::BeliefBook,
        code: &StockCode,
        view: &crate::strategy::MarketView,
        paused: Option<PauseReason>,
    ) -> InstitutionBehavior {
        let own = &self.state.accounts[&account];
        let policy = belief
            .institution_policy()
            .expect("institution book has its frozen behavior policy");
        assess_institution_behavior(
            policy,
            belief.experience(),
            code,
            own.positions()
                .get(code)
                .and_then(crate::Position::cost_price),
            view.stocks[code].last_price,
            ExperienceMoment {
                civil_date: self.civil_date(),
                market_minute: self.current_market_minute(),
                trading_day: u64::from(self.state.day),
            },
            paused,
            if belief.institution_account_risk_paused() {
                Some(true)
            } else {
                belief.experience().peak_equity.map(|_| false)
            },
        )
        .unwrap_or_else(|error| panic!("institution {account:?} personal behavior failed: {error}"))
    }
}

pub(super) struct InstitutionBehavior {
    pub cost_signal: SignalContribution,
    pub risk_pressure_pause: Option<bool>,
    pub adverse_selection_pause: bool,
}

#[allow(clippy::too_many_arguments)]
pub(in crate::session) fn assess_institution_behavior(
    policy: &InstitutionExperiencePolicy,
    experience: &RetailExperienceState,
    code: &StockCode,
    cost: Option<Money>,
    price: Money,
    moment: ExperienceMoment,
    paused: Option<PauseReason>,
    account_risk_pause: Option<bool>,
) -> Result<InstitutionBehavior, crate::experience::ExperienceError> {
    if price.cents() <= 0 {
        return Err(crate::experience::ExperienceError::NonPositiveMoney {
            field: "institution price",
            cents: price.cents(),
        });
    }
    if let Some(peak) = experience.peak_equity.filter(|peak| peak.cents() <= 0) {
        return Err(crate::experience::ExperienceError::NonPositiveMoney {
            field: "institution equity peak",
            cents: peak.cents(),
        });
    }
    // 成本动作是游戏候选，不覆盖基本面/趋势，也不直接生成订单或强制止损。
    let cost = cost.filter(|value| value.cents() > 0);
    let at_loss = cost.is_some_and(|reference| {
        i128::from(reference.cents() - price.cents()) * 10_000
            >= i128::from(reference.cents()) * i128::from(policy.cost_loss_threshold_bp())
    });
    let cost_signal = match cost {
        None => SignalContribution::unavailable(SignalUnavailableReason::MissingObservation),
        Some(reference) => {
            let action = if at_loss && policy.loss_response() == InstitutionLossResponse::HoldOrAdd
            {
                crate::behavior::PositionAction::Add
            } else if at_loss {
                crate::behavior::PositionAction::Watch
            } else if i128::from(price.cents() - reference.cents()) * 10_000
                >= i128::from(reference.cents()) * i128::from(policy.cost_profit_threshold_bp())
            {
                crate::behavior::PositionAction::Reduce
            } else {
                crate::behavior::PositionAction::Hold
            };
            experience_cost_signal(action)
        }
    };
    let cost_pressure_threshold = if paused == Some(PauseReason::RiskPressure) {
        policy.cost_loss_threshold_bp() / 2
    } else {
        policy.cost_loss_threshold_bp()
    };
    let cost_pressure = policy.loss_response() == InstitutionLossResponse::PauseAndReview
        && cost.is_some_and(|reference| {
            let decline = i128::from(reference.cents() - price.cents()) * 10_000;
            let boundary = i128::from(reference.cents()) * i128::from(cost_pressure_threshold);
            if paused == Some(PauseReason::RiskPressure) {
                decline > boundary
            } else {
                decline >= boundary
            }
        });
    experience.feedback.ensure_as_of_reached(&moment)?;
    let risk_pressure_pause = if cost_pressure {
        Some(true)
    } else {
        account_risk_pause
    };
    // 没有真实买入订单身份时，没有“成交后不利选择”，不能用挂单冒充成交。
    let adverse_selection_pause = experience.stocks.get(code).is_some_and(|stock| {
        stock.last_buy_order_id.is_some()
            && stock.last_buy_price.is_some_and(|buy| {
                (i128::from(buy.cents()) - i128::from(price.cents())) * 10_000
                    >= i128::from(buy.cents()) * i128::from(policy.adverse_move_threshold_bp())
            })
    });
    Ok(InstitutionBehavior {
        cost_signal,
        risk_pressure_pause,
        adverse_selection_pause,
    })
}

#[cfg(test)]
mod tests {
    use crate::plans::{PauseReason, SignalScore};
    use crate::strategy::{InstitutionExperiencePolicy, InstitutionLossResponse};
    use crate::{Money, RetailExperienceState, StockCode};

    fn policy(response: InstitutionLossResponse) -> InstitutionExperiencePolicy {
        InstitutionExperiencePolicy::new(1, response, 800, 1200, 3000, 1500, 3, 500).unwrap()
    }

    fn account_risk(
        policy: &InstitutionExperiencePolicy,
        experience: &RetailExperienceState,
        equity: Money,
        moment: crate::experience::ExperienceMoment,
        paused: bool,
    ) -> Result<Option<bool>, crate::experience::ExperienceError> {
        let profile = crate::strategy::StrategyProfile::Institution(
            crate::strategy::InstitutionStyle::DeepValue,
        );
        let mut rng = crate::session::SplitMix64::new(42);
        let analysis =
            crate::strategy::derive_analysis_profile(&profile, crate::AccountId(1), &mut rng)
                .unwrap();
        let mut belief =
            crate::strategy::BeliefBook::new(crate::AccountId(1), profile, analysis, &mut rng);
        *belief.experience_mut() = experience.clone();
        belief.set_institution_policy(policy.clone());
        let mut encoded = serde_json::to_value(&belief).unwrap();
        encoded["institution_account_risk_paused"] = serde_json::json!(paused);
        let mut belief: crate::strategy::BeliefBook = serde_json::from_value(encoded).unwrap();
        super::observe_institution_account_risk(&mut belief, equity, moment)?;
        Ok(if belief.institution_account_risk_paused() {
            Some(true)
        } else {
            experience.peak_equity.map(|_| false)
        })
    }

    fn observe(
        response: InstitutionLossResponse,
        price: i64,
        equity: i64,
        paused: Option<PauseReason>,
        real_buy: bool,
    ) -> super::InstitutionBehavior {
        let code = StockCode("600101".to_owned());
        let mut experience = RetailExperienceState::new(Money::from_cents(10_000)).unwrap();
        if real_buy {
            experience.stocks.insert(
                code.clone(),
                crate::experience::RetailStockExperience {
                    last_buy_price: Some(Money::from_cents(1000)),
                    last_buy_order_id: Some(1),
                    ..Default::default()
                },
            );
        }
        let observed = crate::experience::ExperienceMoment {
            civil_date: crate::CivilDate::from_iso("2030-01-07").unwrap(),
            market_minute: 2,
            trading_day: 0,
        };
        let account_risk = account_risk(
            &policy(response),
            &experience,
            Money::from_cents(equity),
            observed,
            paused == Some(PauseReason::RiskPressure),
        )
        .unwrap();
        super::assess_institution_behavior(
            &policy(response),
            &experience,
            &code,
            Some(Money::from_cents(1000)),
            Money::from_cents(price),
            observed,
            paused,
            account_risk,
        )
        .unwrap()
    }

    #[test]
    fn institution_cost_loss_response_is_individual_not_a_universal_stop_loss() {
        let patient = observe(InstitutionLossResponse::HoldOrAdd, 900, 10_000, None, false);
        let cautious = observe(
            InstitutionLossResponse::PauseAndReview,
            900,
            10_000,
            None,
            false,
        );
        assert_eq!(
            patient.cost_signal.score(),
            Some(SignalScore::new(5000).unwrap())
        );
        assert_eq!(patient.risk_pressure_pause, Some(false));
        assert_eq!(
            cautious.cost_signal.score(),
            Some(SignalScore::new(0).unwrap())
        );
        assert_eq!(cautious.risk_pressure_pause, Some(true));
    }

    #[test]
    fn institution_risk_pressure_has_separate_trigger_and_recovery_thresholds() {
        assert_eq!(
            observe(InstitutionLossResponse::HoldOrAdd, 1000, 7000, None, false)
                .risk_pressure_pause,
            Some(true)
        );
        assert_eq!(
            observe(
                InstitutionLossResponse::HoldOrAdd,
                1000,
                8000,
                Some(PauseReason::RiskPressure),
                false
            )
            .risk_pressure_pause,
            Some(true)
        );
        assert_eq!(
            observe(
                InstitutionLossResponse::HoldOrAdd,
                1000,
                8500,
                Some(PauseReason::RiskPressure),
                false
            )
            .risk_pressure_pause,
            Some(false)
        );
    }

    #[test]
    fn institution_adverse_selection_requires_a_real_buy_identity() {
        assert!(
            !observe(InstitutionLossResponse::HoldOrAdd, 950, 10_000, None, false)
                .adverse_selection_pause
        );
        assert!(
            observe(InstitutionLossResponse::HoldOrAdd, 950, 10_000, None, true)
                .adverse_selection_pause
        );
        assert!(
            !observe(InstitutionLossResponse::HoldOrAdd, 951, 10_000, None, true)
                .adverse_selection_pause
        );
    }

    #[test]
    fn institution_risk_counts_real_failures_and_decays_without_deleting_facts() {
        let code = StockCode("600101".to_owned());
        let mut experience = RetailExperienceState::new(Money::from_cents(10_000)).unwrap();
        for order in 1..=3 {
            experience
                .feedback
                .failure_events
                .push(crate::experience::FailureEventRecord {
                    code: code.clone(),
                    order_id: Some(order),
                    moment: moment(order),
                });
        }
        let assess = |day| {
            let observed = crate::experience::ExperienceMoment {
                trading_day: day,
                ..moment(4)
            };
            let account_risk = account_risk(
                &policy(InstitutionLossResponse::HoldOrAdd),
                &experience,
                Money::from_cents(10_000),
                observed,
                false,
            )
            .unwrap();
            super::assess_institution_behavior(
                &policy(InstitutionLossResponse::HoldOrAdd),
                &experience,
                &code,
                Some(Money::from_cents(1_000)),
                Money::from_cents(1_000),
                observed,
                None,
                account_risk,
            )
            .unwrap()
        };
        assert_eq!(experience.consecutive_failed_buys, 0);
        assert_eq!(assess(0).risk_pressure_pause, Some(true));
        assert_eq!(assess(19).risk_pressure_pause, Some(true));
        assert_eq!(assess(20).risk_pressure_pause, Some(false));
        assert_eq!(experience.feedback.failure_events.len(), 3);
    }

    #[test]
    fn institution_position_observation_uses_individual_adverse_threshold_once() {
        let code = StockCode("600101".to_owned());
        let mut experience = RetailExperienceState::new(Money::from_cents(10_000)).unwrap();
        experience.stocks.insert(
            code.clone(),
            crate::experience::RetailStockExperience {
                entry_reference_price: Some(Money::from_cents(1000)),
                peak_price_since_entry: Some(Money::from_cents(1100)),
                last_buy_price: Some(Money::from_cents(1000)),
                last_buy_order_id: Some(42),
                ..Default::default()
            },
        );
        experience.feedback.stocks.insert(
            code.clone(),
            crate::experience::HoldingEpoch {
                entry_moment: moment(1),
                last_own_observation: None,
                institutional_fees_paid: Some(Money::ZERO),
            },
        );
        let policy = InstitutionExperiencePolicy::new(
            1,
            InstitutionLossResponse::HoldOrAdd,
            800,
            1200,
            3000,
            1500,
            3,
            800,
        )
        .unwrap();

        super::observe_institution_position(
            &mut experience,
            &code,
            Money::from_cents(950),
            moment(2),
            &policy,
        )
        .unwrap();
        assert!(!experience.stocks[&code].adverse_move_recorded);
        assert!(experience.feedback.failure_events.is_empty());

        super::observe_institution_position(
            &mut experience,
            &code,
            Money::from_cents(920),
            moment(3),
            &policy,
        )
        .unwrap();
        assert!(experience.stocks[&code].adverse_move_recorded);
        assert_eq!(experience.feedback.failure_events.len(), 1);
        assert_eq!(
            experience.stocks[&code].peak_price_since_entry,
            Some(Money::from_cents(1100))
        );
        assert_eq!(experience.stocks[&code].last_observed_market_minute, 3);
        assert_eq!(
            experience.feedback.stocks[&code]
                .last_own_observation
                .unwrap()
                .price,
            Money::from_cents(920)
        );

        // A partial fill preserves the same order identity and failure marker.
        experience.stocks.get_mut(&code).unwrap().last_buy_order_id = Some(42);
        super::observe_institution_position(
            &mut experience,
            &code,
            Money::from_cents(900),
            moment(4),
            &policy,
        )
        .unwrap();
        assert_eq!(experience.feedback.failure_events.len(), 1);
        assert_eq!(experience.feedback.failure_events[0].order_id, Some(42));

        // The settlement writer clears the marker for a distinct buy order.
        let stock = experience.stocks.get_mut(&code).unwrap();
        stock.last_buy_order_id = Some(43);
        stock.last_buy_price = Some(Money::from_cents(1000));
        stock.adverse_move_recorded = false;
        super::observe_institution_position(
            &mut experience,
            &code,
            Money::from_cents(920),
            moment(5),
            &policy,
        )
        .unwrap();
        assert_eq!(experience.feedback.failure_events.len(), 2);
        assert_eq!(experience.feedback.failure_events[1].order_id, Some(43));
        assert_eq!(experience.feedback.latest_moment, Some(moment(5)));
    }

    fn moment(market_minute: u64) -> crate::experience::ExperienceMoment {
        crate::experience::ExperienceMoment {
            civil_date: crate::CivilDate::from_iso("2030-01-07").unwrap(),
            market_minute,
            trading_day: 0,
        }
    }
}
