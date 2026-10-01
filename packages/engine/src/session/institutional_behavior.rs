use crate::experience::{ExperienceMoment, RetailExperienceState};
use crate::plans::{
    experience_cost_signal, PauseReason, SignalContribution, SignalUnavailableReason,
};
use crate::strategy::{InstitutionExperiencePolicy, InstitutionLossResponse};
use crate::{Money, StockCode};

pub(super) fn observe_institution_position(
    experience: &mut RetailExperienceState,
    code: &StockCode,
    price: Money,
    moment: ExperienceMoment,
    policy: &InstitutionExperiencePolicy,
) -> Result<(), crate::experience::ExperienceError> {
    if price.cents() <= 0 {
        return Err(crate::experience::ExperienceError::NonPositiveMoney {
            field: "observed institution position price",
            cents: price.cents(),
        });
    }
    experience.feedback.ensure_moment_forward(moment)?;
    if !experience.feedback.stocks.contains_key(code) || !experience.stocks.contains_key(code) {
        return Err(crate::experience::ExperienceError::NoActiveEntry {
            code: code.0.clone(),
        });
    }

    let stock = experience.stocks.get(code);
    let confirms_failure = stock.is_some_and(|stock| {
        !stock.adverse_move_recorded
            && stock.last_buy_order_id.is_some()
            && stock.last_buy_price.is_some_and(|buy| {
                (i128::from(buy.cents()) - i128::from(price.cents())) * 10_000
                    >= i128::from(buy.cents()) * i128::from(policy.adverse_move_threshold_bp())
            })
    });
    let order_id = stock.and_then(|stock| stock.last_buy_order_id);

    let stock = experience
        .stocks
        .get_mut(code)
        .expect("stock experience checked above");
    stock.last_observed_market_minute = moment.market_minute;
    stock.peak_price_since_entry = Some(
        stock
            .peak_price_since_entry
            .map_or(price, |peak| peak.max(price)),
    );
    if confirms_failure {
        stock.adverse_move_recorded = true;
    }

    experience.feedback.advance_clocks(moment);
    experience
        .feedback
        .stocks
        .get_mut(code)
        .expect("active holding epoch checked above")
        .last_own_observation = Some(crate::experience::OwnObservation { price, moment });
    if confirms_failure {
        experience
            .feedback
            .failure_events
            .push(crate::experience::FailureEventRecord {
                code: code.clone(),
                order_id,
                moment,
            });
    }
    Ok(())
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
        let own = &self.accounts[&account];
        let equity = own
            .positions
            .iter()
            .try_fold(own.cash, |equity, (stock, position)| {
                equity.add(view.stocks[stock].last_price.mul_shares(position.qty)?)
            })
            .unwrap_or_else(|error| {
                panic!("institution {account:?} frozen equity failed: {error}")
            });
        let policy = belief
            .institution_policy()
            .expect("institution book has its frozen behavior policy");
        assess_institution_behavior(
            policy,
            belief.experience(),
            code,
            own.positions
                .get(code)
                .and_then(crate::Position::cost_price),
            view.stocks[code].last_price,
            equity,
            ExperienceMoment {
                civil_date: self.civil_date(),
                market_minute: self.current_market_minute(),
                trading_day: u64::from(self.day),
            },
            paused,
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
fn assess_institution_behavior(
    policy: &InstitutionExperiencePolicy,
    experience: &RetailExperienceState,
    code: &StockCode,
    cost: Option<Money>,
    price: Money,
    equity: Money,
    moment: ExperienceMoment,
    paused: Option<PauseReason>,
) -> Result<InstitutionBehavior, crate::experience::ExperienceError> {
    if price.cents() <= 0 || equity.cents() < 0 {
        return Err(crate::experience::ExperienceError::NonPositiveMoney {
            field: if price.cents() <= 0 {
                "institution price"
            } else {
                "institution equity"
            },
            cents: if price.cents() <= 0 {
                price.cents()
            } else {
                equity.cents()
            },
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
    let failure_decay = experience
        .feedback
        .failure_events
        .last()
        .map_or(0, |event| {
            (moment.trading_day - event.moment.trading_day)
                / crate::experience::FAILURE_DECAY_TRADING_DAYS
        });
    let failure_influence =
        (experience.feedback.failure_events.len() as u64).saturating_sub(failure_decay);
    let failed_pressure = failure_influence >= u64::from(policy.risk_pause_failed_buys());
    let drawdown_threshold = if paused == Some(PauseReason::RiskPressure) {
        policy.risk_resume_drawdown_bp()
    } else {
        policy.risk_pause_drawdown_bp()
    };
    let drawdown_pressure = experience.peak_equity.map(|peak| {
        let decline = i128::from(peak.cents()) - i128::from(equity.cents());
        let boundary = i128::from(peak.cents()) * i128::from(drawdown_threshold);
        if paused == Some(PauseReason::RiskPressure) {
            decline * 10_000 > boundary
        } else {
            decline * 10_000 >= boundary
        }
    });
    let risk_pressure_pause = if cost_pressure || failed_pressure {
        Some(true)
    } else {
        drawdown_pressure
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
        super::assess_institution_behavior(
            &policy(response),
            &experience,
            &code,
            Some(Money::from_cents(1000)),
            Money::from_cents(price),
            Money::from_cents(equity),
            crate::experience::ExperienceMoment {
                civil_date: crate::CivilDate::from_iso("2030-01-07").unwrap(),
                market_minute: 2,
                trading_day: 0,
            },
            paused,
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
            super::assess_institution_behavior(
                &policy(InstitutionLossResponse::HoldOrAdd),
                &experience,
                &code,
                Some(Money::from_cents(1_000)),
                Money::from_cents(1_000),
                Money::from_cents(10_000),
                crate::experience::ExperienceMoment {
                    trading_day: day,
                    ..moment(4)
                },
                None,
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
