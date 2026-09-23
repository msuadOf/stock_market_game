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

pub(super) fn current_position_inputs(
    code: &StockCode,
    own: &SelfView,
    account_risk: &AccountRiskObservation,
) -> (f64, u32, u32) {
    let Some(position) = own.positions.get(code) else {
        return (0.0, 0, 0);
    };
    let weight = account_risk
        .positions
        .get(code)
        .unwrap_or_else(|| panic!("account-risk observation is missing held stock {}", code.0))
        .equity_weight
        .unwrap_or_else(|| panic!("held stock {} is missing its equity weight", code.0));
    (weight, position.qty, position.sellable_qty)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn apply_experience_confidence(
    decision: PositionDecision,
    experience: Option<&RetailExperienceState>,
    (current_fraction, current_qty, sellable_qty): (f64, u32, u32),
    max_fraction: f64,
    market: &MarketView,
    account_risk: &AccountRiskObservation,
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
    let code = decision
        .code
        .as_ref()
        .expect("buy decision must identify its stock");
    decision_for_action(
        code,
        if current_qty > 0 {
            PositionAction::Hold
        } else {
            PositionAction::Watch
        },
        DecisionReason::LowConfidence,
        current_fraction,
        current_qty,
        sellable_qty,
        max_fraction,
        market,
        account_risk,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn decision_for_action(
    code: &StockCode,
    mut action: PositionAction,
    mut reason: DecisionReason,
    current_fraction: f64,
    current_qty: u32,
    sellable_qty: u32,
    max_fraction: f64,
    market: &MarketView,
    account_risk: &AccountRiskObservation,
) -> PositionDecision {
    let mut target_fraction = match action {
        PositionAction::Hold | PositionAction::Watch => current_fraction,
        PositionAction::TryBuy => max_fraction * 0.20,
        PositionAction::Add => (current_fraction + max_fraction * 0.25).min(max_fraction),
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
            // 买入只能增加整手，已有零股余数必须保留；取不超过风险目标的最大可达持仓。
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
