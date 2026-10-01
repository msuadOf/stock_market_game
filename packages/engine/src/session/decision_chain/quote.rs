use super::*;

impl GameSession {
    pub(in crate::session) fn generate_next_plan_quote(
        &self,
        cursor: &mut crate::session::plan_chain_candidates::QuotePlans,
        plans: &PlanBook,
        unfinished_routes: &BTreeSet<(AccountId, StockCode)>,
    ) -> Option<PlanExecutionRequest> {
        let id = cursor.account;
        let trading_day = u64::from(self.day);
        let lot = self.setup.config.lot_size;
        let chain_params = self.chain_strategy_params(id);
        let available = cursor.plans.len();
        for _ in 0..available {
            let plan_id = cursor
                .plans
                .pop_front()
                .expect("quote cursor contains the counted plan");
            let plan = plans
                .plan(plan_id)
                .unwrap_or_else(|error| panic!("plan quote cursor lost its plan: {error}"));
            if unfinished_routes.contains(&(id, plan.code.clone())) {
                cursor.plans.push_back(plan_id);
                continue;
            }
            // 撤买观点在不可撤阶段仍保留旧单，但不能因此发出新的买入子单。
            if matches!(plan.status, PlanStatus::Paused { .. })
                || (plan.direction == Side::Buy && plan.opinion.signal_score_bp <= 0)
            {
                let decision = match plan.active_child_order_id {
                    Some(order_id) if self.plan_child_is_cancellable_now() => {
                        crate::plans::QuoteDecision {
                            action: crate::plans::QuoteAction::Cancel { order_id },
                            reason: crate::plans::QuoteReason::PauseRequested,
                        }
                    }
                    Some(order_id) => crate::plans::QuoteDecision {
                        action: crate::plans::QuoteAction::Keep { order_id },
                        reason: crate::plans::QuoteReason::PendingReconsideration,
                    },
                    None => crate::plans::QuoteDecision {
                        action: crate::plans::QuoteAction::Wait,
                        reason: crate::plans::QuoteReason::PauseRequested,
                    },
                };
                return Some(PlanExecutionRequest {
                    plan_id,
                    allocation: crate::plans::AllocationGrant {
                        plan_id,
                        code: plan.code.clone(),
                        allocated_cash: Money::ZERO,
                        constraint: None,
                    },
                    decision,
                    trading_day,
                });
            }
            if !matches!(plan.status, PlanStatus::Active) {
                continue;
            }
            let Some(remaining) = plan.remaining_share_qty() else {
                continue;
            };
            let Some(allocation) = cursor
                .grants
                .as_ref()
                .and_then(|result| {
                    result
                        .grants
                        .iter()
                        .find(|grant| grant.plan_id == plan.plan_id)
                })
                .cloned()
            else {
                continue;
            };
            let view = cursor
                .market
                .stocks
                .get(&plan.code)
                .unwrap_or_else(|| panic!("plan stock {:?} must have a view", plan.code));
            let price = view.last_price;
            let mut book_top = BookTop {
                best_bid: view.best_bid,
                best_ask: view.best_ask,
            };
            if book_top.best_bid.is_none()
                && book_top.best_ask.is_none()
                && matches!(
                    self.phase(),
                    TradingPhase::CallAuction | TradingPhase::ClosingAuction
                )
            {
                book_top.best_bid = Some(price);
                book_top.best_ask = Some(price);
            }
            let market = self
                .markets
                .get(&plan.code)
                .unwrap_or_else(|| panic!("plan stock {:?} must have a market", plan.code));
            let stock = self
                .setup
                .stocks
                .iter()
                .find(|stock| stock.code == plan.code)
                .unwrap_or_else(|| panic!("plan stock {:?} must have a spec", plan.code));
            let (urgency, _) = self.plan_execution_urgency_at_view(
                plan,
                cursor.thirty_minute_bp.get(&plan.code).copied().flatten(),
                cursor.one_minute_bp.get(&plan.code).copied().flatten(),
                &cursor.market,
            );
            let band_up = market
                .up_stop()
                .unwrap_or_else(|error| panic!("up stop failed for {:?}: {error}", plan.code));
            let band_down = market
                .down_stop()
                .unwrap_or_else(|error| panic!("down stop failed for {:?}: {error}", plan.code));
            let apply_price_cage =
                self.phase() == TradingPhase::Continuous && self.setup.config.price_cage_enabled;
            let legal_bound = market
                .limit_order_price_bound(plan.direction, apply_price_cage)
                .unwrap_or_else(|error| {
                    panic!(
                        "limit order price bound failed for {:?}: {error}",
                        plan.code
                    )
                });
            let cage_bound = apply_price_cage.then_some(legal_bound);
            let per_share = self.belief_per_share(id, &plan.code);
            let protection_limit = match plan.direction {
                Side::Buy => {
                    per_share.map_or(legal_bound, |range| range.optimistic.min(legal_bound))
                }
                Side::Sell => {
                    per_share.map_or(legal_bound, |range| range.pessimistic.max(legal_bound))
                }
            };
            let sellable = cursor.sellable.get(&plan.code).copied().unwrap_or(0);
            let max_order_qty = stock.category.max_order_qty(false);
            let desired_qty = match plan.direction {
                Side::Buy => super::next_routable_buy_qty(
                    remaining,
                    chain_params.order_size,
                    lot,
                    max_order_qty,
                )
                .unwrap_or_default(),
                Side::Sell => {
                    super::next_routable_sell_qty(remaining, sellable, lot, max_order_qty)
                        .unwrap_or_default()
                }
            };
            if desired_qty == 0 {
                continue;
            }
            let active_child = self
                .parent_orders
                .get(&id)
                .and_then(|parents| parents.get(&plan.code))
                .filter(|parent| parent.linked_plan_id == Some(plan.plan_id))
                .and_then(|parent| {
                    parent.active_child_order_id.map(|order_id| ActiveQuote {
                        order_id,
                        price: parent.limit_price,
                        qty: parent
                            .active_child_remaining_qty
                            .unwrap_or(parent.child_qty),
                    })
                });
            let mut quote_inputs = QuoteDecisionInputs {
                side: plan.direction,
                urgency: urgency.urgency,
                pause: urgency.pause,
                book: book_top,
                protection_limit,
                band_down,
                band_up,
                tick: stock.tick,
                cage_bound,
                desired_qty,
                lot_size: lot,
                max_order_qty,
                available_sell_qty: sellable,
                active_order: active_child,
                cancellable_now: self.plan_child_is_cancellable_now(),
            };
            let mut decision = decide_quote(&quote_inputs)
                .unwrap_or_else(|error| panic!("quote decision failed for {id:?}: {error}"));
            if plan.direction == Side::Buy {
                if let crate::plans::QuoteAction::Submit { price, qty }
                | crate::plans::QuoteAction::Replace { price, qty, .. } = decision.action
                {
                    match crate::strategy::affordable_buy_qty(
                        qty,
                        price,
                        allocation.allocated_cash,
                        &self.setup.config,
                    ) {
                        Some(funded_qty) => {
                            quote_inputs.desired_qty = funded_qty;
                            decision = decide_quote(&quote_inputs).unwrap_or_else(|error| {
                                panic!("funded quote decision failed for {id:?}: {error}")
                            });
                        }
                        None => {
                            decision = crate::plans::QuoteDecision {
                                action: match active_child {
                                    Some(active) => crate::plans::QuoteAction::Keep {
                                        order_id: active.order_id,
                                    },
                                    None => crate::plans::QuoteAction::Wait,
                                },
                                reason: crate::plans::QuoteReason::InsufficientBudget,
                            };
                        }
                    }
                }
            }
            return Some(PlanExecutionRequest {
                plan_id: plan.plan_id,
                allocation,
                decision,
                trading_day,
            });
        }
        None
    }
}
