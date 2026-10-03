//! 判断层支持函数：基线动作、观察选股、当前持仓输入、经历置信度与动作落地。

use super::*;

pub(super) fn baseline_position_action(
    has_position: bool,
    rng: &mut dyn Rng,
) -> (PositionAction, DecisionReason) {
    if rng.next_f64() < 0.50 {
        (
            if has_position {
                PositionAction::Add
            } else {
                PositionAction::TryBuy
            },
            DecisionReason::BaselinePositioning,
        )
    } else if has_position {
        (PositionAction::Reduce, DecisionReason::BaselinePositioning)
    } else {
        (PositionAction::Watch, DecisionReason::BaselinePositioning)
    }
}

pub(super) fn select_observed_stock(
    market: &MarketView,
    own: &SelfView,
    experience: Option<&RetailExperienceState>,
    rng: &mut dyn Rng,
) -> Option<StockCode> {
    // 60% 优先查看持仓，40% 仍能从全市场发现股票；每个账户独立抽样。
    if !own.positions.is_empty() && rng.next_f64() < 0.60 {
        let index = rng.next_range_u32(0, own.positions.len() as u32) as usize;
        return own.positions.keys().nth(index).cloned();
    }
    if let Some(experience) = experience {
        let watched: Vec<_> = experience
            .stocks
            .keys()
            .filter(|code| market.stocks.contains_key(*code))
            .cloned()
            .collect();
        if !watched.is_empty() && rng.next_f64() < 0.70 {
            let index = rng.next_range_u32(0, watched.len() as u32) as usize;
            return watched.get(index).cloned();
        }
    }
    if market.stocks.is_empty() {
        return None;
    }
    let index = rng.next_range_u32(0, market.stocks.len() as u32) as usize;
    market.stocks.keys().nth(index).cloned()
}

/// 单次仓位目标推导的借用上下文；权威持仓、权益和行情仍由输入 owner 保存。
pub(super) struct RetailPositionDecisionContext<'a> {
    code: &'a StockCode,
    market: &'a MarketView,
    account_risk: &'a AccountRiskObservation,
    pub(super) current_fraction: f64,
    current_qty: u32,
    sellable_qty: u32,
}

impl<'a> RetailPositionDecisionContext<'a> {
    pub(super) fn from_observations(
        code: &'a StockCode,
        own: &SelfView,
        market: &'a MarketView,
        account_risk: &'a AccountRiskObservation,
    ) -> Self {
        let Some(position) = own.positions.get(code) else {
            return Self::from_position_inputs(code, market, account_risk, 0.0, 0, 0);
        };
        let weight = account_risk
            .positions
            .get(code)
            .unwrap_or_else(|| panic!("account-risk observation is missing held stock {}", code.0))
            .equity_weight
            .unwrap_or_else(|| panic!("held stock {} is missing its equity weight", code.0));
        Self::from_position_inputs(
            code,
            market,
            account_risk,
            weight,
            position.qty,
            position.sellable_qty,
        )
    }

    pub(super) fn from_position_inputs(
        code: &'a StockCode,
        market: &'a MarketView,
        account_risk: &'a AccountRiskObservation,
        current_fraction: f64,
        current_qty: u32,
        sellable_qty: u32,
    ) -> Self {
        Self {
            code,
            market,
            account_risk,
            current_fraction,
            current_qty,
            sellable_qty,
        }
    }

    pub(super) fn apply_experience_confidence(
        &self,
        decision: PositionDecision,
        experience: Option<&RetailExperienceState>,
        position_step_fraction: f64,
        rng: &mut dyn Rng,
    ) -> PositionDecision {
        let Some(experience) = experience else {
            return decision;
        };
        if !matches!(
            decision.action,
            PositionAction::TryBuy | PositionAction::Add
        ) || experience.consecutive_failed_buys < 2
        {
            return decision;
        }
        let retry_probability = 1.0 / (f64::from(experience.consecutive_failed_buys) + 1.0);
        if rng.next_f64() < retry_probability {
            return decision;
        }
        decision
            .code
            .as_ref()
            .expect("buy decision must identify its stock");
        self.target_for(
            if self.current_qty > 0 {
                PositionAction::Hold
            } else {
                PositionAction::Watch
            },
            DecisionReason::LowConfidence,
            position_step_fraction,
        )
    }

    pub(super) fn target_for(
        &self,
        mut action: PositionAction,
        mut reason: DecisionReason,
        position_step_fraction: f64,
    ) -> PositionDecision {
        let code = self.code;
        let current_fraction = self.current_fraction;
        let current_qty = self.current_qty;
        let sellable_qty = self.sellable_qty;
        let market = self.market;
        let account_risk = self.account_risk;
        // 试探买入使用较小步幅；加仓累积到本次目标，不形成永久的单股仓位上限。
        let mut target_fraction = match action {
            PositionAction::Hold | PositionAction::Watch => current_fraction,
            PositionAction::TryBuy => position_step_fraction * 0.80,
            PositionAction::Add => (current_fraction + position_step_fraction).min(1.0),
            PositionAction::Reduce => current_fraction * 0.50,
            PositionAction::Exit => 0.0,
        };
        let stock = market
            .stocks
            .get(code)
            .unwrap_or_else(|| panic!("selected stock {} is absent from market view", code.0));
        let raw_target_qty = if stock.last_price.cents() > 0 && account_risk.equity.cents() > 0 {
            let target_value = account_risk.equity.cents() as f64 * target_fraction;
            let raw_qty = (target_value / stock.last_price.cents() as f64).floor();
            raw_qty.max(0.0).min(u32::MAX as f64) as u32
        } else {
            0
        };
        let target_qty = match action {
            PositionAction::TryBuy | PositionAction::Add => {
                // 买入只能增加整手，已有零股余数必须保留；取不超过本次目标的最大可达持仓。
                let remainder = current_qty % 100;
                if raw_target_qty < remainder {
                    remainder
                } else {
                    remainder + ((raw_target_qty - remainder) / 100) * 100
                }
            }
            PositionAction::Hold | PositionAction::Watch => current_qty,
            PositionAction::Reduce => {
                // 减仓后持仓保留原有零股余数；只有整手可卖时，不把 Reduce 扩大成 Exit。
                let remainder = current_qty % 100;
                if raw_target_qty < remainder {
                    current_qty
                } else {
                    remainder + ((raw_target_qty - remainder) / 100) * 100
                }
            }
            PositionAction::Exit => 0,
        };
        if !matches!(action, PositionAction::Hold | PositionAction::Watch)
            && account_risk.equity.cents() > 0
        {
            target_fraction = target_qty as f64 * stock.last_price.cents() as f64
                / account_risk.equity.cents() as f64;
        }
        let mut desired_delta = i64::from(target_qty) - i64::from(current_qty);
        let mut executable_delta = desired_delta;
        match action {
            PositionAction::Hold | PositionAction::Watch => {
                desired_delta = 0;
                executable_delta = 0;
            }
            PositionAction::TryBuy | PositionAction::Add if desired_delta <= 0 => {
                action = PositionAction::Hold;
                target_fraction = current_fraction;
                desired_delta = 0;
                executable_delta = 0;
            }
            PositionAction::Reduce | PositionAction::Exit if desired_delta >= 0 => {
                action = PositionAction::Hold;
                target_fraction = current_fraction;
                desired_delta = 0;
                executable_delta = 0;
            }
            PositionAction::Reduce | PositionAction::Exit if sellable_qty == 0 => {
                reason = DecisionReason::T1Locked;
                executable_delta = 0;
            }
            PositionAction::Reduce | PositionAction::Exit => {
                executable_delta = -((-desired_delta).min(i64::from(sellable_qty)));
            }
            _ => {}
        }
        PositionDecision {
            code: Some(code.clone()),
            action,
            reason,
            target_position_fraction: target_fraction,
            desired_delta_shares: desired_delta,
            executable_delta_shares: executable_delta,
        }
    }
}

#[cfg(test)]
mod position_context_tests {
    use super::*;
    use crate::strategy::StockView;
    use crate::Money;

    fn inputs() -> (StockCode, MarketView, AccountRiskObservation) {
        let code = StockCode("600101".to_owned());
        let market = MarketView {
            stocks: [(
                code.clone(),
                StockView {
                    best_bid: None,
                    best_ask: None,
                    last_price: Money::from_cents(100),
                    max_buy_price: Money::from_cents(110),
                    daily_upper_limit: Money::from_cents(110),
                    min_sell_price: Money::from_cents(90),
                    recent_prices: vec![],
                    recent_market_minute_prices: vec![],
                    relative_volume: 1.0,
                    order_book_imbalance: 0.0,
                },
            )]
            .into(),
            tick: 0,
            market_minute: 0,
        };
        let risk = AccountRiskObservation {
            equity: Money::from_cents(100_000),
            return_from_reference: None,
            drawdown_from_peak: None,
            positions: BTreeMap::new(),
        };
        (code, market, risk)
    }

    #[test]
    fn position_targets_preserve_odd_lots_and_t1_intent_execution_split() {
        let (code, market, risk) = inputs();
        let target = |action, qty, sellable, fraction| {
            RetailPositionDecisionContext::from_position_inputs(
                &code, &market, &risk, fraction, qty, sellable,
            )
            .target_for(action, DecisionReason::NoSignal, 0.25)
        };
        let buy = target(PositionAction::TryBuy, 0, 0, 0.0);
        assert_eq!(
            (buy.desired_delta_shares, buy.executable_delta_shares),
            (200, 200)
        );
        let add = target(PositionAction::Add, 155, 155, 0.155);
        assert_eq!(
            (add.desired_delta_shares, add.executable_delta_shares),
            (200, 200)
        );
        let reduce = target(PositionAction::Reduce, 355, 100, 0.355);
        assert_eq!(
            (reduce.desired_delta_shares, reduce.executable_delta_shares),
            (-200, -100)
        );
        let exit = target(PositionAction::Exit, 355, 0, 0.355);
        assert_eq!(
            (exit.desired_delta_shares, exit.executable_delta_shares),
            (-355, 0)
        );
        assert_eq!(exit.reason, DecisionReason::T1Locked);
        let odd_lot = target(PositionAction::Reduce, 55, 55, 0.055);
        assert_eq!(odd_lot.action, PositionAction::Hold);
        assert_eq!(odd_lot.desired_delta_shares, 0);
    }
    struct CountingRng {
        value: f64,
        draws: usize,
    }

    impl Rng for CountingRng {
        fn next_f64(&mut self) -> f64 {
            self.draws += 1;
            self.value
        }
        fn next_range_u32(&mut self, _lo: u32, _hi: u32) -> u32 {
            panic!("仓位信心转换不应抽取股票索引")
        }
    }

    #[test]
    fn confidence_only_consumes_one_draw_for_eligible_buy_and_preserves_hold_watch() {
        let (code, market, risk) = inputs();
        let mut experience = RetailExperienceState::without_equity_reference();
        experience.consecutive_failed_buys = 3;
        let mut rng = CountingRng {
            value: 0.25,
            draws: 0,
        };
        for (qty, fraction, buy_action, fallback) in [
            (0, 0.0, PositionAction::TryBuy, PositionAction::Watch),
            (155, 0.155, PositionAction::Add, PositionAction::Hold),
        ] {
            let context = RetailPositionDecisionContext::from_position_inputs(
                &code, &market, &risk, fraction, qty, qty,
            );
            let buy = context.target_for(buy_action, DecisionReason::NoSignal, 0.25);
            let draws_before = rng.draws;
            assert_eq!(
                context.apply_experience_confidence(buy.clone(), None, 0.25, &mut rng),
                buy
            );
            let inactive = context.target_for(fallback, DecisionReason::NoSignal, 0.25);
            assert_eq!(
                context.apply_experience_confidence(
                    inactive.clone(),
                    Some(&experience),
                    0.25,
                    &mut rng
                ),
                inactive
            );
            assert_eq!(rng.draws, draws_before);
            let result =
                context.apply_experience_confidence(buy, Some(&experience), 0.25, &mut rng);
            assert_eq!(rng.draws, draws_before + 1);
            assert_eq!(result.action, fallback);
            assert_eq!(result.reason, DecisionReason::LowConfidence);
            assert_eq!(result.target_position_fraction, fraction);
            assert_eq!(
                (result.desired_delta_shares, result.executable_delta_shares),
                (0, 0)
            );
        }
    }
    #[test]
    fn unusable_price_or_equity_preserves_existing_target_fallback_and_risk_guard() {
        let (code, market, risk) = inputs();
        for (price, equity) in [(100, 0), (100, -1), (0, 100_000), (-1, 100_000)] {
            let mut market = market.clone();
            market.stocks.get_mut(&code).unwrap().last_price = Money::from_cents(price);
            let mut risk = risk.clone();
            risk.equity = Money::from_cents(equity);
            let context = RetailPositionDecisionContext::from_position_inputs(
                &code, &market, &risk, 0.155, 155, 50,
            );
            let add = context.target_for(PositionAction::Add, DecisionReason::NoSignal, 0.25);
            assert_eq!(add.action, PositionAction::Hold);
            assert_eq!(add.target_position_fraction, 0.155);
            assert_eq!(
                (add.desired_delta_shares, add.executable_delta_shares),
                (0, 0)
            );
            let exit = context.target_for(PositionAction::Exit, DecisionReason::NoSignal, 0.25);
            assert_eq!(
                (exit.desired_delta_shares, exit.executable_delta_shares),
                (-155, -50)
            );
        }
        let own = SelfView {
            cash: Money::ZERO,
            positions: [(
                code.clone(),
                crate::strategy::PositionView {
                    qty: 155,
                    sellable_qty: 50,
                    cost_price: None,
                },
            )]
            .into(),
        };
        assert!(
            std::panic::catch_unwind(|| RetailPositionDecisionContext::from_observations(
                &code, &own, &market, &risk
            ))
            .is_err()
        );
        let mut risk = risk;
        risk.positions.insert(
            code.clone(),
            crate::observation::PositionRiskObservation {
                market_value: Money::from_cents(15_500),
                unrealized_return: None,
                equity_weight: None,
                drawdown_from_position_peak: None,
            },
        );
        assert!(
            std::panic::catch_unwind(|| RetailPositionDecisionContext::from_observations(
                &code, &own, &market, &risk
            ))
            .is_err()
        );
    }
}
