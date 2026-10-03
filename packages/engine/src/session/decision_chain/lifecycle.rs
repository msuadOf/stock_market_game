//! 在固定账户/市场观察与当前计划/母单事实之间执行本次单账户复核。

use super::*;

/// 复核只拥有本次派生事实；PlanBook、母单与个人状态仍由私有 candidate 持有。
pub(super) struct PlanLifecycleReview<'a> {
    session: &'a GameSession,
    account: AccountId,
    market: &'a MarketView,
    plans: &'a PlanBook,
    held_by_code: BTreeMap<StockCode, u32>,
    equity: Money,
    belief: &'a BeliefBook,
    chain_params: crate::strategy::BeliefChainParams,
    trading_day: u64,
    lot: u32,
}

impl<'a> PlanLifecycleReview<'a> {
    pub(super) fn capture(
        session: &'a GameSession,
        account: AccountId,
        market: &'a MarketView,
        plans: &'a PlanBook,
    ) -> Option<Self> {
        let held_by_code = session
            .state
            .accounts
            .get(&account)
            .unwrap_or_else(|| panic!("belief account {account:?} must exist"))
            .positions()
            .iter()
            .map(|(code, position)| (code.clone(), position.qty()))
            .collect();
        let equity = session
            .account_equity(account)
            .unwrap_or_else(|error| panic!("equity for {account:?} failed: {error}"));
        if equity.cents() <= 0 {
            return None;
        }
        Some(Self {
            session,
            account,
            market,
            plans,
            held_by_code,
            equity,
            belief: session
                .state
                .belief_participants
                .get(&account)
                .map(|participant| participant.belief())
                .unwrap_or_else(|| panic!("belief account {account:?} must have a belief book")),
            chain_params: session.chain_strategy_params(account),
            trading_day: u64::from(session.state.day),
            lot: session.state.setup.config.lot_size,
        })
    }

    pub(super) fn collect(
        &self,
        assessments: &BTreeMap<StockCode, CandidateAssessment>,
    ) -> Vec<PlanLifecycleAction> {
        let mut actions = Vec::with_capacity(assessments.len());
        if self.equity.cents() <= 0 {
            return actions;
        }
        for (code, assessment) in assessments {
            actions.extend(self.assess_one(code, assessment));
        }
        actions
    }

    fn compute_target_qty(
        &self,
        code: &StockCode,
        current_weight_bp: u32,
        score: crate::plans::SignalScore,
        price: Money,
    ) -> u32 {
        let max_holdable_shares = self
            .session
            .state
            .setup
            .stocks
            .iter()
            .find(|stock| stock.code == *code)
            .map(|stock| stock.total_shares.min(u64::from(u32::MAX)))
            .unwrap_or(0);
        crate::plans::CandidateTargetProposal::from_score(
            current_weight_bp,
            score,
            self.chain_params.position_step_bp,
            self.equity,
            price,
            max_holdable_shares,
            self.lot,
        )
        .map(|proposal| proposal.target_qty())
        .unwrap_or_else(|error| match error {
            CandidateError::InvalidPositionFraction { .. } => panic!(
                "target weight failed for {:?} {code:?}: {error}",
                self.account
            ),
            _ => panic!(
                "target quantity failed for {:?} {code:?}: {error}",
                self.account
            ),
        })
    }

    fn assess_one(
        &self,
        code: &StockCode,
        assessment: &CandidateAssessment,
    ) -> Vec<PlanLifecycleAction> {
        let mut actions = Vec::new();
        let id = self.account;
        let market_view = self.market;
        let plans = self.plans;
        let trading_day = self.trading_day;
        let lot = self.lot;
        let held_by_code = &self.held_by_code;
        let equity = self.equity;
        let belief = self.belief;

        let view = market_view
            .stocks
            .get(code)
            .unwrap_or_else(|| panic!("candidate {code:?} must have a market view"));
        let price = view.last_price;
        let held_qty = held_by_code.get(code).copied().unwrap_or(0);
        let position_value = price
            .mul_shares(held_qty)
            .unwrap_or_else(|error| panic!("position value failed for {id:?}: {error}"));
        let current_weight_bp = u32::try_from(
            (i128::from(position_value.cents()) * 10_000 / i128::from(equity.cents()))
                .clamp(0, 10_000),
        )
        .expect("clamped weight fits u32");
        let score = match assessment {
            CandidateAssessment::Scored { score, .. } => Some(*score),
            CandidateAssessment::InsufficientInformation { .. } => None,
        };
        // ActiveTrader has zero fundamental weight; its plan must not require
        // a valuation it never uses. Other styles still require personal value.
        let fundamental_ready = belief.analysis().weights().fundamental_bp() == 0
            || belief
                .entry(code)
                .is_some_and(|entry| matches!(entry.valuation, ValuationOutcome::Available { .. }));
        let direction = match (fundamental_ready, score) {
            (true, Some(score))
                if score.value() >= plans.policy().reverse_revision_threshold_bp =>
            {
                Some(Side::Buy)
            }
            (true, Some(score))
                if score.value() <= -plans.policy().reverse_revision_threshold_bp =>
            {
                Some(Side::Sell)
            }
            _ => None,
        };
        let target_qty = match (score, fundamental_ready) {
            (Some(score), true) => self.compute_target_qty(code, current_weight_bp, score, price),
            _ => 0,
        };
        let active = plans.active_plan(id, code).cloned();
        match active {
            Some(plan) => {
                let plan_id = plan.plan_id();
                if plan.is_terminal() || trading_day > plan.last_valid_trading_day() {
                    return actions; // 日终扫描负责到期终止
                }
                let path = crate::observation::build_price_path_observation(
                    self.session
                        .state
                        .market_minute_closes
                        .get(code)
                        .expect("plan stock has minute history"),
                    &[],
                    None,
                )
                .unwrap_or_else(|error| {
                    panic!("plan urgency path failed for {id:?} {code:?}: {error}")
                });
                let (urgency, _risk) = self.session.plan_execution_urgency_at_view(
                    &plan,
                    ratio_to_bp(path.thirty_minute.return_ratio),
                    ratio_to_bp(path.one_minute.return_ratio),
                    market_view,
                );
                let reversing_to_sell = plan.direction() == Side::Buy
                    && direction == Some(Side::Sell)
                    && score.is_some_and(|score| {
                        reverse_crosses_threshold(
                            plan.direction(),
                            Side::Sell,
                            score.value(),
                            plans.policy().reverse_revision_threshold_bp,
                        )
                    });
                if plan.direction() == Side::Buy
                    && fundamental_ready
                    && score.is_some_and(|value| value.value() <= 0)
                    && !reversing_to_sell
                {
                    let opinion = PlanOpinion {
                        signal_score_bp: score.expect("withdrawal requires a known score").value(),
                        source: OpinionSource::Blended,
                    };
                    if opinion != plan.opinion() || urgency.urgency != plan.urgency() {
                        actions.push(PlanLifecycleAction::Revise {
                            plan_id,
                            revision: PlanRevision {
                                reason: RevisionReason::SignalShift,
                                trading_day,
                                direction: plan.direction(),
                                target: plan.target(),
                                opinion,
                                confidence_bp: plan.confidence_bp(),
                                urgency: urgency.urgency,
                                below_filled_rationale: None,
                            },
                        });
                    }
                    let child_order_id = plan.active_child_order_id();
                    if child_order_id.is_none() || self.session.plan_child_is_cancellable_now() {
                        actions.push(PlanLifecycleAction::Terminate {
                            plan_id,
                            code: code.clone(),
                            child_order_id,
                            reason: TerminationReason::Cancelled,
                            trading_day,
                        });
                    }
                    return actions;
                }
                if !reversing_to_sell {
                    match plan.status() {
                        PlanStatus::Active => {
                            if let PauseAssessment::PauseAndRequestCancel(reason) = urgency.pause {
                                actions.push(PlanLifecycleAction::ExecutionState {
                                    plan_id,
                                    event: PlanEvent::Paused {
                                        reason,
                                        trading_day,
                                    },
                                });
                                return actions;
                            }
                        }
                        PlanStatus::Paused { reason } => {
                            if reason != crate::plans::PauseReason::RiskPressure
                                && matches!(
                                    urgency.pause,
                                    PauseAssessment::PauseAndRequestCancel(
                                        crate::plans::PauseReason::RiskPressure
                                    )
                                )
                            {
                                actions.push(PlanLifecycleAction::ExecutionState {
                                    plan_id,
                                    event: PlanEvent::Paused {
                                        reason: crate::plans::PauseReason::RiskPressure,
                                        trading_day,
                                    },
                                });
                                return actions;
                            }
                            let behavior = self.session.institution_behavior(
                                id,
                                belief,
                                code,
                                market_view,
                                Some(reason),
                            );
                            let pause = if reason == crate::plans::PauseReason::RiskPressure
                                && behavior.risk_pressure_pause.is_none()
                            {
                                PauseAssessment::Unavailable {
                                    reason:
                                        "missing own equity observation for risk-pressure recovery"
                                            .to_owned(),
                                }
                            } else {
                                urgency.pause
                            };
                            let recovery = score.filter(|_| fundamental_ready).map(|score| {
                                assess_recovery(
                                    &RecoveryInputs {
                                        is_next_own_observation: true,
                                        pause,
                                        signal_score_bp: score.value(),
                                    },
                                    &self.session.state.urgency_policy,
                                )
                                .unwrap_or_else(|error| {
                                    panic!("plan recovery failed for {plan_id:?}: {error}")
                                })
                            });
                            match recovery {
                                Some(RecoveryAssessment::Resume) => {
                                    actions.push(PlanLifecycleAction::ExecutionState {
                                        plan_id,
                                        event: PlanEvent::Resumed {
                                            reason: ResumeReason::TriggerCleared,
                                            trading_day,
                                        },
                                    });
                                }
                                Some(RecoveryAssessment::ReviseOrTerminate) => {
                                    let reviewed_score =
                                        score.expect("recovery review requires a known signal");
                                    let withdraw_buy = plan.direction() == Side::Buy
                                        && reviewed_score.value() <= 0;
                                    if plan.direction() == Side::Buy {
                                        let opinion = PlanOpinion {
                                            signal_score_bp: reviewed_score.value(),
                                            source: OpinionSource::Blended,
                                        };
                                        if opinion != plan.opinion()
                                            || urgency.urgency != plan.urgency()
                                        {
                                            actions.push(PlanLifecycleAction::Revise {
                                                plan_id,
                                                revision: PlanRevision {
                                                    reason: RevisionReason::SignalShift,
                                                    trading_day,
                                                    direction: plan.direction(),
                                                    target: plan.target(),
                                                    opinion,
                                                    confidence_bp: plan.confidence_bp(),
                                                    urgency: urgency.urgency,
                                                    below_filled_rationale: None,
                                                },
                                            });
                                        } else {
                                            actions.push(PlanLifecycleAction::Observe {
                                                plan_id,
                                                trading_day,
                                            });
                                        }
                                        if withdraw_buy {
                                            let child_order_id = self
                                                .session
                                                .state
                                                .parent_orders
                                                .get(&id)
                                                .and_then(|parents| parents.get(code))
                                                .filter(|parent| {
                                                    parent.linked_plan_id() == Some(plan_id)
                                                })
                                                .and_then(|parent| parent.active_child_order_id());
                                            if child_order_id.is_none()
                                                || self.session.plan_child_is_cancellable_now()
                                            {
                                                actions.push(PlanLifecycleAction::Terminate {
                                                    plan_id,
                                                    code: code.clone(),
                                                    child_order_id,
                                                    reason: TerminationReason::Cancelled,
                                                    trading_day,
                                                });
                                            }
                                        }
                                        return actions;
                                    }
                                }
                                _ => {
                                    actions.push(PlanLifecycleAction::Observe {
                                        plan_id,
                                        trading_day,
                                    });
                                    return actions;
                                }
                            }
                        }
                        _ => unreachable!("active plan index excludes terminal states"),
                    }
                }
                let Some(score) = score.filter(|_| fundamental_ready) else {
                    actions.push(PlanLifecycleAction::Observe {
                        plan_id,
                        trading_day,
                    });
                    return actions;
                };
                let new_direction = direction.unwrap_or(plan.direction());
                let flip = new_direction != plan.direction();
                if flip
                    && !reverse_crosses_threshold(
                        plan.direction(),
                        new_direction,
                        score.value(),
                        plans.policy().reverse_revision_threshold_bp,
                    )
                {
                    actions.push(PlanLifecycleAction::Observe {
                        plan_id,
                        trading_day,
                    });
                    return actions;
                }
                let Some(delta) = desired_delta_shares(new_direction, target_qty, held_qty, lot)
                else {
                    actions.push(PlanLifecycleAction::Observe {
                        plan_id,
                        trading_day,
                    });
                    return actions;
                };
                let confidence = belief
                    .entry(code)
                    .map(|entry| u32::from(entry.confidence_bp))
                    .unwrap_or(5_000);
                let reviewed_plan = plan.review_preview(new_direction, confidence);
                let (reviewed_urgency, _) = self.session.plan_execution_urgency_at_view(
                    &reviewed_plan,
                    ratio_to_bp(path.thirty_minute.return_ratio),
                    ratio_to_bp(path.one_minute.return_ratio),
                    market_view,
                );
                let opinion = PlanOpinion {
                    signal_score_bp: score.value(),
                    source: OpinionSource::Blended,
                };
                let same_direction = !flip;
                let (_, acquired_count) = self.session.plan_review_facts(id, code, market_view);
                let price_shift_bp = plan.review().last_review_price.map(|reviewed| {
                    (i128::from(price.cents()) - i128::from(reviewed.cents())).abs() * 10_000
                        / i128::from(reviewed.cents())
                });
                let quiet = same_direction
                    && (score.value() - plan.opinion().signal_score_bp).abs()
                        < plan.review().min_signal_delta_bp
                    && price_shift_bp.is_some_and(|change| {
                        change < i128::from(plan.review().min_price_change_bp)
                    })
                    && acquired_count == plan.review().last_review_acquired_count
                    && plan.review().last_review_resources
                        == Some(self.session.plan_review_resources(id, code))
                    && plan.confidence_bp() == confidence
                    && plan.urgency() == reviewed_urgency.urgency;
                if quiet {
                    actions.push(PlanLifecycleAction::Observe {
                        plan_id,
                        trading_day,
                    });
                    return actions;
                }
                let unchanged = same_direction
                    && plan.target() == PlanTarget::ShareCount(delta)
                    && plan.confidence_bp() == confidence
                    && plan.urgency() == reviewed_urgency.urgency
                    && plan.opinion() == opinion;
                if unchanged {
                    actions.push(PlanLifecycleAction::Observe {
                        plan_id,
                        trading_day,
                    });
                    return actions;
                }
                let revision = PlanRevision {
                    reason: RevisionReason::SignalShift,
                    trading_day,
                    direction: new_direction,
                    target: PlanTarget::ShareCount(delta),
                    opinion,
                    confidence_bp: confidence,
                    urgency: reviewed_urgency.urgency,
                    below_filled_rationale: (same_direction && delta < plan.filled_qty())
                        .then_some(TerminationReason::Cancelled),
                };
                // task-24 复核移交项（本轮修复）：终止/反向修订会结束旧腿，
                // 在途子单必须先经真实路由撤销——否则子单被搁置在市场继续
                // 成交到日终，且反向后的新子单会触发
                // record_parent_order_submission 的「第二在途子单」断言。
                let will_terminate = revision.below_filled_rationale.is_some();
                let linked_parent = self
                    .session
                    .state
                    .parent_orders
                    .get(&id)
                    .and_then(|parents| parents.get(code))
                    .filter(|parent| parent.linked_plan_id() == Some(plan_id));
                let child_order_id =
                    linked_parent.and_then(|parent| parent.active_child_order_id());
                let child_would_exceed_new_target = linked_parent
                    .and_then(|parent| parent.active_child_remaining_qty())
                    .is_some_and(|remaining| {
                        plan.filled_qty()
                            .checked_add(remaining)
                            .expect("linked child exposure must fit in u32")
                            > delta
                    });
                if will_terminate || flip || child_would_exceed_new_target {
                    if child_order_id.is_some() && !self.session.plan_child_is_cancellable_now() {
                        actions.push(PlanLifecycleAction::Observe {
                            plan_id,
                            trading_day,
                        });
                        return actions;
                    }
                    actions.push(PlanLifecycleAction::Restructure {
                        account: id,
                        plan_id,
                        code: code.clone(),
                        child_order_id,
                        terminating: will_terminate,
                        revision,
                    });
                    return actions;
                }
                actions.push(PlanLifecycleAction::Revise { plan_id, revision });
            }
            None => {
                if self.session.belief_style(id)
                    == Some(crate::strategy::InstitutionStyle::ActiveTrader)
                    && self.session.phase() != TradingPhase::Continuous
                {
                    return actions;
                }
                let Some(direction) = direction else {
                    return actions;
                };
                let Some(delta) = desired_delta_shares(direction, target_qty, held_qty, lot) else {
                    return actions;
                };
                let entry = belief.entry(code);
                if entry.is_none() && belief.analysis().weights().fundamental_bp() > 0 {
                    return actions;
                }
                let intraday = self.session.belief_style(id)
                    == Some(crate::strategy::InstitutionStyle::ActiveTrader);
                let mut open = PlanOpen {
                    account: id,
                    code: code.clone(),
                    direction,
                    target: PlanTarget::ShareCount(delta),
                    opinion: PlanOpinion {
                        signal_score_bp: score.map(|value| value.value()).unwrap_or_default(),
                        source: OpinionSource::Blended,
                    },
                    confidence_bp: entry.map_or(5_000, |value| u32::from(value.confidence_bp)),
                    urgency: Urgency::Normal,
                    horizon_trading_days: if intraday {
                        1
                    } else {
                        u32::from(
                            entry
                                .expect("fundamental plan has a belief entry")
                                .horizon_trading_days,
                        )
                        .max(1)
                    },
                    created_trading_day: trading_day,
                };
                open.urgency = self.session.initial_plan_urgency(
                    id,
                    open.direction,
                    open.confidence_bp,
                    open.horizon_trading_days,
                );
                actions.push(PlanLifecycleAction::Create { open });
            }
        }
        actions
    }
}
