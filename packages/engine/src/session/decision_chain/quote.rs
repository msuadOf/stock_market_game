use super::*;

impl GameSession {
    pub(in crate::session) fn generate_next_plan_quote(
        &self,
        cursor: &mut crate::session::plan_chain_candidates::QuotePlans,
        plans: &PlanBook,
    ) -> Option<PlanExecutionRequest> {
        let id = cursor.account;
        let trading_day = u64::from(self.day);
        let lot = self.setup.config.lot_size;
        let chain_params = self.chain_strategy_params(id);
        while let Some(plan_id) = cursor.plans.pop_front() {
            let plan = plans
                .plan(plan_id)
                .unwrap_or_else(|error| panic!("plan quote cursor lost its plan: {error}"));
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
            let urgency_inputs = UrgencyInputs {
                side: plan.direction,
                return_30min_bp: cursor.thirty_minute_bp.get(&plan.code).copied().flatten(),
                return_1min_bp: cursor.one_minute_bp.get(&plan.code).copied().flatten(),
                risk_pressure_pause: false,
                adverse_selection_pause: false,
                risk_reduction_active: false,
                account_drawdown_bp: None,
                remaining_trading_days: u32::try_from(
                    plan.last_valid_trading_day().saturating_sub(trading_day),
                )
                .unwrap_or(u32::MAX),
                confidence_bp: plan.confidence_bp,
                style: match self.belief_style(id) {
                    Some(crate::strategy::InstitutionStyle::DeepValue) => PatienceStyle::DeepValue,
                    _ => PatienceStyle::Other,
                },
            };
            let urgency = assess_urgency(&urgency_inputs, &UrgencyPolicy::default())
                .unwrap_or_else(|error| panic!("urgency failed for {id:?}: {error}"));
            let band_up = market
                .up_stop()
                .unwrap_or_else(|error| panic!("up stop failed for {:?}: {error}", plan.code));
            let band_down = market
                .down_stop()
                .unwrap_or_else(|error| panic!("down stop failed for {:?}: {error}", plan.code));
            let cage_bound = market.continuous_limit_bound(plan.direction).ok();
            let per_share = self.belief_per_share(id, &plan.code);
            let mut protection_limit = match plan.direction {
                Side::Buy => per_share.map_or(band_up, |range| range.optimistic.min(band_up)),
                Side::Sell => per_share.map_or(band_down, |range| range.pessimistic.max(band_down)),
            };
            if let Some(cage) = cage_bound {
                protection_limit = match plan.direction {
                    Side::Buy => protection_limit.min(cage),
                    Side::Sell => protection_limit.max(cage),
                };
            }
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
            let child_required = match plan.direction {
                Side::Buy => buy_order_reservation(
                    &self.setup.config,
                    protection_limit,
                    desired_qty,
                    Money::ZERO,
                ),
                Side::Sell => Ok(Money::ZERO),
            }
            .unwrap_or_else(|error| panic!("child reservation failed for {id:?}: {error}"));
            if child_required > allocation.allocated_cash {
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
            let quote_inputs = QuoteDecisionInputs {
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
            let decision = decide_quote(&quote_inputs)
                .unwrap_or_else(|error| panic!("quote decision failed for {id:?}: {error}"));
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
