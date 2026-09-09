//! NPC 的可解释行为计划。
//!
//! 本模块只把权威市场观测与独立账户处境解释为目标仓位，不直接修改订单簿、成交或账户。

use std::collections::BTreeMap;

use crate::observation::{
    AccountRiskObservation, EqualWeightMarketObservation, PricePathObservation,
};
use crate::strategy::{MarketView, RetailStyle, Rng, SelfView, StrategyData};
use crate::{RetailExperienceState, StockCode};

/// 一个 tick 内可由所有 NPC 共享的只读市场背景。
#[derive(Clone, Debug, PartialEq)]
pub struct BehaviorMarketObservation {
    pub price_paths: BTreeMap<StockCode, PricePathObservation>,
    pub thirty_minute_market: EqualWeightMarketObservation,
}

/// 判断层的动作；`Hold` 与 `Watch` 都不会自动生成委托，但语义不同。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PositionAction {
    Hold,
    Watch,
    TryBuy,
    Add,
    Reduce,
    Exit,
}

/// 从实际输入生成的主要判断理由，不是事后编造的心理描述。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecisionReason {
    PositionRisk,
    TakeProfit,
    Momentum,
    Pullback,
    BroadMarketRisk,
    /// 价格以真实完整分钟窗口突破此前区间高点，且量能确认。
    RangeBreakout,
    /// 价格以真实完整分钟窗口跌破此前区间低点。
    RangeBreakdown,
    /// 账户相对其可恢复净值峰值的回撤触发整体去风险；不表示某一只股票必然亏损。
    AccountDrawdown,
    BaselinePositioning,
    NoSignal,
    InsufficientHistory,
    T1Locked,
    LowConfidence,
    PostExitCooldown,
    BreakEvenRelief,
    ProfitGiveback,
}

/// 判断层输出。数量是相对当前持仓的目标差额，正数买入、负数卖出。
#[derive(Clone, Debug, PartialEq)]
pub struct PositionDecision {
    pub code: Option<StockCode>,
    pub action: PositionAction,
    pub reason: DecisionReason,
    pub target_position_fraction: f64,
    /// 相对当前持仓的完整目标差额；正数买入、负数卖出。
    pub desired_delta_shares: i64,
    /// 本轮在 T+1 与库存边界内可交给执行层的差额；仍需现金、整手和费用校验。
    pub executable_delta_shares: i64,
}

/// 固定行情与账户输入下构造散户目标仓位；暂由测试驱动补全。
pub fn decide_retail_position(
    strategy: &StrategyData,
    style: RetailStyle,
    market: &MarketView,
    own: &SelfView,
    observations: &BehaviorMarketObservation,
    account_risk: &AccountRiskObservation,
    rng: &mut dyn Rng,
) -> PositionDecision {
    decide_retail_position_inner(
        strategy,
        style,
        market,
        own,
        observations,
        account_risk,
        None,
        0,
        rng,
    )
}

/// 在 B02 瞬时判断上叠加该自然人的真实成交/观察经历。
#[allow(clippy::too_many_arguments)]
pub fn decide_retail_position_with_experience(
    strategy: &StrategyData,
    style: RetailStyle,
    market: &MarketView,
    own: &SelfView,
    observations: &BehaviorMarketObservation,
    account_risk: &AccountRiskObservation,
    experience: &RetailExperienceState,
    market_minute: u64,
    rng: &mut dyn Rng,
) -> PositionDecision {
    decide_retail_position_inner(
        strategy,
        style,
        market,
        own,
        observations,
        account_risk,
        Some(experience),
        market_minute,
        rng,
    )
}

#[allow(clippy::too_many_arguments)]
fn decide_retail_position_inner(
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

fn baseline_position_action(
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

fn select_observed_stock(
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

fn current_position_inputs(
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
fn apply_experience_confidence(
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
fn decision_for_action(
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
