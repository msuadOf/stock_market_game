use super::*;

impl GameSession {
    pub(in crate::session) fn generate_next_plan_quote(
        &self,
        cursor: &mut crate::session::plan_chain_candidates::QuotePlans,
        plans: &PlanBook,
        unfinished_routes: &BTreeSet<(AccountId, StockId)>,
    ) -> Option<PlanExecutionRequest> {
        let id = cursor.account;
        let trading_day = u64::from(self.state.day);
        let lot = self.state.setup.config.lot_size;
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
            if unfinished_routes.contains(&(id, plan.code().clone())) {
                cursor.plans.push_back(plan_id);
                continue;
            }
            // 撤买观点在不可撤阶段仍保留旧单，但不能因此发出新的买入子单。
            if matches!(plan.status(), PlanStatus::Paused { .. })
                || (plan.direction() == Side::Buy && plan.opinion().signal_score_bp <= 0)
            {
                let decision = match plan.active_child_order_id() {
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
                        code: plan.code().clone(),
                        allocated_cash: Money::ZERO,
                        constraint: None,
                    },
                    decision,
                    trading_day,
                });
            }
            if !matches!(plan.status(), PlanStatus::Active) {
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
                        .find(|grant| grant.plan_id == plan.plan_id())
                })
                .cloned()
            else {
                continue;
            };
            let view = cursor
                .market
                .stocks
                .get(plan.code())
                .unwrap_or_else(|| panic!("plan stock {:?} must have a view", plan.code()));
            let price = view.last_price;
            let market = self
                .state
                .markets
                .get(plan.code())
                .unwrap_or_else(|| panic!("plan stock {:?} must have a market", plan.code()));
            let plan_code = self
                .state
                .stocks
                .code(*plan.code())
                .expect("plan stock must be cataloged");
            let stock = self
                .state
                .setup
                .stocks
                .iter()
                .find(|stock| stock.code.0 == *plan_code)
                .unwrap_or_else(|| panic!("plan stock {:?} must have a spec", plan_code));
            let (urgency, _) = self.plan_execution_urgency_at_view(
                plan,
                cursor.thirty_minute_bp.get(plan.code()).copied().flatten(),
                cursor.one_minute_bp.get(plan.code()).copied().flatten(),
                &cursor.market,
            );
            let band_up = market
                .up_stop()
                .unwrap_or_else(|error| panic!("up stop failed for {:?}: {error}", plan.code()));
            let band_down = market
                .down_stop()
                .unwrap_or_else(|error| panic!("down stop failed for {:?}: {error}", plan.code()));
            let apply_price_cage = self.phase() == TradingPhase::Continuous
                && self.state.setup.config.price_cage_enabled;
            let legal_bound = market
                .limit_order_price_bound(plan.direction(), apply_price_cage)
                .unwrap_or_else(|error| {
                    panic!(
                        "limit order price bound failed for {:?}: {error}",
                        plan.code()
                    )
                });
            // D3a-3 件4b：报价决策段提取共享（组线程首子单同一函数——
            // 报价字段不得分叉）。band/cage/合法界/protection/数量/决策/
            // affordable 复核全部收口在 decide_plan_quote。
            let active_child = plan.execution().and_then(|leg| {
                plan.active_child_order_id().map(|order_id| ActiveQuote {
                    order_id,
                    price: leg.limit_price(),
                    qty: leg.active_child_remaining_qty().unwrap_or(leg.child_qty()),
                })
            });
            let per_share = self.belief_per_share(id, plan.code());
            let sellable = cursor.sellable.get(plan.code()).copied().unwrap_or(0);
            let max_order_qty = stock.category.max_order_qty(false);
            let decision = super::decide_plan_quote(super::PlanQuoteInputs {
                account: id,
                plan,
                price,
                best_bid: view.best_bid,
                best_ask: view.best_ask,
                auction_book_fallback: matches!(
                    self.phase(),
                    TradingPhase::CallAuction | TradingPhase::ClosingAuction
                ),
                band_up,
                band_down,
                legal_bound,
                apply_price_cage,
                per_share: per_share.as_ref(),
                urgency: urgency.urgency,
                pause: urgency.pause,
                remaining,
                order_size: chain_params.order_size,
                lot,
                max_order_qty,
                sellable,
                active_child,
                cancellable_now: self.plan_child_is_cancellable_now(),
                tick_size: stock.tick,
                allocation_cash: allocation.allocated_cash,
                config: &self.state.setup.config,
            });
            let Some(decision) = decision else {
                continue;
            };
            return Some(PlanExecutionRequest {
                plan_id: plan.plan_id(),
                allocation,
                decision,
                trading_day,
            });
        }
        None
    }
}
