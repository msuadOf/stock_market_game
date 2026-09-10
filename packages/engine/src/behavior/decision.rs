//! 散户目标仓位判断内核 decide_retail_position_inner（B02/B03 + 经历叠加）。
//!
//! // allow: SIZE_OK — 单个遗留判断函数（~600 纯逻辑行）。W1-Task 3 只做纯移动，
//! // 拆分函数体涉及行为语义，属于后续行为任务；此处保持逐字不变。

use super::*;

use super::heuristics::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn decide_retail_position_inner(
    strategy: &StrategyData,
    style: RetailStyle,
    market: &MarketView,
    own: &SelfView,
    observations: &BehaviorMarketObservation,
    account_risk: &AccountRiskObservation,
    experience: Option<&RetailExperienceState>,
    market_minute: u64,
    rng: &mut dyn Rng,
) -> PositionDecision {
    assert!(
        strategy.stop_loss_threshold.is_finite() && strategy.stop_loss_threshold > 0.0,
        "validated retail strategy must have a positive finite stop-loss threshold"
    );
    assert!(
        strategy.take_profit_threshold.is_finite() && strategy.take_profit_threshold > 0.0,
        "validated retail strategy must have a positive finite take-profit threshold"
    );
    assert!(
        strategy.max_stock_fraction.is_finite()
            && (0.0..=1.0).contains(&strategy.max_stock_fraction),
        "validated retail strategy must have max_stock_fraction in [0,1]"
    );
    assert!(
        strategy.arrival_rate.is_finite() && (0.0..=1.0).contains(&strategy.arrival_rate),
        "validated retail strategy must have arrival_rate in [0,1]"
    );
    assert!(
        strategy.dip_threshold.is_finite() && strategy.dip_threshold > 0.0,
        "validated retail strategy must have a positive finite dip threshold"
    );
    assert!(
        strategy.volume_confirmation.is_finite() && strategy.volume_confirmation >= 0.0,
        "validated retail strategy must have a non-negative finite volume confirmation"
    );

    let broad_stress = observations
        .thirty_minute_market
        .decline_fraction
        .is_some_and(|fraction| fraction >= 0.60)
        && observations
            .thirty_minute_market
            .equal_weight_return
            .is_some_and(|value| value < 0.0);
    let universe_floor = if market.stocks.is_empty() {
        0.0
    } else {
        1.0 / market.stocks.len() as f64
    };
    let effective_max_fraction = strategy.max_stock_fraction.max(universe_floor).min(1.0);

    // 账户回撤是独立于单股成本的风险事实：单一仓位可能已经反弹甚至微盈，但账户仍可能
    // 远低于其真实净值峰值。以该个体自身的止损尺度的两倍作为第一道整体去风险门槛，
    // 避免把所有轻微波动都解释为账户危机。不同风格保留不同的行动纪律（见下方）。
    let account_drawdown_pressure = account_risk
        .drawdown_from_peak
        .filter(|drawdown| drawdown.is_finite())
        .map_or(0.0, |drawdown| {
            (-drawdown / (strategy.stop_loss_threshold * 2.0)).max(0.0)
        });

    // 风险判断先于行情分支和随机到达。选择相对个人阈值压力最大的真实持仓，
    // 因而深亏后的横盘或微反弹不会把已经越线的风险隐藏掉。
    let risk_candidate = own
        .positions
        .iter()
        .filter_map(|(code, position)| {
            let risk = account_risk.positions.get(code).unwrap_or_else(|| {
                panic!("account-risk observation is missing held stock {}", code.0)
            });
            // 零或负净成本持仓没有合法的成本收益率；保留持仓权重，但不伪造 PnL 信号。
            let pnl = risk.unrealized_return?;
            let weight = risk
                .equity_weight
                .unwrap_or_else(|| panic!("held stock {} is missing its equity weight", code.0));
            let loss_pressure = (-pnl / strategy.stop_loss_threshold).max(0.0);
            let profit_pressure = (pnl / strategy.take_profit_threshold).max(0.0);
            let broad_pressure = if broad_stress && weight >= 0.50 && pnl < 0.0 {
                (-pnl / (strategy.stop_loss_threshold * 0.50)).max(0.0)
            } else {
                0.0
            };
            let giveback_pressure = risk
                .drawdown_from_position_peak
                .filter(|_| pnl > 0.0)
                .map_or(0.0, |drawdown| (-drawdown / 0.05).max(0.0));
            let pressure = loss_pressure
                .max(profit_pressure)
                .max(broad_pressure)
                .max(giveback_pressure);
            (pressure >= 1.0).then_some((
                code,
                position,
                pnl,
                weight,
                pressure,
                broad_pressure,
                giveback_pressure,
            ))
        })
        .max_by(|left, right| {
            (left.1.sellable_qty > 0)
                .cmp(&(right.1.sellable_qty > 0))
                .then_with(|| {
                    left.4
                        .partial_cmp(&right.4)
                        .expect("validated risk observations are finite")
                })
                .then_with(|| right.0.cmp(left.0))
        });

    if let Some((code, position, pnl, weight, _pressure, broad_pressure, giveback_pressure)) =
        risk_candidate
    {
        let (action, reason) = if giveback_pressure >= 1.0 {
            let action = match style {
                RetailStyle::Panic | RetailStyle::Momentum | RetailStyle::Noise => {
                    PositionAction::Reduce
                }
                RetailStyle::DipBuyer | RetailStyle::LongTerm => {
                    if rng.next_f64() < 0.40 {
                        PositionAction::Hold
                    } else {
                        PositionAction::Reduce
                    }
                }
                RetailStyle::Dormant => {
                    if rng.next_f64() < 0.90 {
                        PositionAction::Hold
                    } else {
                        PositionAction::Reduce
                    }
                }
            };
            (action, DecisionReason::ProfitGiveback)
        } else if broad_pressure >= 1.0 && pnl > -strategy.stop_loss_threshold {
            match style {
                RetailStyle::Panic | RetailStyle::Momentum => {
                    (PositionAction::Reduce, DecisionReason::BroadMarketRisk)
                }
                _ => (PositionAction::Hold, DecisionReason::BroadMarketRisk),
            }
        } else if pnl <= -strategy.stop_loss_threshold {
            let action = match style {
                RetailStyle::Panic => PositionAction::Exit,
                RetailStyle::Momentum => PositionAction::Reduce,
                RetailStyle::Noise => {
                    if rng.next_f64() < 0.70 {
                        PositionAction::Reduce
                    } else {
                        PositionAction::Hold
                    }
                }
                RetailStyle::DipBuyer => {
                    if broad_stress {
                        PositionAction::Reduce
                    } else if rng.next_f64() < 0.80 {
                        PositionAction::Add
                    } else {
                        PositionAction::Reduce
                    }
                }
                RetailStyle::LongTerm => {
                    if rng.next_f64() < 0.95 {
                        PositionAction::Hold
                    } else {
                        PositionAction::Reduce
                    }
                }
                RetailStyle::Dormant => {
                    if rng.next_f64() < 0.98 {
                        PositionAction::Hold
                    } else {
                        PositionAction::Reduce
                    }
                }
            };
            (action, DecisionReason::PositionRisk)
        } else {
            let action = match style {
                RetailStyle::Momentum | RetailStyle::Panic => PositionAction::Exit,
                RetailStyle::Noise | RetailStyle::DipBuyer => PositionAction::Reduce,
                RetailStyle::LongTerm => {
                    if rng.next_f64() < 0.90 {
                        PositionAction::Hold
                    } else {
                        PositionAction::Reduce
                    }
                }
                RetailStyle::Dormant => {
                    if rng.next_f64() < 0.98 {
                        PositionAction::Hold
                    } else {
                        PositionAction::Reduce
                    }
                }
            };
            (action, DecisionReason::TakeProfit)
        };
        let decision = decision_for_action(
            code,
            action,
            reason,
            weight,
            position.qty,
            position.sellable_qty,
            effective_max_fraction,
            market,
            account_risk,
        );
        return apply_experience_confidence(
            decision,
            experience,
            current_position_inputs(code, own, account_risk),
            effective_max_fraction,
            market,
            account_risk,
            rng,
        );
    }

    if account_drawdown_pressure >= 1.0 {
        // 在没有更紧急的单股止损、止盈或浮盈回吐时，优先处理可卖且权重最高的真实持仓。
        // 这不是强制所有人卖出：风格只决定如何解释同一回撤，成交仍经过 T+1 与订单簿。
        let drawdown_candidate = own
            .positions
            .iter()
            .filter_map(|(code, position)| {
                let weight = account_risk
                    .positions
                    .get(code)
                    .unwrap_or_else(|| {
                        panic!("account-risk observation is missing held stock {}", code.0)
                    })
                    .equity_weight
                    .unwrap_or_else(|| {
                        panic!("held stock {} is missing its equity weight", code.0)
                    });
                assert!(
                    weight.is_finite() && weight > 0.0,
                    "held stock {} has invalid equity weight {weight}",
                    code.0
                );
                Some((code, position, weight))
            })
            .max_by(|left, right| {
                (left.1.sellable_qty > 0)
                    .cmp(&(right.1.sellable_qty > 0))
                    .then_with(|| {
                        left.2
                            .partial_cmp(&right.2)
                            .expect("validated equity weights are finite")
                    })
                    .then_with(|| right.0.cmp(left.0))
            });
        if let Some((code, position, weight)) = drawdown_candidate {
            let action = match style {
                RetailStyle::Panic => PositionAction::Exit,
                RetailStyle::Momentum => PositionAction::Reduce,
                RetailStyle::Noise => {
                    if rng.next_f64() < 0.70 {
                        PositionAction::Reduce
                    } else {
                        PositionAction::Hold
                    }
                }
                RetailStyle::DipBuyer => {
                    if rng.next_f64() < 0.65 {
                        PositionAction::Hold
                    } else {
                        PositionAction::Reduce
                    }
                }
                RetailStyle::LongTerm => {
                    if rng.next_f64() < 0.90 {
                        PositionAction::Hold
                    } else {
                        PositionAction::Reduce
                    }
                }
                RetailStyle::Dormant => {
                    if rng.next_f64() < 0.97 {
                        PositionAction::Hold
                    } else {
                        PositionAction::Reduce
                    }
                }
            };
            return decision_for_action(
                code,
                action,
                DecisionReason::AccountDrawdown,
                weight,
                position.qty,
                position.sellable_qty,
                effective_max_fraction,
                market,
                account_risk,
            );
        }
    }

    if experience.is_some_and(|state| state.consecutive_failed_buys > 0)
        && matches!(style, RetailStyle::LongTerm | RetailStyle::DipBuyer)
    {
        let break_even = own
            .positions
            .iter()
            .filter(|(_, position)| position.sellable_qty > 0)
            .filter_map(|(code, position)| {
                let risk = &account_risk.positions[code];
                let pnl = risk.unrealized_return?;
                (pnl.abs() <= 0.01).then_some((code, position, risk.equity_weight))
            })
            .max_by_key(|(code, _, _)| *code);
        if let Some((code, position, weight)) = break_even {
            return decision_for_action(
                code,
                PositionAction::Reduce,
                DecisionReason::BreakEvenRelief,
                weight.unwrap_or_else(|| {
                    panic!("held stock {} is missing its equity weight", code.0)
                }),
                position.qty,
                position.sellable_qty,
                effective_max_fraction,
                market,
                account_risk,
            );
        }
    }

    let Some(code) = select_observed_stock(market, own, experience, rng) else {
        return PositionDecision {
            code: None,
            action: PositionAction::Watch,
            reason: DecisionReason::NoSignal,
            target_position_fraction: 0.0,
            desired_delta_shares: 0,
            executable_delta_shares: 0,
        };
    };
    let position = own.positions.get(&code);
    let current_fraction = if position.is_some() {
        account_risk
            .positions
            .get(&code)
            .unwrap_or_else(|| panic!("account-risk observation is missing held stock {}", code.0))
            .equity_weight
            .unwrap_or_else(|| panic!("held stock {} is missing its equity weight", code.0))
    } else {
        0.0
    };
    let current_qty = position.map_or(0, |value| value.qty);
    let sellable_qty = position.map_or(0, |value| value.sellable_qty);
    if position.is_none()
        && experience.is_some_and(|state| state.is_in_post_exit_cooldown(&code, market_minute))
    {
        return decision_for_action(
            &code,
            PositionAction::Watch,
            DecisionReason::PostExitCooldown,
            0.0,
            0,
            0,
            effective_max_fraction,
            market,
            account_risk,
        );
    }
    let Some(path) = observations.price_paths.get(&code) else {
        panic!("behavior observation is missing market stock {}", code.0);
    };
    let Some(short_return) = path.thirty_minute.return_ratio else {
        // 集合竞价等尚无当日 30 分钟窗口的时点仍可存在独立的基础交易需求；
        // 其理由保持为历史不足，不能把随机需求伪装成趋势判断。
        if rng.next_f64() < strategy.arrival_rate {
            let action = if rng.next_f64() < 0.50 {
                if position.is_some() {
                    PositionAction::Add
                } else {
                    PositionAction::TryBuy
                }
            } else if position.is_some() {
                PositionAction::Reduce
            } else {
                PositionAction::Watch
            };
            return decision_for_action(
                &code,
                action,
                DecisionReason::InsufficientHistory,
                current_fraction,
                current_qty,
                sellable_qty,
                effective_max_fraction,
                market,
                account_risk,
            );
        }
        return decision_for_action(
            &code,
            if position.is_some() {
                PositionAction::Hold
            } else {
                PositionAction::Watch
            },
            DecisionReason::InsufficientHistory,
            current_fraction,
            current_qty,
            sellable_qty,
            effective_max_fraction,
            market,
            account_risk,
        );
    };

    // 非风险行为仍保留到达概率；它不能再门控已经越线的止损/止盈检查。
    if rng.next_f64() >= strategy.arrival_rate {
        return decision_for_action(
            &code,
            if position.is_some() {
                PositionAction::Hold
            } else {
                PositionAction::Watch
            },
            DecisionReason::NoSignal,
            current_fraction,
            current_qty,
            sellable_qty,
            effective_max_fraction,
            market,
            account_risk,
        );
    }

    let prior_range = path.prior_thirty_minute_range;
    let volume_confirmed = market
        .stocks
        .get(&code)
        .expect("selected stock exists")
        .relative_volume
        >= strategy.volume_confirmation;
    let range_breakout = prior_range.is_some_and(|range| range.broke_above);
    let range_breakdown = prior_range.is_some_and(|range| range.broke_below);

    let (action, reason) = match style {
        RetailStyle::Dormant => {
            if rng.next_f64() < 0.90 {
                (
                    if position.is_some() {
                        PositionAction::Hold
                    } else {
                        PositionAction::Watch
                    },
                    DecisionReason::NoSignal,
                )
            } else {
                baseline_position_action(position.is_some(), rng)
            }
        }
        RetailStyle::LongTerm => {
            if short_return <= -strategy.dip_threshold
                && path.five_day.return_ratio.is_some_and(|value| value > 0.0)
            {
                (
                    if position.is_some() {
                        PositionAction::Add
                    } else {
                        PositionAction::TryBuy
                    },
                    DecisionReason::Pullback,
                )
            } else {
                if rng.next_f64() < 0.70 {
                    (
                        if position.is_some() {
                            PositionAction::Hold
                        } else {
                            PositionAction::Watch
                        },
                        DecisionReason::NoSignal,
                    )
                } else {
                    baseline_position_action(position.is_some(), rng)
                }
            }
        }
        RetailStyle::DipBuyer => {
            if (short_return <= -strategy.dip_threshold || range_breakdown) && !broad_stress {
                (
                    if position.is_some() {
                        PositionAction::Add
                    } else {
                        PositionAction::TryBuy
                    },
                    if range_breakdown && short_return > -strategy.dip_threshold {
                        DecisionReason::RangeBreakdown
                    } else {
                        DecisionReason::Pullback
                    },
                )
            } else {
                if rng.next_f64() < 0.60 {
                    (
                        if position.is_some() {
                            PositionAction::Hold
                        } else {
                            PositionAction::Watch
                        },
                        DecisionReason::NoSignal,
                    )
                } else {
                    baseline_position_action(position.is_some(), rng)
                }
            }
        }
        RetailStyle::Momentum => {
            if (short_return >= strategy.dip_threshold || range_breakout) && volume_confirmed {
                (
                    if position.is_some() {
                        PositionAction::Add
                    } else {
                        PositionAction::TryBuy
                    },
                    if range_breakout && short_return < strategy.dip_threshold {
                        DecisionReason::RangeBreakout
                    } else {
                        DecisionReason::Momentum
                    },
                )
            } else if (short_return <= -strategy.dip_threshold || range_breakdown)
                && position.is_some()
            {
                (
                    PositionAction::Reduce,
                    if range_breakdown && short_return > -strategy.dip_threshold {
                        DecisionReason::RangeBreakdown
                    } else {
                        DecisionReason::Momentum
                    },
                )
            } else {
                (
                    if position.is_some() {
                        PositionAction::Hold
                    } else {
                        PositionAction::Watch
                    },
                    DecisionReason::NoSignal,
                )
            }
        }
        RetailStyle::Panic => {
            if broad_stress && position.is_some() && current_fraction >= 0.50 {
                (PositionAction::Reduce, DecisionReason::BroadMarketRisk)
            } else {
                if rng.next_f64() < 0.50 {
                    (
                        if position.is_some() {
                            PositionAction::Hold
                        } else {
                            PositionAction::Watch
                        },
                        if broad_stress {
                            DecisionReason::BroadMarketRisk
                        } else {
                            DecisionReason::NoSignal
                        },
                    )
                } else {
                    baseline_position_action(position.is_some(), rng)
                }
            }
        }
        RetailStyle::Noise => {
            if rng.next_f64() < 0.50 {
                (
                    if position.is_some() {
                        PositionAction::Add
                    } else {
                        PositionAction::TryBuy
                    },
                    DecisionReason::BaselinePositioning,
                )
            } else if position.is_some() {
                (PositionAction::Reduce, DecisionReason::BaselinePositioning)
            } else {
                (PositionAction::Watch, DecisionReason::BaselinePositioning)
            }
        }
    };
    let decision = decision_for_action(
        &code,
        action,
        reason,
        current_fraction,
        current_qty,
        sellable_qty,
        effective_max_fraction,
        market,
        account_risk,
    );
    apply_experience_confidence(
        decision,
        experience,
        (current_fraction, current_qty, sellable_qty),
        effective_max_fraction,
        market,
        account_risk,
        rng,
    )
}
