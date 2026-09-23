#[cfg(test)]
use super::reconcile_plan::ReconciliationPlan;
use super::reconcile_plan::WorkingOrderDecision;
use super::*;

impl GameSession {
    #[cfg(test)]
    pub(in crate::session) fn reconcile_npc_working_orders(
        &mut self,
        account: AccountId,
        desired: Vec<Intent>,
        phase: TradingPhase,
        events: &mut Vec<Event>,
    ) -> Vec<Intent> {
        let (mut continuous, mut auction) = self.working_orders_by_account();
        self.reconcile_npc_working_orders_from_index(
            account,
            desired,
            phase,
            ReconcileScope::AllWorkingOrders,
            WorkingOrderSlices {
                continuous: continuous.remove(&account).as_deref().unwrap_or(&[]),
                auction: auction.remove(&account).as_deref().unwrap_or(&[]),
            },
            events,
        )
    }

    #[cfg(test)]
    pub(in crate::session) fn plan_npc_working_orders(
        &self,
        account: AccountId,
        desired: Vec<Intent>,
        phase: TradingPhase,
    ) -> ReconciliationPlan {
        let (continuous, auction) = self.working_orders_by_account();
        self.plan_npc_working_order_reconciliation(
            desired,
            phase,
            ReconcileScope::AllWorkingOrders,
            WorkingOrderSlices {
                continuous: continuous.get(&account).map(Vec::as_slice).unwrap_or(&[]),
                auction: auction.get(&account).map(Vec::as_slice).unwrap_or(&[]),
            },
        )
    }

    #[cfg(test)]
    pub(in crate::session) fn plan_npc_working_orders_from_index(
        &self,
        desired: Vec<Intent>,
        phase: TradingPhase,
        scope: ReconcileScope,
        working: WorkingOrderSlices<'_>,
    ) -> ReconciliationPlan {
        self.plan_npc_working_order_reconciliation(desired, phase, scope, working)
    }

    pub(in crate::session) fn reconcile_npc_working_orders_from_index(
        &mut self,
        account: AccountId,
        desired: Vec<Intent>,
        phase: TradingPhase,
        scope: ReconcileScope,
        working: WorkingOrderSlices<'_>,
        events: &mut Vec<Event>,
    ) -> Vec<Intent> {
        let plan = self.plan_npc_working_order_reconciliation(desired, phase, scope, working);
        self.execute_npc_working_order_decisions(account, phase, &plan.decisions, events);
        plan.residual_intents
    }

    fn execute_npc_working_order_decisions(
        &mut self,
        account: AccountId,
        phase: TradingPhase,
        decisions: &[WorkingOrderDecision],
        events: &mut Vec<Event>,
    ) {
        for decision in decisions {
            match decision {
                WorkingOrderDecision::Keep { order_id } => {
                    let _ = order_id;
                }
                WorkingOrderDecision::Cancel { order_id, code } => match phase {
                    TradingPhase::Continuous => {
                        self.cancel_continuous_order(account, code.clone(), *order_id, events);
                    }
                    TradingPhase::CallAuction => {
                        self.cancel_auction_order(account, code.clone(), *order_id, events);
                    }
                    TradingPhase::ClosingAuction | TradingPhase::PreOpen => {}
                },
                WorkingOrderDecision::Replace {
                    old_order_id,
                    new_intent,
                } => {
                    let code = intent_code(new_intent).clone();
                    self.cancel_continuous_order(account, code, *old_order_id, events);
                }
            }
        }
    }
}

fn intent_code(intent: &Intent) -> &StockCode {
    match intent {
        Intent::PlaceLimit { code, .. }
        | Intent::PlaceMarket { code, .. }
        | Intent::Cancel { code, .. } => code,
    }
}
